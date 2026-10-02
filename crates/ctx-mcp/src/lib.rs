//! ctx-mcp — a resident MCP server over newline-delimited JSON-RPC on stdio.
//!
//! The server exposes the runtime's built pieces as structured tool calls so a
//! non-Rust host (an editor, an orchestrator) can use them without scraping
//! human text:
//!
//! | tool               | what it does                                              |
//! |--------------------|-----------------------------------------------------------|
//! | `ctx_run`          | run an argv (no shell) under the root, capture behind a handle |
//! | `ctx_output`       | read captured lines back, optionally filtered             |
//! | `ctx_read_raw`     | byte-exact, hash-verified bytes behind a handle (base64)  |
//! | `ctx_snapshot`     | record a file's current content, report unchanged/changed |
//! | `ctx_count_tokens` | count tokens with a labelled real BPE                     |
//! | `ctx_stats`        | what the store holds                                      |
//!
//! Every tool result carries `structuredContent`; the `content` text is a
//! convenience for humans and must not be parsed. stdout is protocol data
//! only; logs go to stderr.

use anyhow::{Context, Result};
use base64::Engine as _;
use ctx_core::handle::{Handle, HandleKind};
use ctx_core::tokens::TokenCounter;
use ctx_store::Store;
use serde_json::{json, Map, Value};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// MCP protocol revision this server speaks.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Default and maximum wall-clock budget for `ctx_run`.
const DEFAULT_RUN_TIMEOUT_MS: u64 = 120_000;
const MAX_RUN_TIMEOUT_MS: u64 = 3_600_000;

/// Default cap on bytes returned by `ctx_read_raw` in one call.
const DEFAULT_READ_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Typed failure codes carried in a tool error's structured content. Callers
/// branch on these, never on the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolErrorCode {
    InvalidArguments,
    NotFound,
    PathOutsideRoot,
    SpawnFailed,
    TooLarge,
    Corrupt,
    Internal,
}

impl ToolErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolErrorCode::InvalidArguments => "invalid_arguments",
            ToolErrorCode::NotFound => "not_found",
            ToolErrorCode::PathOutsideRoot => "path_outside_root",
            ToolErrorCode::SpawnFailed => "spawn_failed",
            ToolErrorCode::TooLarge => "too_large",
            ToolErrorCode::Corrupt => "corrupt",
            ToolErrorCode::Internal => "internal",
        }
    }
}

#[derive(Debug)]
pub struct ToolError {
    pub code: ToolErrorCode,
    pub message: String,
}

impl ToolError {
    fn new(code: ToolErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self::new(ToolErrorCode::InvalidArguments, message)
    }
    fn internal(err: anyhow::Error) -> Self {
        Self::new(ToolErrorCode::Internal, format!("{err:#}"))
    }
}

type ToolResult = std::result::Result<(String, Value), ToolError>;

/// The server state: one root, one store, one lazily loaded tokenizer.
pub struct Server {
    root: PathBuf,
    store: Store,
    counter: OnceLock<std::result::Result<TokenCounter, String>>,
}

