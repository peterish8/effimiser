//! Symbol-lookup benchmark: vanilla ripgrep baseline vs the symbol index.
//!
//! The question Phase 3 has to answer before anything is built on top of it:
//! **is an index actually better than ripgrep at "where is X defined"?**
//! If it is not, the index is not worth its complexity, and that is a finding
//! worth having now rather than after phases 4-11 depend on it.
//!
//! # Baseline fairness
//!
//! The anti-gaming rules forbid comparing against a deliberately weak baseline,
//! so the baseline here is not `rg <name>`. It is the definition-shaped pattern
//! a competent agent would actually write, anchored to a line start and
//! covering the Rust definition keywords with their usual modifiers. Process
//! spawn cost is included, because an agent really pays it.
//!
//! # Metrics, fixed before any number was seen
//!
//! Latency alone would be the wrong measure. An index that is marginally
//! faster but returns the same wall of matches saves nothing that matters,
//! because the expensive resource is the model's context, not the CPU. So each
//! lookup records:
//!
//! - wall-clock latency, including process spawn for the baseline;
//! - how many matches came back;
//! - bytes of output the agent would have to consume;
//! - **tokens** of that output, counted with the real BPE, not estimated;
//! - whether the true definition site is present in the result.
//!
//! Index build time is measured and reported separately rather than excluded.
//! Hiding it would be gaming: indexing is part of the user's experience.

use anyhow::{Context, Result};
use clap::Parser;
use ctx_bench::harness::{Environment, Runner};
use ctx_bench::stats::Stats;
use ctx_core::tokens::TokenCounter;
use ctx_index::Index;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "symbols", about = "Symbol lookup: ripgrep baseline vs index")]
struct Cli {
    /// Symbols sampled per repository.
    #[arg(long, default_value_t = 40)]
    symbols: usize,

    /// Measured repetitions per symbol.
    #[arg(long, default_value_t = 3)]
    reps: usize,

    /// Append JSONL results here.
    #[arg(long)]
    out: Option<PathBuf>,
}

/// A repository under test, with the tier it represents.
struct Corpus {
    tier: &'static str,
    name: &'static str,
    path: PathBuf,
}

/// One lookup's outcome. Deliberately records the context cost, not just time.
#[derive(Debug, Clone, Serialize)]
struct LookupOutcome {
    symbol: String,
    latency_ns: u64,
    matches: usize,
    output_bytes: usize,
    output_tokens: usize,
}

/// Per-arm sample collection, so both arms accumulate identically.
#[derive(Default)]
struct ArmSamples {
    latency: Vec<u64>,
    matches: Vec<usize>,
    bytes: Vec<usize>,
    tokens: Vec<usize>,
}

impl ArmSamples {
    fn push(&mut self, o: LookupOutcome) {
        self.latency.push(o.latency_ns);
        self.matches.push(o.matches);
        self.bytes.push(o.output_bytes);
        self.tokens.push(o.output_tokens);
    }
}

/// Index construction cost. Reported rather than excluded: indexing is part of
/// the user's experience, and hiding it would be gaming the comparison.
#[derive(Debug, Clone, Serialize)]
struct IndexBuild {
    name: String,
    tier: String,
    repo: String,
    files: usize,
    files_parsed_cold: usize,
    symbols: usize,
    build_ns: u64,
    noop_reindex_ns: u64,
    /// Target 5 in benchmark-plan.md: this must be 0.
    noop_files_reparsed: usize,
    db_bytes: u64,
    environment: Environment,
}

