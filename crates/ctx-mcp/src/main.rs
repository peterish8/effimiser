//! `ctx-mcp` — serve the Context Runtime over MCP (newline-delimited JSON-RPC
//! on stdio). stdout carries protocol frames only; logs go to stderr.

use anyhow::{Context, Result};
use clap::Parser;
use ctx_mcp::Server;
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ctx-mcp",
    version,
    about = "Context Runtime MCP server (stdio)"
)]
struct Cli {
    /// Project root whose `.ctx/` store this server owns. Required: the server
    /// never guesses a root from its working directory.
    #[arg(long)]
    root: PathBuf,
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
    let server = Server::open(&cli.root).context("opening the context store")?;
    tracing::info!(root = %server.root().display(), "ctx-mcp ready");

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.context("reading stdin")?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(message) => server.handle_message(&message),
            Err(e) => Some(serde_json::json!({
                "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": format!("parse error: {e}") }
            })),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}