impl Server {
    /// Open the store under `root/.ctx`. The root is canonicalised so path
    /// containment checks compare like with like.
    pub fn open(root: &Path) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("resolving root {}", root.display()))?;
        let store = Store::open(&root)?;
        Ok(Self {
            root,
            store,
            counter: OnceLock::new(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn counter(&self) -> std::result::Result<&TokenCounter, ToolError> {
        self.counter
            .get_or_init(|| TokenCounter::default_counter().map_err(|e| format!("{e:#}")))
            .as_ref()
            .map_err(|e| ToolError::new(ToolErrorCode::Internal, e.clone()))
    }

    /// Handle one decoded JSON-RPC message. Returns the response to write, or
    /// `None` for a notification.
    pub fn handle_message(&self, message: &Value) -> Option<Value> {
        let Some(obj) = message.as_object() else {
            return Some(rpc_error(
                Value::Null,
                -32600,
                "request must be a JSON object",
            ));
        };
        let id = obj.get("id").cloned();
        let is_notification = id.is_none();
        let id = id.unwrap_or(Value::Null);
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return (!is_notification).then(|| rpc_error(id, -32600, "jsonrpc must be \"2.0\""));
        }
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            return (!is_notification).then(|| rpc_error(id, -32600, "method must be a string"));
        };
        let params = obj.get("params").cloned().unwrap_or(json!({}));

        let outcome = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "ctx-mcp", "version": env!("CARGO_PKG_VERSION") },
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => self.call_tool(&params),
            m if m.starts_with("notifications/") => return None,
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        if is_notification {
            return None;
        }
        Some(match outcome {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, msg)) => rpc_error(id, code, &msg),
        })
    }

    fn call_tool(&self, params: &Value) -> std::result::Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((-32602, "tools/call needs a string `name`".to_string()))?;
        let empty = Map::new();
        let args = match params.get("arguments") {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(m)) => m,
            Some(_) => return Err((-32602, "`arguments` must be an object".into())),
        };
        let result = match name {
            "ctx_run" => self.tool_run(args),
            "ctx_output" => self.tool_output(args),
            "ctx_read_raw" => self.tool_read_raw(args),
            "ctx_snapshot" => self.tool_snapshot(args),
            "ctx_count_tokens" => self.tool_count_tokens(args),
            "ctx_stats" => self.tool_stats(),
            _ => return Err((-32602, format!("unknown tool: {name}"))),
        };
        Ok(match result {
            Ok((text, structured)) => json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": structured,
                "isError": false,
            }),
            Err(e) => json!({
                "content": [{ "type": "text", "text": format!("{}: {}", e.code.as_str(), e.message) }],
                "structuredContent": { "error": { "code": e.code.as_str(), "message": e.message } },
                "isError": true,
            }),
        })
    }

    fn tool_run(&self, args: &Map<String, Value>) -> ToolResult {
        let argv = string_array(args, "argv")?;
        let Some((program, rest)) = argv.split_first() else {
            return Err(ToolError::invalid("argv must name a program"));
        };
        if program.is_empty() {
            return Err(ToolError::invalid("argv[0] must be non-empty"));
        }
        let timeout_ms = optional_u64(args, "timeout_ms")?.unwrap_or(DEFAULT_RUN_TIMEOUT_MS);
        if timeout_ms == 0 || timeout_ms > MAX_RUN_TIMEOUT_MS {
            return Err(ToolError::invalid(format!(
                "timeout_ms must be in 1..={MAX_RUN_TIMEOUT_MS}"
            )));
        }

        let started = Instant::now();
        // Spawned with an argument vector, never through a shell.
        let mut child = Command::new(program)
            .args(rest)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::new(ToolErrorCode::SpawnFailed, format!("{program}: {e}")))?;
        let stdout_reader = Drain::start(child.stdout.take());
        let stderr_reader = Drain::start(child.stderr.take());

        let deadline = started + Duration::from_millis(timeout_ms);
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() >= deadline => {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait().ok();
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(ToolError::internal(e.into())),
            }
        };
        // After a timeout a grandchild may still hold the pipes open; take what
        // arrived instead of waiting for it.
        let grace = timed_out.then(|| Duration::from_millis(250));
        let stdout = stdout_reader.finish(grace);
        let stderr = stderr_reader.finish(grace);
        let duration_ms = started.elapsed().as_millis() as u64;
        let exit_code = if timed_out {
            None
        } else {
            status.and_then(|s| s.code())
        };

        let command_line = argv.join(" ");
        let handle = self
            .store
            .put_run(
                &command_line,
                &self.root.display().to_string(),
                exit_code,
                &stdout,
                &stderr,
                duration_ms,
            )
            .map_err(ToolError::internal)?;
        let (stdout_hash, stderr_hash) = self
            .store
            .run_hashes(&handle)
            .map_err(ToolError::internal)?
            .ok_or_else(|| ToolError::new(ToolErrorCode::Internal, "run vanished after capture"))?;

        let counter = self.counter()?;
        let stdout_tokens = counter.count(&String::from_utf8_lossy(&stdout)).tokens;
        let stderr_tokens = counter.count(&String::from_utf8_lossy(&stderr)).tokens;
        let tokenizer = counter.tokenizer().as_str();

        let text = format!(
            "$ {command_line}\n{} in {duration_ms} ms\ncaptured {} bytes ({} {tokenizer} tokens) — not shown\nraw: {handle}",
            match (timed_out, exit_code) {
                (true, _) => "timed out".to_string(),
                (false, Some(c)) => format!("exit {c}"),
                (false, None) => "terminated by signal".to_string(),
            },
            stdout.len() + stderr.len(),
            stdout_tokens + stderr_tokens,
        );
        Ok((
            text,
            json!({
                "handle": handle.to_string(),
                "argv": argv,
                "exit_code": exit_code,
                "timed_out": timed_out,
                "duration_ms": duration_ms,
                "stdout": { "hash": stdout_hash, "byte_len": stdout.len(), "tokens": stdout_tokens },
                "stderr": { "hash": stderr_hash, "byte_len": stderr.len(), "tokens": stderr_tokens },
                "tokenizer": tokenizer,
                "hash_algorithm": "blake3",
            }),
        ))
    }

    fn tool_output(&self, args: &Map<String, Value>) -> ToolResult {
        let handle = parse_handle(args)?;
        if handle.kind() != HandleKind::Output {
            return Err(ToolError::invalid("ctx_output needs an output:// handle"));
        }
        let grep = optional_str(args, "grep")?;
        let limit = optional_u64(args, "limit")?.unwrap_or(200) as usize;
        if self
            .store
            .get_run(&handle)
            .map_err(ToolError::internal)?
            .is_none()
        {
            return Err(ToolError::new(
                ToolErrorCode::NotFound,
                format!("no such run: {handle}"),
            ));
        }
        let (stdout, stderr) = self
            .store
            .run_output(&handle)
            .map_err(ToolError::internal)?;

        let mut lines = Vec::new();
        let mut text = String::new();
        let mut matched = 0usize;
        for (stream, body) in [("stdout", &stdout), ("stderr", &stderr)] {
            for (i, line) in String::from_utf8_lossy(body).lines().enumerate() {
                if grep.is_some_and(|q| !line.contains(q)) {
                    continue;
                }
                matched += 1;
                if lines.len() < limit {
                    text.push_str(&format!("{stream}:{}: {line}\n", i + 1));
                    lines.push(json!({ "stream": stream, "line": i + 1, "text": line }));
                }
            }
        }
        let shown = lines.len();
        Ok((
            text,
            json!({ "handle": handle.to_string(), "lines": lines, "matched": matched, "shown": shown }),
        ))
    }

    fn tool_read_raw(&self, args: &Map<String, Value>) -> ToolResult {
        let handle = parse_handle(args)?;
        let max_bytes = optional_u64(args, "max_bytes")?.unwrap_or(DEFAULT_READ_MAX_BYTES);
        let parts: Vec<(&str, String)> = match handle.kind() {
            HandleKind::Output => {
                let (so, se) = self
                    .store
                    .run_hashes(&handle)
                    .map_err(|e| ToolError::invalid(format!("{e:#}")))?
                    .ok_or_else(|| {
                        ToolError::new(ToolErrorCode::NotFound, format!("no such run: {handle}"))
                    })?;
                vec![("stdout", so), ("stderr", se)]
            }
            HandleKind::FileSnapshot => {
                let hash = handle.path().to_string();
                let known = self.store.blob_meta(&hash).map_err(ToolError::internal)?;
                if known.is_none() {
                    return Err(ToolError::new(
                        ToolErrorCode::NotFound,
                        format!("no such snapshot: {handle}"),
                    ));
                }
                vec![("content", hash)]
            }
            HandleKind::Tool | HandleKind::Git => {
                return Err(ToolError::invalid(format!(
                    "{} handles are not stored by this runtime yet",
                    handle.kind().scheme()
                )))
            }
        };

        let mut total = 0u64;
        for (_, hash) in &parts {
            if let Some(meta) = self.store.blob_meta(hash).map_err(ToolError::internal)? {
                total += meta.byte_len;
            }
        }
        if total > max_bytes {
            return Err(ToolError::new(
                ToolErrorCode::TooLarge,
                format!("{total} bytes exceed max_bytes {max_bytes}"),
            ));
        }

        let mut blobs = Vec::new();
        for (name, hash) in parts {
            let bytes = self
                .store
                .get_blob(&hash)
                .map_err(|e| ToolError::new(ToolErrorCode::NotFound, format!("{e:#}")))?;
            let actual = ctx_store::blobs::hash(&bytes);
            if actual != hash {
                return Err(ToolError::new(
                    ToolErrorCode::Corrupt,
                    format!("blob {hash} is corrupt: its bytes hash to {actual}"),
                ));
            }
            blobs.push(json!({
                "name": name,
                "hash": hash,
                "byte_len": bytes.len(),
                "base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
            }));
        }
        Ok((
            format!("{handle}: {total} bytes, verified"),
            json!({ "handle": handle.to_string(), "hash_algorithm": "blake3", "blobs": blobs }),
        ))
    }

    fn tool_snapshot(&self, args: &Map<String, Value>) -> ToolResult {
        let requested = required_str(args, "path")?;
        let (relative, absolute) = self.resolve_inside_root(requested)?;
        let bytes = std::fs::read(&absolute)
            .map_err(|e| ToolError::new(ToolErrorCode::NotFound, format!("{relative}: {e}")))?;
        let previous = self
            .store
            .last_seen_hash(&relative)
            .map_err(ToolError::internal)?;
        let hash = self
            .store
            .record_snapshot(&relative, &bytes)
            .map_err(ToolError::internal)?;
        let unchanged = previous.as_deref() == Some(hash.as_str());
        let counter = self.counter()?;
        let tokens = counter.count(&String::from_utf8_lossy(&bytes)).tokens;
        let handle = Handle::file_snapshot(&hash);
        Ok((
            format!(
                "{relative}: {} ({} bytes)\nraw: {handle}",
                if unchanged { "unchanged" } else { "recorded" },
                bytes.len()
            ),
            json!({
                "path": relative,
                "handle": handle.to_string(),
                "hash": hash,
                "hash_algorithm": "blake3",
                "byte_len": bytes.len(),
                "previous_hash": previous,
                "unchanged": unchanged,
                "tokens": tokens,
                "tokenizer": counter.tokenizer().as_str(),
            }),
        ))
    }

    fn tool_count_tokens(&self, args: &Map<String, Value>) -> ToolResult {
        let text = required_str(args, "text")?;
        let counter = self.counter()?;
        let count = counter.count(text);
        Ok((
            format!("{} {} tokens", count.tokens, counter.tokenizer().as_str()),
            json!({
                "tokens": count.tokens,
                "tokenizer": counter.tokenizer().as_str(),
                "is_proxy_for_claude": counter.tokenizer().is_proxy_for_claude(),
            }),
        ))
    }

    fn tool_stats(&self) -> ToolResult {
        let raw = self.store.total_raw_bytes().map_err(ToolError::internal)?;
        Ok((
            format!("raw bytes held locally: {raw}"),
            json!({ "root": self.root.display().to_string(), "raw_bytes_held": raw }),
        ))
    }

    /// Resolve a caller-supplied path against the root and refuse anything
    /// that would leave it, including via `..` or a symlink.
    fn resolve_inside_root(
        &self,
        requested: &str,
    ) -> std::result::Result<(String, PathBuf), ToolError> {
        let candidate = Path::new(requested);
        if candidate.is_absolute()
            || candidate
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(ToolError::new(
                ToolErrorCode::PathOutsideRoot,
                format!("{requested} is not a relative path inside the root"),
            ));
        }
        let joined = self.root.join(candidate);
        let resolved = joined
            .canonicalize()
            .map_err(|e| ToolError::new(ToolErrorCode::NotFound, format!("{requested}: {e}")))?;
        let relative = resolved.strip_prefix(&self.root).map_err(|_| {
            ToolError::new(
                ToolErrorCode::PathOutsideRoot,
                format!("{requested} resolves outside the root"),
            )
        })?;
        let relative = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        Ok((relative, resolved))
    }
}

