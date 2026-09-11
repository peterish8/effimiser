//! Distribution summary and regression comparison.
//!
//! Two rules are enforced here rather than left to reviewer discipline:
//! a summary always carries p50 and p95 (not just `min`), and a comparison
//! returns an explicit [`Verdict`] so "it looks better" is never the output.

use serde::{Deserialize, Serialize};

/// Summary of a sample of durations, in nanoseconds.
///
/// `min` is present for completeness but is never the headline: reporting only
/// the fastest run is explicitly forbidden by the benchmark plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    /// Number of observations. A summary of fewer than 3 is marked low-confidence.
    pub n: usize,
    pub min_ns: u64,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub max_ns: u64,
    /// True when `n` is too small for the percentiles to mean much.
    pub low_confidence: bool,
}

impl Stats {
    /// Summarise a sample. Panics on an empty sample, because an empty
    /// benchmark silently reporting zeros is worse than a crash.
    pub fn from_durations(mut ns: Vec<u64>) -> Self {
        assert!(!ns.is_empty(), "cannot summarise an empty sample");
        ns.sort_unstable();
        Self {
            n: ns.len(),
            min_ns: ns[0],
            p50_ns: percentile(&ns, 50.0),
            p95_ns: percentile(&ns, 95.0),
            max_ns: *ns.last().expect("non-empty"),
            low_confidence: ns.len() < 3,
        }
    }

    pub fn p50_ms(&self) -> f64 {
        self.p50_ns as f64 / 1e6
    }

    pub fn p95_ms(&self) -> f64 {
        self.p95_ns as f64 / 1e6
    }
}

/// Nearest-rank percentile on a sorted slice.
fn percentile(sorted: &[u64], p: f64) -> u64 {
    debug_assert!(!sorted.is_empty());
    debug_assert!((0.0..=100.0).contains(&p));
    if sorted.len() == 1 {
        return sorted[0];
    }
    // Nearest-rank: index = ceil(p/100 * n) - 1, clamped.
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

/// The outcome of comparing a treatment against a baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Treatment is meaningfully faster.
    Improvement,
    /// Change is within noise.
    Neutral,
    /// Treatment is meaningfully slower. Blocks a merge.
    Regression,
}

/// A baseline-vs-treatment comparison on p50, with the noise threshold stated
/// rather than implied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    pub baseline: Stats,
    pub treatment: Stats,
    /// Treatment p50 as a ratio of baseline p50. 0.5 = twice as fast.
    pub p50_ratio: f64,
    /// Relative change either way that counts as noise, e.g. 0.10 for ±10%.
    pub noise_threshold: f64,
    pub verdict: Verdict,
}

/// Compare two samples on p50. `noise_threshold` is the relative change below
/// which the result is called neutral.
///
/// Deliberately conservative: a change must exceed the threshold to be called
/// an improvement, so noise cannot be reported as a win.
pub fn compare(baseline: Stats, treatment: Stats, noise_threshold: f64) -> Comparison {
    assert!(
        noise_threshold >= 0.0,
        "noise threshold must be non-negative"
    );
    // Guard against a zero baseline, which would make the ratio meaningless.
    let ratio = if baseline.p50_ns == 0 {
        if treatment.p50_ns == 0 {
            1.0
        } else {
            f64::INFINITY
        }
    } else {
        treatment.p50_ns as f64 / baseline.p50_ns as f64
    };

    let verdict = if ratio > 1.0 + noise_threshold {
        Verdict::Regression
    } else if ratio < 1.0 - noise_threshold {
        Verdict::Improvement
    } else {
        Verdict::Neutral
    };

    Comparison {
        baseline,
        treatment,
        p50_ratio: ratio,
        noise_threshold,
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats_of(v: &[u64]) -> Stats {
        Stats::from_durations(v.to_vec())
    }

    #[test]
    fn percentiles_are_ordered() {
        let s = stats_of(&[10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);
        assert_eq!(s.n, 10);
        assert_eq!(s.min_ns, 10);
        assert_eq!(s.max_ns, 100);
        assert!(s.min_ns <= s.p50_ns);
        assert!(s.p50_ns <= s.p95_ns);
        assert!(s.p95_ns <= s.max_ns);
    }

    #[test]
    fn p50_is_not_the_minimum_on_a_skewed_sample() {
        // One fast run among slow ones must not be able to represent the set.
        let mut v = vec![1u64];
        v.extend(std::iter::repeat(100).take(99));
        let s = stats_of(&v);
        assert_eq!(s.min_ns, 1);
        assert_eq!(s.p50_ns, 100, "p50 must reflect the bulk, not the outlier");
    }

    #[test]
    fn single_observation_is_flagged_low_confidence() {
        let s = stats_of(&[42]);
        assert!(s.low_confidence);
        assert_eq!(s.p50_ns, 42);
        assert_eq!(s.p95_ns, 42);
    }

    #[test]
    fn three_observations_are_not_flagged() {
        assert!(!stats_of(&[1, 2, 3]).low_confidence);
    }

    #[test]
    #[should_panic(expected = "empty sample")]
    fn empty_sample_panics_rather_than_reporting_zero() {
        Stats::from_durations(vec![]);
    }

    // --- The negative control: the harness must catch a planted regression ---

    #[test]
    fn planted_regression_is_reported_as_a_regression() {
        let baseline = stats_of(&[100; 50]);
        // Treatment is 3x slower. If the harness calls this anything other
        // than a regression, it cannot be trusted to validate a real win.
        let treatment = stats_of(&[300; 50]);
        let c = compare(baseline, treatment, 0.10);
        assert_eq!(c.verdict, Verdict::Regression);
        assert!((c.p50_ratio - 3.0).abs() < 1e-9);
    }

    #[test]
    fn genuine_improvement_is_reported_as_improvement() {
        let c = compare(stats_of(&[200; 50]), stats_of(&[80; 50]), 0.10);
        assert_eq!(c.verdict, Verdict::Improvement);
    }

    #[test]
    fn noise_is_not_reported_as_a_win() {
        // 3% faster with a 10% noise threshold is not a result.
        let c = compare(stats_of(&[100; 50]), stats_of(&[97; 50]), 0.10);
        assert_eq!(
            c.verdict,
            Verdict::Neutral,
            "a sub-threshold change must not be claimed as an improvement"
        );
    }

    #[test]
    fn threshold_boundary_stays_neutral() {
        // Exactly at the threshold is neutral, not an improvement.
        let c = compare(stats_of(&[100; 10]), stats_of(&[90; 10]), 0.10);
        assert_eq!(c.verdict, Verdict::Neutral);
    }

    #[test]
    fn zero_baseline_does_not_produce_a_bogus_ratio() {
        let c = compare(stats_of(&[0; 5]), stats_of(&[0; 5]), 0.10);
        assert_eq!(c.p50_ratio, 1.0);
        assert_eq!(c.verdict, Verdict::Neutral);

        let c2 = compare(stats_of(&[0; 5]), stats_of(&[10; 5]), 0.10);
        assert!(c2.p50_ratio.is_infinite());
        assert_eq!(c2.verdict, Verdict::Regression);
    }

    #[test]
    fn comparison_serialises_with_its_threshold() {
        let c = compare(stats_of(&[100; 5]), stats_of(&[300; 5]), 0.10);
        let json = serde_json::to_string(&c).unwrap();
        // The threshold must travel with the verdict, so a reader can tell
        // how strict the call was.
        assert!(json.contains("noise_threshold"));
        assert!(json.contains("regression"));
    }
}