/// Aggregated arm result for one corpus.
#[derive(Debug, Clone, Serialize)]
struct ArmResult {
    name: String,
    arm: String,
    tier: String,
    repo: String,
    repo_files: usize,
    repo_lines: usize,
    symbols_measured: usize,
    reps_per_symbol: usize,
    latency: Stats,
    /// Distribution of how much output the agent would have to read.
    matches_p50: usize,
    matches_p95: usize,
    output_bytes_p50: usize,
    output_tokens_p50: usize,
    output_tokens_p95: usize,
    output_tokens_total: usize,
    tokenizer: String,
    /// Measured cost of spawning ripgrep with no search at all. Recorded as a
    /// number, not just prose, because it is what separates "our data
    /// structure is faster" from "we are already running and ripgrep is not".
    spawn_floor_p50_ns: Option<u64>,
    notes: String,
    environment: Environment,
}

/// The definition-shaped pattern a competent agent would write for Rust.
/// Kept in one place so the baseline cannot be quietly weakened later.
fn definition_pattern(symbol: &str) -> String {
    format!(
        r"^\s*(pub\s*(\([^)]*\)\s*)?)?(default\s+)?(async\s+)?(unsafe\s+)?(extern\s+\S+\s+)?(fn|struct|enum|trait|union|type|const|static|mod|macro_rules!)\s+{}\b",
        regex_escape(symbol)
    )
}

fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Every Rust file in the corpus, excluding build output and VCS internals.
fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !matches!(name.as_ref(), "target" | ".git" | "node_modules" | "vendor") {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Deterministically sample symbol names defined in the corpus.
///
/// Uses a plain scan rather than the index under test, so the sample cannot be
/// biased toward whatever the index happens to be good at. Sorted and evenly
/// strided, so the same corpus always yields the same symbols.
fn sample_symbols(files: &[PathBuf], want: usize) -> Vec<String> {
    const KEYWORDS: [&str; 8] = [
        "fn ", "struct ", "enum ", "trait ", "union ", "type ", "const ", "static ",
    ];
    let mut names: BTreeSet<String> = BTreeSet::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        for line in text.lines() {
            let t = line.trim_start();
            // Skip obvious non-definitions: comments and calls.
            if t.starts_with("//") || t.starts_with('*') {
                continue;
            }
            let t = t.strip_prefix("pub ").unwrap_or(t);
            let t = t.split_once(") ").map(|(_, r)| r).unwrap_or(t);
            for kw in KEYWORDS {
                if let Some(rest) = t.strip_prefix(kw) {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    // Very short names are ambiguous and would flatter whichever
                    // arm returns fewer results; skip them.
                    if name.len() >= 4 && !name.chars().next().is_some_and(|c| c.is_numeric()) {
                        names.insert(name);
                    }
                }
            }
        }
    }
    let all: Vec<String> = names.into_iter().collect();
    if all.len() <= want {
        return all;
    }
    let stride = all.len() / want;
    all.into_iter().step_by(stride.max(1)).take(want).collect()
}

/// Baseline arm: spawn ripgrep exactly as an agent would.
fn rg_lookup(root: &Path, symbol: &str, counter: &TokenCounter) -> Result<LookupOutcome> {
    let pattern = definition_pattern(symbol);
    let t = Instant::now();
    let out = Command::new("rg")
        .args(["--line-number", "--no-heading", "--type", "rust", &pattern])
        .arg(root)
        .output()
        .context("spawning ripgrep")?;
    let latency_ns = t.elapsed().as_nanos() as u64;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let matches = if stdout.trim().is_empty() {
        0
    } else {
        stdout.lines().count()
    };
    Ok(LookupOutcome {
        symbol: symbol.to_string(),
        latency_ns,
        matches,
        output_bytes: out.stdout.len(),
        output_tokens: counter.count(&stdout).tokens,
    })
}

/// Spawn-floor probe: ripgrep doing no search at all.
///
/// Measured because the baseline's latency barely moved between a 3k-line and
/// a 264k-line repository, which can only be true if a fixed cost dominates.
/// Attributing that cost matters: an index that "wins" purely by not paying
/// process spawn has a deployment-model advantage, not a data-structure one,
/// and saying otherwise would overstate the result.
fn rg_spawn_floor() -> Result<u64> {
    let t = Instant::now();
    Command::new("rg")
        .arg("--version")
        .output()
        .context("spawning ripgrep")?;
    Ok(t.elapsed().as_nanos() as u64)
}