/// Collects a pipe into a shared buffer so a caller can take what arrived even
/// when a grandchild keeps the pipe open after the direct child was killed.
struct Drain {
    buf: Arc<Mutex<Vec<u8>>>,
    done: mpsc::Receiver<()>,
}

impl Drain {
    fn start<R: Read + Send + 'static>(pipe: Option<R>) -> Self {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let (tx, done) = mpsc::channel();
        let sink = Arc::clone(&buf);
        std::thread::spawn(move || {
            if let Some(mut p) = pipe {
                let mut chunk = [0u8; 64 * 1024];
                loop {
                    match p.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => sink
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .extend_from_slice(&chunk[..n]),
                    }
                }
            }
            let _ = tx.send(());
        });
        Self { buf, done }
    }

    /// Wait until the pipe closes, or at most `grace` when given, then return
    /// the bytes collected so far.
    fn finish(self, grace: Option<Duration>) -> Vec<u8> {
        match grace {
            None => {
                let _ = self.done.recv();
            }
            Some(g) => {
                let _ = self.done.recv_timeout(g);
            }
        }
        std::mem::take(&mut *self.buf.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn parse_handle(args: &Map<String, Value>) -> std::result::Result<Handle, ToolError> {
    required_str(args, "handle")?
        .parse::<Handle>()
        .map_err(|e| ToolError::invalid(e.to_string()))
}

fn required_str<'a>(
    args: &'a Map<String, Value>,
    key: &str,
) -> std::result::Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid(format!("`{key}` must be a string")))
}

