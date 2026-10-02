# `ctx-mcp` — MCP stdio surface

`ctx-mcp --root <project-root>` serves one project's `.ctx/` store over
newline-delimited JSON-RPC 2.0 on stdio (MCP protocol `2025-06-18`). stdout
carries protocol frames only; logs go to stderr (`CTX_LOG=info` to see them).
The root is required and canonicalised; the server never infers it.

Supported methods: `initialize`, `ping`, `tools/list`, `tools/call`, and any
`notifications/*` (ignored, never answered). Malformed frames get JSON-RPC
errors (`-32700` parse, `-32600` invalid request, `-32601` unknown method,
`-32602` bad tool name or arguments).

## Tool results

Every `tools/call` result has `content` (human text, do not parse),
`structuredContent` (the contract) and `isError`. On failure,
`structuredContent` is `{ "error": { "code", "message" } }` where `code` is one
of `invalid_arguments`, `not_found`, `path_outside_root`, `spawn_failed`,
`too_large`, `corrupt`, `internal`. Branch on the code, never the message.

| Tool | Arguments | `structuredContent` |
|---|---|---|
| `ctx_run` | `argv: string[]`, `timeout_ms?` (default 120000, max 3600000) | `handle`, `argv`, `exit_code` (null on timeout/signal), `timed_out`, `duration_ms`, `stdout`/`stderr`: `{hash, byte_len, tokens}`, `tokenizer`, `hash_algorithm` |
| `ctx_output` | `handle`, `grep?`, `limit?` (default 200) | `handle`, `lines: [{stream, line, text}]`, `matched`, `shown` |
| `ctx_read_raw` | `handle`, `max_bytes?` (default 16 MiB) | `handle`, `hash_algorithm`, `blobs: [{name, hash, byte_len, base64}]` — `stdout`+`stderr` for `output://`, `content` for `file-snapshot://` |
| `ctx_snapshot` | `path` (relative, inside root) | `path`, `handle`, `hash`, `hash_algorithm`, `byte_len`, `previous_hash`, `unchanged`, `tokens`, `tokenizer` |
| `ctx_count_tokens` | `text` | `tokens`, `tokenizer`, `is_proxy_for_claude` |
| `ctx_stats` | — | `root`, `raw_bytes_held` |

## Guarantees

- `ctx_run` spawns `argv` directly, never through a shell, with the root as
  working directory and stdin closed. On timeout the child is killed and the
  output captured so far is still stored behind the handle.
- `ctx_read_raw` re-hashes every blob before returning it and reports
  `corrupt` rather than returning bytes that no longer match their address.
- `ctx_snapshot` refuses absolute paths, `..` components, and symlinks that
  resolve outside the root (`path_outside_root`).
- Token counts always name their tokenizer; `o200k_base` is a proxy for
  Claude, not an exact count.

## Not exposed yet

`tool://` and `git://` handles are parsed but not stored; reading one returns
`invalid_arguments`. The Rust symbol index (`ctx-index`) is not served.
