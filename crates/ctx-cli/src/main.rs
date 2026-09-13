//! `ctx` — the Context Runtime command line.
//!
//! Phase 1 surface: initialise a store, report what it holds, capture a
//! command's full output behind a handle, and read that raw output back.
//! Everything here is local; no network calls, no cloud services.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ctx_core::handle::Handle;
use ctx_core::tokens::TokenCounter;
use ctx_store::Store;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "ctx",
    version,
    about = "Local context runtime for AI coding agents",
    long_about = None
)]
struct Cli {
    /// Repository root. Defaults to the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create the local store and report what was detected.
    Init,
    /// Report what the store holds and what it has withheld.
    Stats {
        /// Emit machine-readable JSON instead of a human summary.
        #[arg(long)]
        json: bool,
    },
    /// Run a command, capturing its full output behind a handle.
    ///
    /// The full stdout/stderr is stored locally; this prints a small summary
    /// plus the handle needed to recover the original bytes.
    Run {
        /// The command and its arguments.
        #[arg(trailing_var_arg = true, required = true)]
        argv: Vec<String>,
    },
    /// Read captured output back by handle.
    Output {
        /// A handle such as `output://run/3f9a1c2b`.
        handle: String,
        /// Print only lines containing this substring.
        #[arg(long)]
        grep: Option<String>,
        /// Print at most this many matching lines.
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CTX_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let root = match cli.root {
        Some(r) => r,
        None => std::env::current_dir().context("resolving current directory")?,
    };

    match cli.command {
        Command::Init => cmd_init(&root),
        Command::Stats { json } => cmd_stats(&root, json),
        Command::Run { argv } => cmd_run(&root, &argv),
        Command::Output {
            handle,
            grep,
            limit,
        } => cmd_output(&root, &handle, grep.as_deref(), limit),
    }
}

fn cmd_init(root: &Path) -> Result<()> {
    let store = Store::open(root)?;
    let counter = TokenCounter::default_counter()?;
    println!("Context Runtime initialised");
    println!("  root:       {}", root.display());
    println!("  store:      {}", root.join(".ctx").display());
    println!("  tokenizer:  {}", counter.tokenizer().as_str());
    println!("  raw bytes held: {}", store.total_raw_bytes()?);
    Ok(())
}

fn cmd_stats(root: &Path, json: bool) -> Result<()> {
    let store = Store::open(root)?;
    let raw_bytes = store.total_raw_bytes()?;
    let counter = TokenCounter::default_counter()?;

    if json {
        let v = serde_json::json!({
            "root": root.display().to_string(),
            "raw_bytes_held": raw_bytes,
            "tokenizer": counter.tokenizer().as_str(),
            "tokenizer_is_proxy_for_claude": counter.tokenizer().is_proxy_for_claude(),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        println!("raw bytes held locally: {raw_bytes}");
        println!(
            "token counts reported using: {} (proxy tokenizer, not Claude's own)",
            counter.tokenizer().as_str()
        );
    }
    Ok(())
}

/// Run a child process directly (no shell), capture everything, and report a
/// summary plus a recovery handle.
fn cmd_run(root: &Path, argv: &[String]) -> Result<()> {
    let store = Store::open(root)?;
    let (program, args) = argv
        .split_first()
        .context("run needs at least a program name")?;

    // Load the vocabulary while the child runs, not after it exits.
    // Construction costs ~250-400 ms (measured; see docs/benchmarks/LOOP_LOG.md)
    // and needs nothing the child produces, so serialising the two simply adds
    // that to every captured command. Overlapped, it is free for any command
    // slower than the load, and no worse than before for anything faster.
    let counter_load = std::thread::spawn(TokenCounter::default_counter);

    let started = std::time::Instant::now();
    // Spawned via Command::args, never through a shell, so arguments cannot be
    // reinterpreted as shell syntax.
    let out = std::process::Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("running {program}"))?;
    let elapsed = started.elapsed().as_millis() as u64;

    let command_line = argv.join(" ");
    let handle = store.put_run(
        &command_line,
        &root.display().to_string(),
        out.status.code(),
        &out.stdout,
        &out.stderr,
        elapsed,
    )?;

    // What the model would see: a small summary plus a recovery handle. The
    // full bytes stay in the store.
    let counter = counter_load
        .join()
        .map_err(|_| anyhow::anyhow!("the tokenizer loader thread panicked"))??;
    let raw_bytes = out.stdout.len() + out.stderr.len();
    let stdout_text = String::from_utf8_lossy(&out.stdout);
    let raw_tokens = counter.count(&stdout_text).tokens;

    println!("$ {command_line}");
    println!(
        "exit {} in {elapsed} ms",
        out.status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".into())
    );
    println!(
        "captured {raw_bytes} bytes of output ({raw_tokens} {} tokens) — not shown",
        counter.tokenizer().as_str()
    );
    println!("raw: {handle}");
    Ok(())
}

fn cmd_output(root: &Path, handle: &str, grep: Option<&str>, limit: usize) -> Result<()> {
    let store = Store::open(root)?;
    let handle: Handle = handle
        .parse()
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("parsing handle")?;

    let (stdout, stderr) = store.run_output(&handle)?;
    let text = String::from_utf8_lossy(&stdout);
    let err_text = String::from_utf8_lossy(&stderr);

    let mut shown = 0usize;
    let mut matched = 0usize;
    for (stream, body) in [("stdout", text.as_ref()), ("stderr", err_text.as_ref())] {
        for (i, line) in body.lines().enumerate() {
            let hit = match grep {
                Some(q) => line.contains(q),
                None => true,
            };
            if !hit {
                continue;
            }
            matched += 1;
            if shown < limit {
                println!("{stream}:{}: {line}", i + 1);
                shown += 1;
            }
        }
    }
    if matched > shown {
        println!("... {} more matching lines (raise --limit)", matched - shown);
    }
    Ok(())
}
