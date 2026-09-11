//! Timing runner and reproducibility record.
//!
//! Every emitted line carries its own [`Environment`], so an orphan number
//! with no provenance cannot be produced by construction. Fields that cannot
//! be determined are `null` rather than filled with a plausible guess.

use crate::stats::Stats;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Whether a measurement reflects a first-touch or a steady state.
///
/// Recorded explicitly because quietly excluding cold-start cost is one of the
/// easier ways to make an optimisation look better than it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Thermal {
    /// First execution in this process, caches unprimed. Always `n = 1`.
    Cold,
    /// Steady state after warmup iterations were run and discarded.
    Warm,
}

/// The reproducibility record attached to every measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub git_commit: Option<String>,
    pub git_dirty: Option<bool>,
    pub runtime_version: String,
    pub os: String,
    pub arch: String,
    pub cpu_count: Option<usize>,
    /// CPU model string, when the platform will tell us. `null` if unknown —
    /// never guessed.
    pub cpu_model: Option<String>,
    pub rustc_version: Option<String>,
    /// Seconds since the Unix epoch.
    pub timestamp: u64,
}

impl Environment {
    pub fn detect() -> Self {
        Self {
            git_commit: git(&["rev-parse", "--short", "HEAD"]),
            git_dirty: git(&["status", "--porcelain"]).map(|s| !s.trim().is_empty()),
            runtime_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cpu_count: std::thread::available_parallelism().ok().map(|n| n.get()),
            cpu_model: cpu_model(),
            rustc_version: command_output("rustc", &["--version"]),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }
}

fn git(args: &[&str]) -> Option<String> {
    command_output("git", args)
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Best-effort CPU model. Returns `None` rather than a guess.
fn cpu_model() -> Option<String> {
    if cfg!(target_os = "windows") {
        std::env::var("PROCESSOR_IDENTIFIER").ok().filter(|s| !s.is_empty())
    } else if cfg!(target_os = "linux") {
        let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        text.lines()
            .find(|l| l.starts_with("model name"))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
    } else {
        command_output("sysctl", &["-n", "machdep.cpu.brand_string"])
    }
}

/// One benchmark result, self-describing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Measurement {
    /// Stable identifier, e.g. `store.put_run`.
    pub name: String,
    /// What one iteration did, in words. Prevents a bare number from being
    /// reinterpreted later as measuring something else.
    pub operation: String,
    pub thermal: Thermal,
    pub stats: Stats,
    /// Warmup iterations run and discarded before measuring.
    pub warmup: usize,
    /// Size of the input one iteration processed, when meaningful.
    pub input_bytes: Option<u64>,
    /// Derived from p50 and `input_bytes`. Reported only when both exist.
    pub throughput_mib_s: Option<f64>,
    /// Anything a reader needs in order not to misread the number.
    pub notes: Option<String>,
    pub environment: Environment,
}

impl Measurement {
    fn new(
        name: &str,
        operation: &str,
        thermal: Thermal,
        stats: Stats,
        warmup: usize,
        input_bytes: Option<u64>,
        notes: Option<String>,
        environment: Environment,
    ) -> Self {
        let throughput_mib_s = input_bytes.and_then(|b| {
            if stats.p50_ns == 0 {
                None
            } else {
                let secs = stats.p50_ns as f64 / 1e9;
                Some((b as f64 / (1024.0 * 1024.0)) / secs)
            }
        });
        Self {
            name: name.to_string(),
            operation: operation.to_string(),
            thermal,
            stats,
            warmup,
            input_bytes,
            throughput_mib_s,
            notes,
            environment,
        }
    }
}

/// Runs timed closures and collects self-describing measurements.
pub struct Runner {
    environment: Environment,
    measurements: Vec<Measurement>,
}

impl Runner {
    pub fn new() -> Self {
        Self {
            environment: Environment::detect(),
            measurements: Vec::new(),
        }
    }

    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    pub fn measurements(&self) -> &[Measurement] {
        &self.measurements
    }

