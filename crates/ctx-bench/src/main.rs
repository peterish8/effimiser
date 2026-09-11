//! `ctx-bench` — runs the deterministic microbenchmark suite.
//!
//! These are the measurements that can run on every commit: token counting and
//! store I/O. They do not involve a model, so they are cheap, repeatable, and
//! the right place to catch a performance regression early.
//!
//! Agent-level suites (navigation, terminal output, full coding tasks) need a
//! model in the loop and live in a separate harness.

use anyhow::{Context, Result};
use clap::Parser;
use ctx_bench::harness::Runner;
use ctx_core::tokens::{TokenCounter, Tokenizer};
use ctx_store::Store;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ctx-bench", about = "Deterministic microbenchmarks", version)]
struct Cli {
    /// Measured iterations per benchmark.
    #[arg(long, default_value_t = 100)]
    iters: usize,

    /// Warmup iterations, run and discarded.
    #[arg(long, default_value_t = 5)]
    warmup: usize,

    /// Append JSONL results here. Defaults to stdout only.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Skip the 40 MB store cases, which dominate runtime.
    #[arg(long)]
    quick: bool,
}

/// Deterministic pseudo-text. Uses a fixed seed so the input is identical on
/// every run and across machines — a benchmark whose input varies cannot
/// detect a regression.
fn synthetic_text(bytes: usize) -> String {
    const WORDS: [&str; 16] = [
        "fn", "let", "match", "impl", "token", "validate", "refresh", "expired",
        "Result", "Error", "async", "await", "struct", "enum", "trait", "where",
    ];
    let mut s = String::with_capacity(bytes + 16);
    let mut state: u64 = 0x9E3779B97F4A7C15;
    while s.len() < bytes {
        // xorshift64: deterministic, no dependency.
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        s.push_str(WORDS[(state % 16) as usize]);
        s.push(if state % 8 == 0 { '\n' } else { ' ' });
    }
    s.truncate(bytes);
    s
}

fn synthetic_bytes(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut runner = Runner::new();

    eprintln!(
        "ctx-bench: {} iters, {} warmup, commit {}",
        cli.iters,
        cli.warmup,
        runner
            .environment()
            .git_commit
            .clone()
            .unwrap_or_else(|| "unknown".into())
    );
    if runner.environment().git_dirty == Some(true) {
        eprintln!("WARNING: working tree is dirty; results are not tied to a clean commit");
    }

    // --- Token counting -----------------------------------------------------
    // Hypothesis from iteration 1: BPE vocabulary loading dominates cold start.
    // This is the measurement that confirms or refutes it.
    runner.measure(
        "tokens.counter_construct",
        "construct a TokenCounter, loading the o200k_base vocabulary",
        0,
        // Construction is expensive; a large iteration count would dominate
        // the whole suite for no extra information.
        cli.iters.min(10),
        None,
        Some("cold value is the figure that matters for CLI startup"),
        || {
            std::hint::black_box(
                TokenCounter::new(Tokenizer::O200kBase).expect("vocabulary loads"),
            );
        },
    );

    let counter = TokenCounter::default_counter()?;
    for size in [1_024usize, 102_400, 1_048_576] {
        let text = synthetic_text(size);
        runner.measure(
            &format!("tokens.count_{}", human(size)),
            "count tokens in synthetic source-like text",
            cli.warmup,
            cli.iters,
            Some(size as u64),
            None,
            || {
                std::hint::black_box(counter.count(&text));
            },
        );
    }

    // --- Store I/O ----------------------------------------------------------
    let tmp = tempfile::tempdir().context("creating a scratch directory")?;
    let store = Store::open(tmp.path()).context("opening the scratch store")?;

    let mut sizes: Vec<usize> = vec![1_024, 1_048_576];
    if !cli.quick {
        sizes.push(40 * 1_048_576);
    }

    for size in sizes {
        let payload = synthetic_bytes(size);

        // put_run at a given size. Content addressing makes a repeat write of
        // identical bytes nearly free, which would flatter the number, so each
        // iteration writes distinct content by varying one leading byte.
        let mut counter_byte: u8 = 0;
        let mut buf = payload.clone();
        runner.measure(
            &format!("store.put_run_{}", human(size)),
            "capture a command run, writing distinct stdout to the blob store",
            cli.warmup.min(2),
            // Large writes are slow; scale iterations down so the suite stays
            // runnable on every commit.
            if size > 1_048_576 {
                cli.iters.min(5)
            } else {
                cli.iters.min(30)
            },
            Some(size as u64),
            Some("each iteration writes distinct bytes, so dedup does not flatter the result"),
            || {
                counter_byte = counter_byte.wrapping_add(1);
                buf[0] = counter_byte;
                std::hint::black_box(
                    store
                        .put_run("bench", "/tmp", Some(0), &buf, b"", 1)
                        .expect("store write succeeds"),
                );
            },
        );

        // Read-back latency for the same size.
        let handle = store
            .put_run("bench-read", "/tmp", Some(0), &payload, b"", 1)
            .context("seeding a run to read back")?;
        runner.measure(
            &format!("store.run_output_{}", human(size)),
            "recover full captured output by handle",
            cli.warmup.min(2),
            if size > 1_048_576 {
                cli.iters.min(10)
            } else {
                cli.iters.min(30)
            },
            Some(size as u64),
            None,
            || {
                let (out, _) = store.run_output(&handle).expect("handle resolves");
                std::hint::black_box(out);
            },
        );
    }

    // Snapshot lookup: the operation that decides whether a re-read can be
    // answered with "unchanged" instead of the whole file.
    let content = synthetic_bytes(64 * 1024);
    store.record_snapshot("src/bench.rs", &content)?;
    runner.measure(
        "store.last_seen_hash",
        "look up the last content hash shown for a path",
        cli.warmup,
        cli.iters,
        None,
        Some("gates the unchanged-file fast path; must stay far below a file read"),
        || {
            std::hint::black_box(
                store
                    .last_seen_hash("src/bench.rs")
                    .expect("snapshot query succeeds"),
            );
        },
    );

    // --- Report -------------------------------------------------------------
    let jsonl = runner.to_jsonl()?;
    if let Some(path) = &cli.out {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("appending results to {}", path.display()))?;
        f.write_all(jsonl.as_bytes())?;
        eprintln!("appended {} lines to {}", jsonl.lines().count(), path.display());
    }

    println!(
        "{:<34} {:>7} {:>11} {:>11} {:>12}",
        "benchmark", "n", "p50", "p95", "throughput"
    );
    for m in runner.measurements() {
        // Cold lines are n=1 and not comparable; mark them so a reader cannot
        // mistake one for a distribution.
        let tag = if m.stats.low_confidence { " *" } else { "" };
        let thr = m
            .throughput_mib_s
            .map(|t| format!("{t:>8.1} MiB/s"))
            .unwrap_or_else(|| "           -".to_string());
        println!(
            "{:<34} {:>7} {:>9.3}ms {:>9.3}ms {}{}",
            m.name,
            m.stats.n,
            m.stats.p50_ms(),
            m.stats.p95_ms(),
            thr,
            tag
        );
    }
    println!("\n* n<3: single observation, not a distribution");
    Ok(())
}

fn human(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{}mib", bytes / 1_048_576)
    } else {
        format!("{}kib", bytes / 1024)
    }
}