/// Treatment arm: an in-process index lookup, rendered as the agent would see it.
///
/// The rendering matters. Comparing a raw struct against ripgrep's formatted
/// text would understate the baseline, so the index result is formatted into
/// the same `path:line: kind name` shape a tool would actually return, and
/// *that* is what gets counted.
fn index_lookup(idx: &Index, symbol: &str, counter: &TokenCounter) -> Result<LookupOutcome> {
    let t = Instant::now();
    let defs = idx.find_definition(symbol)?;
    let latency_ns = t.elapsed().as_nanos() as u64;

    let mut rendered = String::new();
    for d in &defs {
        match &d.container {
            Some(c) => rendered.push_str(&format!(
                "{}:{}: {} {}::{}
",
                d.path,
                d.line,
                d.kind.as_str(),
                c,
                d.name
            )),
            None => rendered.push_str(&format!(
                "{}:{}: {} {}
",
                d.path,
                d.line,
                d.kind.as_str(),
                d.name
            )),
        }
    }
    Ok(LookupOutcome {
        symbol: symbol.to_string(),
        latency_ns,
        matches: defs.len(),
        output_bytes: rendered.len(),
        output_tokens: counter.count(&rendered).tokens,
    })
}

/// Definition sites ripgrep reports, as (relative path, line).
fn rg_sites(root: &Path, symbol: &str) -> Result<BTreeSet<(String, usize)>> {
    let pattern = definition_pattern(symbol);
    let out = Command::new("rg")
        .args(["--line-number", "--no-heading", "--type", "rust", &pattern])
        .arg(root)
        .output()
        .context("spawning ripgrep")?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut sites = BTreeSet::new();
    for line in text.lines() {
        // Windows paths carry a drive letter, so split from the right on the
        // two trailing ':' separated fields rather than the first ':'.
        let mut parts = line.splitn(3, ':').collect::<Vec<_>>();
        if parts.len() < 3 {
            continue;
        }
        // Re-join a leading drive letter if the split ate it.
        if parts[0].len() == 1 && parts[1].starts_with('\\') {
            let joined = format!("{}:{}", parts[0], parts[1]);
            let rest: Vec<&str> = parts[2].splitn(2, ':').collect();
            if rest.len() < 2 {
                continue;
            }
            parts = vec![Box::leak(joined.into_boxed_str()), rest[0], rest[1]];
        }
        let Ok(line_no) = parts[1].parse::<usize>() else {
            continue;
        };
        let rel = Path::new(parts[0])
            .strip_prefix(root)
            .unwrap_or(Path::new(parts[0]))
            .to_string_lossy()
            .replace('\\', "/");
        sites.insert((rel, line_no));
    }
    Ok(sites)
}

/// How often the index's answer matches ripgrep's, and how often it does not.
///
/// A lookup that is four thousand times faster but points at the wrong line is
/// worthless, so agreement is measured rather than assumed. Disagreement is not
/// automatically an index bug — ripgrep's regex misses definitions it was never
/// written to match, and tree-sitter finds definitions a line-oriented pattern
/// cannot see — so both directions are counted separately.
#[derive(Debug, Clone, Serialize)]
struct Agreement {
    name: String,
    tier: String,
    repo: String,
    symbols: usize,
    /// Both arms returned exactly the same set of sites.
    identical: usize,
    /// Index found every site ripgrep did, plus at least one more.
    index_superset: usize,
    /// Ripgrep found a site the index missed. These are the ones that matter.
    index_missed_a_site: usize,
    /// Neither arm found anything.
    both_empty: usize,
    /// Example disagreements, kept so a reader can check rather than trust.
    examples: Vec<String>,
    environment: Environment,
}