fn optional_str<'a>(
    args: &'a Map<String, Value>,
    key: &str,
) -> std::result::Result<Option<&'a str>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(ToolError::invalid(format!("`{key}` must be a string"))),
    }
}

fn optional_u64(
    args: &Map<String, Value>,
    key: &str,
) -> std::result::Result<Option<u64>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| ToolError::invalid(format!("`{key}` must be a non-negative integer"))),
    }
}

fn string_array(
    args: &Map<String, Value>,
    key: &str,
) -> std::result::Result<Vec<String>, ToolError> {
    let items = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| ToolError::invalid(format!("`{key}` must be an array of strings")))?;
    items
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| ToolError::invalid(format!("`{key}` must be an array of strings")))
        })
        .collect()
}

/// The tool catalogue returned by `tools/list`.
pub fn tool_definitions() -> Value {
    json!([
        {
            "name": "ctx_run",
            "description": "Run a program with arguments (no shell) in the project root and capture its full output behind a handle.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "argv": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                    "timeout_ms": { "type": "integer", "minimum": 1, "maximum": MAX_RUN_TIMEOUT_MS }
                },
                "required": ["argv"],
                "additionalProperties": false
            }
        },
        {
            "name": "ctx_output",
            "description": "Read captured output lines back by handle, optionally filtered by substring.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "handle": { "type": "string" },
                    "grep": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 0 }
                },
                "required": ["handle"],
                "additionalProperties": false
            }
        },
        {
            "name": "ctx_read_raw",
            "description": "Return the byte-exact, hash-verified bytes behind an output:// or file-snapshot:// handle as base64.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "handle": { "type": "string" },
                    "max_bytes": { "type": "integer", "minimum": 0 }
                },
                "required": ["handle"],
                "additionalProperties": false
            }
        },
        {
            "name": "ctx_snapshot",
            "description": "Record the current content of a file inside the root and report whether it changed since last seen.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
                "additionalProperties": false
            }
        },
        {
            "name": "ctx_count_tokens",
            "description": "Count tokens in text with a real BPE; the result names the tokenizer.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"],
                "additionalProperties": false
            }
        },
        {
            "name": "ctx_stats",
            "description": "Report how many raw bytes the store holds.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> (tempfile::TempDir, Server) {
        let dir = tempfile::tempdir().unwrap();
        let s = Server::open(dir.path()).unwrap();
        (dir, s)
    }

    fn call(s: &Server, name: &str, args: Value) -> Value {
        let resp = s
            .handle_message(&json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": name, "arguments": args }
            }))
            .unwrap();
        resp["result"].clone()
    }

    fn decode(b64: &Value) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(b64.as_str().unwrap())
            .unwrap()
    }

    #[test]
    fn initialize_reports_protocol_and_tools() {
        let (_d, s) = server();
        let init = s
            .handle_message(&json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}))
            .unwrap();
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSION);
        let list = s
            .handle_message(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .unwrap();
        let names: Vec<_> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&"ctx_read_raw".to_string()));
        assert!(names.contains(&"ctx_snapshot".to_string()));
    }

    #[test]
    fn notifications_get_no_response() {
        let (_d, s) = server();
        assert!(s
            .handle_message(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .is_none());
    }

    #[test]
    fn unknown_method_and_bad_frames_are_rpc_errors() {
        let (_d, s) = server();
        let r = s
            .handle_message(&json!({"jsonrpc":"2.0","id":7,"method":"nope"}))
            .unwrap();
        assert_eq!(r["error"]["code"], -32601);
        let r = s.handle_message(&json!([1, 2])).unwrap();
        assert_eq!(r["error"]["code"], -32600);
        let r = s
            .handle_message(&json!({"jsonrpc":"1.0","id":1,"method":"ping"}))
            .unwrap();
        assert_eq!(r["error"]["code"], -32600);
    }

    #[cfg(unix)]
    #[test]
    fn run_then_read_raw_is_byte_exact_and_hash_verified() {
        let (_d, s) = server();
        let r = call(
            &s,
            "ctx_run",
            json!({"argv": ["sh", "-c", "printf 'a\\nb\\n'; printf oops >&2; exit 3"]}),
        );
        assert_eq!(r["isError"], false);
        let sc = &r["structuredContent"];
        assert_eq!(sc["exit_code"], 3);
        assert_eq!(sc["stdout"]["byte_len"], 4);
        let handle = sc["handle"].as_str().unwrap();

        let raw = call(&s, "ctx_read_raw", json!({"handle": handle}));
        let blobs = raw["structuredContent"]["blobs"].as_array().unwrap();
        assert_eq!(decode(&blobs[0]["base64"]), b"a\nb\n");
        assert_eq!(decode(&blobs[1]["base64"]), b"oops");
        assert_eq!(blobs[0]["hash"], sc["stdout"]["hash"]);

        let out = call(&s, "ctx_output", json!({"handle": handle, "grep": "b"}));
        assert_eq!(out["structuredContent"]["matched"], 1);
        assert_eq!(out["structuredContent"]["lines"][0]["text"], "b");
    }

    #[cfg(unix)]
    #[test]
    fn run_times_out_and_still_captures() {
        let (_d, s) = server();
        let r = call(
            &s,
            "ctx_run",
            json!({"argv": ["sh", "-c", "echo started; sleep 5"], "timeout_ms": 200}),
        );
        let sc = &r["structuredContent"];
        assert_eq!(sc["timed_out"], true);
        assert!(sc["exit_code"].is_null());
        assert!(
            sc["duration_ms"].as_u64().unwrap() < 2_000,
            "a grandchild holding the pipe must not stall capture"
        );
    }

    #[test]
    fn missing_program_is_a_typed_spawn_failure() {
        let (_d, s) = server();
        let r = call(
            &s,
            "ctx_run",
            json!({"argv": ["definitely-not-a-real-program-xyz"]}),
        );
        assert_eq!(r["isError"], true);
        assert_eq!(r["structuredContent"]["error"]["code"], "spawn_failed");
    }

    #[test]
    fn snapshot_detects_unchanged_and_changed_content() {
        let (d, s) = server();
        std::fs::create_dir_all(d.path().join("src")).unwrap();
        std::fs::write(d.path().join("src/a.rs"), b"fn a() {}\n").unwrap();
        let first = call(&s, "ctx_snapshot", json!({"path": "src/a.rs"}));
        assert_eq!(first["structuredContent"]["unchanged"], false);
        assert_eq!(first["structuredContent"]["path"], "src/a.rs");
        let again = call(&s, "ctx_snapshot", json!({"path": "src/a.rs"}));
        assert_eq!(again["structuredContent"]["unchanged"], true);
        std::fs::write(d.path().join("src/a.rs"), b"fn b() {}\n").unwrap();
        let changed = call(&s, "ctx_snapshot", json!({"path": "src/a.rs"}));
        assert_eq!(changed["structuredContent"]["unchanged"], false);

        let handle = first["structuredContent"]["handle"].as_str().unwrap();
        let raw = call(&s, "ctx_read_raw", json!({"handle": handle}));
        assert_eq!(
            decode(&raw["structuredContent"]["blobs"][0]["base64"]),
            b"fn a() {}\n",
            "the first snapshot stays recoverable after the file changes"
        );
    }

    #[test]
    fn snapshot_refuses_paths_outside_the_root() {
        let (_d, s) = server();
        for p in ["../etc/passwd", "/etc/passwd"] {
            let r = call(&s, "ctx_snapshot", json!({"path": p}));
            assert_eq!(
                r["structuredContent"]["error"]["code"], "path_outside_root",
                "{p}"
            );
        }
    }

    #[test]
    fn unknown_handles_are_not_found() {
        let (_d, s) = server();
        let r = call(
            &s,
            "ctx_read_raw",
            json!({"handle": "output://run/deadbeef"}),
        );
        assert_eq!(r["structuredContent"]["error"]["code"], "not_found");
        let r = call(
            &s,
            "ctx_read_raw",
            json!({"handle": format!("file-snapshot://{}", "0".repeat(64))}),
        );
        assert_eq!(r["structuredContent"]["error"]["code"], "not_found");
        let r = call(&s, "ctx_output", json!({"handle": "output://run/deadbeef"}));
        assert_eq!(r["structuredContent"]["error"]["code"], "not_found");
        let r = call(&s, "ctx_read_raw", json!({"handle": "garbage"}));
        assert_eq!(r["structuredContent"]["error"]["code"], "invalid_arguments");
    }

    #[test]
    fn count_tokens_names_its_tokenizer() {
        let (_d, s) = server();
        let r = call(&s, "ctx_count_tokens", json!({"text": "hello world"}));
        assert_eq!(r["structuredContent"]["tokenizer"], "o200k_base");
        assert_eq!(r["structuredContent"]["tokens"], 2);
        assert_eq!(r["structuredContent"]["is_proxy_for_claude"], true);
    }

    #[test]
    fn read_raw_enforces_max_bytes() {
        let (d, s) = server();
        std::fs::write(d.path().join("big.txt"), vec![b'x'; 1000]).unwrap();
        let snap = call(&s, "ctx_snapshot", json!({"path": "big.txt"}));
        let handle = snap["structuredContent"]["handle"].as_str().unwrap();
        let r = call(
            &s,
            "ctx_read_raw",
            json!({"handle": handle, "max_bytes": 10}),
        );
        assert_eq!(r["structuredContent"]["error"]["code"], "too_large");
    }
}