    /// Time `f` and record both the cold first touch and the warm steady state.
    ///
    /// Returns `(cold, warm)`. The cold measurement is genuinely the first
    /// execution in this process and is reported with `n = 1`; it is never
    /// folded into the warm distribution, and the warm distribution never
    /// silently includes it.
    pub fn measure<F: FnMut()>(
        &mut self,
        name: &str,
        operation: &str,
        warmup: usize,
        iters: usize,
        input_bytes: Option<u64>,
        notes: Option<&str>,
        mut f: F,
    ) -> (Measurement, Measurement) {
        assert!(iters > 0, "iters must be positive");

        // Cold: the very first execution, before any warmup.
        let t0 = Instant::now();
        f();
        let cold_ns = t0.elapsed().as_nanos() as u64;

        for _ in 0..warmup {
            f();
        }

        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t = Instant::now();
            f();
            samples.push(t.elapsed().as_nanos() as u64);
        }

        let cold = Measurement::new(
            &format!("{name}.cold"),
            operation,
            Thermal::Cold,
            Stats::from_durations(vec![cold_ns]),
            0,
            input_bytes,
            Some(
                "first execution in a fresh process; n=1 by definition, \
                 not comparable to the warm distribution"
                    .to_string(),
            ),
            self.environment.clone(),
        );
        let warm = Measurement::new(
            name,
            operation,
            Thermal::Warm,
            Stats::from_durations(samples),
            warmup,
            input_bytes,
            notes.map(|s| s.to_string()),
            self.environment.clone(),
        );

        self.measurements.push(cold.clone());
        self.measurements.push(warm.clone());
        (cold, warm)
    }

    /// Serialise every measurement as JSONL, one self-describing object per line.
    pub fn to_jsonl(&self) -> anyhow::Result<String> {
        let mut out = String::new();
        for m in &self.measurements {
            out.push_str(&serde_json::to_string(m)?);
            out.push('\n');
        }
        Ok(out)
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_reports_unknowns_as_null_not_guesses() {
        let e = Environment::detect();
        assert_eq!(e.os, std::env::consts::OS);
        assert!(!e.runtime_version.is_empty());
        // cpu_model may legitimately be None; what matters is that it is either
        // a real string or absent, never a placeholder.
        if let Some(m) = &e.cpu_model {
            assert!(!m.is_empty());
            assert!(!m.contains("unknown"));
        }
    }

    #[test]
    fn measure_separates_cold_from_warm() {
        let mut r = Runner::new();
        let mut n = 0u64;
        let (cold, warm) = r.measure(
            "test.noop",
            "increment a counter",
            2,
            20,
            None,
            None,
            || {
                n += 1;
            },
        );
        assert_eq!(cold.thermal, Thermal::Cold);
        assert_eq!(cold.stats.n, 1);
        assert!(cold.stats.low_confidence);
        assert_eq!(warm.thermal, Thermal::Warm);
        assert_eq!(warm.stats.n, 20);
        assert_eq!(warm.warmup, 2);
        // 1 cold + 2 warmup + 20 measured
        assert_eq!(n, 23);
    }

    #[test]
    fn throughput_is_only_reported_when_input_size_is_known() {
        let mut r = Runner::new();
        let (_, with_size) = r.measure(
            "test.sized",
            "touch a buffer",
            0,
            5,
            Some(1024),
            None,
            || {
                std::hint::black_box(vec![0u8; 64]);
            },
        );
        assert!(with_size.throughput_mib_s.is_some());

        let (_, without) =
            r.measure("test.unsized", "touch a buffer", 0, 5, None, None, || {
                std::hint::black_box(vec![0u8; 64]);
            });
        assert!(
            without.throughput_mib_s.is_none(),
            "throughput must not be invented when input size is unknown"
        );
    }

    #[test]
    fn jsonl_lines_each_carry_full_provenance() {
        let mut r = Runner::new();
        r.measure("test.x", "noop", 0, 3, None, None, || {});
        let jsonl = r.to_jsonl().unwrap();
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2, "one cold and one warm line");
        for line in lines {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            // Each line must stand alone as evidence.
            assert!(v["environment"]["os"].is_string());
            assert!(v["environment"]["timestamp"].is_number());
            assert!(v["operation"].is_string());
            assert!(v["stats"]["p50_ns"].is_number());
            assert!(v["stats"]["p95_ns"].is_number());
        }
    }

    #[test]
    #[should_panic(expected = "iters must be positive")]
    fn zero_iterations_is_rejected() {
        let mut r = Runner::new();
        r.measure("test.bad", "noop", 0, 0, None, None, || {});
    }
}