/// On-disk size of a SQLite database including its WAL sidecars.
fn db_total_bytes(db: &Path) -> u64 {
    let mut total = std::fs::metadata(db).map(|m| m.len()).unwrap_or(0);
    for suffix in ["-wal", "-shm"] {
        let mut p = db.as_os_str().to_os_string();
        p.push(suffix);
        total += std::fs::metadata(PathBuf::from(p))
            .map(|m| m.len())
            .unwrap_or(0);
    }
    total
}

fn percentile(sorted: &[usize], p: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let runner = Runner::new();
    let counter = TokenCounter::default_counter()?;

    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .context("locating the home directory")?;
    let aclones = PathBuf::from(&home).join("Desktop").join("aclones");

    let corpora = vec![
        Corpus {
            tier: "small",
            name: "Effimiser",
            path: PathBuf::from(&home).join("Desktop").join("Effimiser"),
        },
        Corpus {
            tier: "medium",
            name: "ast-grep",
            path: aclones.join("ast-grep"),
        },
        Corpus {
            tier: "large",
            name: "probe",
            path: aclones.join("probe"),
        },
    ];

    let mut results: Vec<ArmResult> = Vec::new();
    let mut index_builds: Vec<IndexBuild> = Vec::new();
    let mut agreements: Vec<Agreement> = Vec::new();
    let scratch = tempfile::tempdir().context("creating a scratch directory")?;

    for corpus in &corpora {
        if !corpus.path.exists() {
            eprintln!("skipping {}: {} not found", corpus.name, corpus.path.display());
            continue;
        }
        let files = rust_files(&corpus.path);
        let lines: usize = files
            .iter()
            .filter_map(|f| std::fs::read_to_string(f).ok())
            .map(|t| t.lines().count())
            .sum();
        let symbols = sample_symbols(&files, cli.symbols);
        eprintln!(
            "{} [{}]: {} files, {} lines, {} symbols sampled",
            corpus.name,
            corpus.tier,
            files.len(),
            lines,
            symbols.len()
        );

        // Build the index into a scratch directory: the corpora are other
        // people's repositories and must not be written into.
        let db = scratch.path().join(format!("{}.sqlite", corpus.name));
        let mut idx = Index::open_at(&db, &corpus.path)?;
        let t = Instant::now();
        let build = idx.index_all()?;
        let build_ns = t.elapsed().as_nanos() as u64;

        // Reindex with nothing changed: the incrementality claim, measured on a
        // real repository rather than a fixture.
        let t = Instant::now();
        let rebuild = idx.index_all()?;
        let rebuild_ns = t.elapsed().as_nanos() as u64;

        eprintln!(
            "  index: {} symbols from {} files in {:.0} ms; no-op reindex {:.0} ms ({} reparsed)",
            build.symbols_written,
            build.files_parsed,
            build_ns as f64 / 1e6,
            rebuild_ns as f64 / 1e6,
            rebuild.files_parsed
        );
        index_builds.push(IndexBuild {
            name: format!("symbols.index_build.{}", corpus.tier),
            tier: corpus.tier.into(),
            repo: corpus.name.into(),
            files: build.files_seen,
            files_parsed_cold: build.files_parsed,
            symbols: build.symbols_written,
            build_ns,
            noop_reindex_ns: rebuild_ns,
            noop_files_reparsed: rebuild.files_parsed,
            // Sum the WAL sidecars too. Measuring only the main file reported
            // 4 KiB for a 7,320-symbol index, because in WAL mode the pages
            // live in `-wal` until a checkpoint. An on-disk-size claim that
            // undercounts by three orders of magnitude is worse than none.
            db_bytes: db_total_bytes(&db),
            environment: runner.environment().clone(),
        });

        // Warm both arms so we measure steady state, not first touch.
        if let Some(first) = symbols.first() {
            let _ = rg_lookup(&corpus.path, first, &counter)?;
            let _ = index_lookup(&idx, first, &counter)?;
        }

        let mut base = ArmSamples::default();
        let mut treat = ArmSamples::default();
        let mut floor: Vec<u64> = Vec::new();

        // Interleaved, order swapped each round. Iteration 3 showed that
        // running one arm to completion then the other lets drift land on a
        // single arm and manufacture a speedup.
        for (i, symbol) in symbols.iter().enumerate() {
            for rep in 0..cli.reps {
                if (i + rep) % 2 == 0 {
                    base.push(rg_lookup(&corpus.path, symbol, &counter)?);
                    treat.push(index_lookup(&idx, symbol, &counter)?);
                } else {
                    treat.push(index_lookup(&idx, symbol, &counter)?);
                    base.push(rg_lookup(&corpus.path, symbol, &counter)?);
                }
            }
            if i % 5 == 0 {
                floor.push(rg_spawn_floor()?);
            }
        }

        let spawn_floor = Stats::from_durations(floor);
        eprintln!(
            "  ripgrep spawn floor (no search): p50 {:.1} ms",
            spawn_floor.p50_ns as f64 / 1e6
        );

        // Accuracy pass, deliberately outside the timed loop so it cannot
        // perturb the latency numbers.
        let mut agree = Agreement {
            name: format!("symbols.agreement.{}", corpus.tier),
            tier: corpus.tier.into(),
            repo: corpus.name.into(),
            symbols: symbols.len(),
            identical: 0,
            index_superset: 0,
            index_missed_a_site: 0,
            both_empty: 0,
            examples: Vec::new(),
            environment: runner.environment().clone(),
        };
        for symbol in &symbols {
            let rg_set = rg_sites(&corpus.path, symbol)?;
            let idx_set: BTreeSet<(String, usize)> = idx
                .find_definition(symbol)?
                .into_iter()
                .map(|d| (d.path, d.line))
                .collect();
            if rg_set.is_empty() && idx_set.is_empty() {
                agree.both_empty += 1;
            } else if rg_set == idx_set {
                agree.identical += 1;
            } else if rg_set.is_subset(&idx_set) {
                agree.index_superset += 1;
                if agree.examples.len() < 5 {
                    let extra: Vec<String> = idx_set
                        .difference(&rg_set)
                        .map(|(p, l)| format!("{p}:{l}"))
                        .collect();
                    agree.examples
                        .push(format!("{symbol}: index also found {}", extra.join(", ")));
                }
            } else {
                agree.index_missed_a_site += 1;
                if agree.examples.len() < 5 {
                    let missed: Vec<String> = rg_set
                        .difference(&idx_set)
                        .map(|(p, l)| format!("{p}:{l}"))
                        .collect();
                    agree.examples
                        .push(format!("{symbol}: MISSED {}", missed.join(", ")));
                }
            }
        }
        eprintln!(
            "  agreement: {} identical, {} index-superset, {} index-missed, {} both-empty",
            agree.identical, agree.index_superset, agree.index_missed_a_site, agree.both_empty
        );
        for e in &agree.examples {
            eprintln!("      {e}");
        }
        agreements.push(agree);

        for (arm, mut samples, notes) in [
            (
                "baseline_ripgrep",
                base,
                format!(
                    "vanilla arm: ripgrep with a definition-shaped pattern, process spawn \
                     included because an agent pays it. Measured spawn floor on this machine \
                     (rg --version, no search): p50 {:.1} ms, most of this arm's latency.",
                    spawn_floor.p50_ns as f64 / 1e6
                ),
            ),
            (
                "treatment_index",
                treat,
                "in-process index lookup, result rendered as 'path:line: kind name' so both \
                 arms are counted on comparable output. Excludes index build, which is \
                 reported separately as symbols.index_build.*"
                    .to_string(),
            ),
        ] {
            let tokens_total: usize = samples.tokens.iter().sum::<usize>() / cli.reps.max(1);
            samples.matches.sort_unstable();
            samples.bytes.sort_unstable();
            let mut sorted_tokens = samples.tokens.clone();
            sorted_tokens.sort_unstable();
            results.push(ArmResult {
                name: format!("symbols.lookup.{}", corpus.tier),
                arm: arm.into(),
                tier: corpus.tier.into(),
                repo: corpus.name.into(),
                repo_files: files.len(),
                repo_lines: lines,
                symbols_measured: symbols.len(),
                reps_per_symbol: cli.reps,
                latency: Stats::from_durations(samples.latency),
                matches_p50: percentile(&samples.matches, 0.50),
                matches_p95: percentile(&samples.matches, 0.95),
                output_bytes_p50: percentile(&samples.bytes, 0.50),
                output_tokens_p50: percentile(&sorted_tokens, 0.50),
                output_tokens_p95: percentile(&sorted_tokens, 0.95),
                output_tokens_total: tokens_total,
                tokenizer: counter.tokenizer().as_str().into(),
                spawn_floor_p50_ns: Some(spawn_floor.p50_ns),
                notes,
                environment: runner.environment().clone(),
            });
        }
    }

    // Report.
    println!(
        "\n{:<8} {:<10} {:>18} {:>10} {:>10} {:>9} {:>9} {:>8}",
        "tier", "repo", "arm", "p50 ms", "p95 ms", "tok p50", "tok p95", "match"
    );
    println!("{}", "-".repeat(92));
    for r in &results {
        println!(
            "{:<8} {:<10} {:>18} {:>10.2} {:>10.2} {:>9} {:>9} {:>8}",
            r.tier,
            r.repo,
            r.arm,
            r.latency.p50_ns as f64 / 1e6,
            r.latency.p95_ns as f64 / 1e6,
            r.output_tokens_p50,
            r.output_tokens_p95,
            r.matches_p50
        );
    }

    println!("\nspeedup and token ratio, per tier:");
    for tier in ["small", "medium", "large"] {
        let b = results
            .iter()
            .find(|r| r.tier == tier && r.arm == "baseline_ripgrep");
        let t = results
            .iter()
            .find(|r| r.tier == tier && r.arm == "treatment_index");
        if let (Some(b), Some(t)) = (b, t) {
            let speedup = b.latency.p50_ns as f64 / t.latency.p50_ns.max(1) as f64;
            let tok = if t.output_tokens_p50 == 0 {
                f64::NAN
            } else {
                b.output_tokens_p50 as f64 / t.output_tokens_p50 as f64
            };
            println!(
                "  {:<7} latency x{:>7.1}   tokens x{:>5.2}   ({} -> {} tok p50)",
                tier, speedup, tok, b.output_tokens_p50, t.output_tokens_p50
            );
        }
    }

    println!("\nindex build cost (not excluded):");
    println!(
        "  {:<7} {:>7} {:>8} {:>11} {:>14} {:>12}",
        "tier", "files", "symbols", "build ms", "no-op re-idx", "db KiB"
    );
    for b in &index_builds {
        println!(
            "  {:<7} {:>7} {:>8} {:>11.0} {:>11.1} ms {:>12}",
            b.tier,
            b.files,
            b.symbols,
            b.build_ns as f64 / 1e6,
            b.noop_reindex_ns as f64 / 1e6,
            b.db_bytes / 1024
        );
        assert_eq!(
            b.noop_files_reparsed, 0,
            "incrementality is broken for {}: a no-op reindex reparsed {} files",
            b.repo, b.noop_files_reparsed
        );
    }
    println!("  (no-op reindex reparsed 0 files everywhere: target 5 holds)");

    if let Some(path) = &cli.out {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let mut jsonl = String::new();
        for r in &results {
            jsonl.push_str(&serde_json::to_string(r)?);
            jsonl.push('\n');
        }
        for b in &index_builds {
            jsonl.push_str(&serde_json::to_string(b)?);
            jsonl.push('\n');
        }
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        f.write_all(jsonl.as_bytes())?;
        eprintln!("\nappended {} lines to {}", results.len(), path.display());
    }

    Ok(())
}
