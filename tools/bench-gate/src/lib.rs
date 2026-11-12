//! The comparison behind `bench-gate`: which bench cases run, how a run's JSON line becomes
//! [`Metrics`], and when a result counts as a slowdown against the baseline.
//!
//! Rules: each case runs [`RUNS`] times and the best value of each metric is
//! kept, so a slowdown fails only when every run was slow (noise rarely does that). Median
//! metrics (GPU frame, wall p50) may grow by at most [`Limits::median`]; the wall p99 by at
//! most [`Limits::tail`]. On tiled GPUs only the whole GPU frame is valid, so
//! per-pass times are not compared.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Runs per case.
pub const RUNS: usize = 3;

/// One bench case: a stable name and the bench arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Case {
    /// The key in the baseline file.
    pub name: &'static str,
    /// Arguments passed to the render bench.
    pub args: &'static [&'static str],
}

/// The cases every check runs: the block grid at two sizes, and the large one without TAA.
pub const CASES: &[Case] = &[
    Case {
        name: "grid40-taa",
        args: &["--grid", "40", "--taa", "on", "--frames", "480"],
    },
    Case {
        name: "grid40-textured",
        args: &[
            "--grid",
            "40",
            "--taa",
            "on",
            "--textured",
            "on",
            "--frames",
            "480",
        ],
    },
    Case {
        name: "grid80-taa",
        args: &["--grid", "80", "--taa", "on", "--frames", "480"],
    },
    Case {
        name: "grid80-no-taa",
        args: &["--grid", "80", "--taa", "off", "--frames", "480"],
    },
];

/// What one run measured, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// GPU time from the first pass start to the last pass end (median over frames).
    pub gpu_frame_ms: f64,
    /// Wall frame time, median.
    pub wall_p50_ms: f64,
    /// Wall frame time, 99th percentile.
    pub wall_p99_ms: f64,
}

impl Metrics {
    /// Reads the bench's JSON line.
    ///
    /// # Errors
    /// [`GateError::BadBenchOutput`] when the line is not JSON or lacks a field.
    pub fn from_bench_line(line: &str) -> Result<Self, GateError> {
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| GateError::BadBenchOutput(error.to_string()))?;
        let field = |pointer: &str| {
            value
                .pointer(pointer)
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| GateError::BadBenchOutput(format!("missing {pointer}")))
        };
        Ok(Self {
            gpu_frame_ms: field("/gpu_ms/frame")?,
            wall_p50_ms: field("/wall_ms_p50")?,
            wall_p99_ms: field("/wall_ms_p99")?,
        })
    }

    /// The best (lowest) value of each metric over `runs`; `None` for no runs.
    #[must_use]
    pub fn best(runs: &[Self]) -> Option<Self> {
        let (first, rest) = runs.split_first()?;
        Some(rest.iter().fold(*first, |best, run| Self {
            gpu_frame_ms: best.gpu_frame_ms.min(run.gpu_frame_ms),
            wall_p50_ms: best.wall_p50_ms.min(run.wall_p50_ms),
            wall_p99_ms: best.wall_p99_ms.min(run.wall_p99_ms),
        }))
    }
}

/// The baseline file: one device, metrics per case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    /// The device the numbers come from.
    pub device: String,
    /// Why the baseline last changed (the commit message says it too).
    pub note: String,
    /// Metrics by case name.
    pub cases: BTreeMap<String, Metrics>,
}

/// How much each kind of metric may grow, as a fraction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// GPU frame and wall p50.
    pub median: f64,
    /// Wall p99.
    pub tail: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            median: 0.05,
            tail: 0.10,
        }
    }
}

/// One metric of one case that grew past its limit.
#[derive(Debug, Clone, PartialEq)]
pub struct Slowdown {
    /// The case.
    pub case: String,
    /// The metric's name.
    pub metric: &'static str,
    /// The baseline value.
    pub baseline_ms: f64,
    /// The new value.
    pub current_ms: f64,
}

/// Compares fresh results with the baseline. A case missing from the baseline is an error, so
/// adding a case forces recording a baseline for it.
///
/// # Errors
/// [`GateError::MissingCase`].
pub fn compare(
    baseline: &Baseline,
    current: &BTreeMap<String, Metrics>,
    limits: Limits,
) -> Result<Vec<Slowdown>, GateError> {
    let mut slow = Vec::new();
    for (case, now) in current {
        let base = baseline
            .cases
            .get(case)
            .ok_or_else(|| GateError::MissingCase(case.clone()))?;
        let checks = [
            (
                "gpu frame",
                base.gpu_frame_ms,
                now.gpu_frame_ms,
                limits.median,
            ),
            ("wall p50", base.wall_p50_ms, now.wall_p50_ms, limits.median),
            ("wall p99", base.wall_p99_ms, now.wall_p99_ms, limits.tail),
        ];
        for (metric, baseline_ms, current_ms, limit) in checks {
            if current_ms > baseline_ms * (1.0 + limit) {
                slow.push(Slowdown {
                    case: case.clone(),
                    metric,
                    baseline_ms,
                    current_ms,
                });
            }
        }
    }
    Ok(slow)
}

/// Why the gate could not decide.
#[derive(Debug, Error)]
pub enum GateError {
    /// The bench printed something unexpected.
    #[error("bench output: {0}")]
    BadBenchOutput(String),
    /// The baseline has no numbers for a case; record one.
    #[error("the baseline has no case {0}; run with --record")]
    MissingCase(String),
    /// Reading or writing a file, or running the bench, failed.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The baseline file is not valid JSON.
    #[error("baseline: {0}")]
    Baseline(#[from] serde_json::Error),
    /// The bench exited with an error.
    #[error("bench failed: {0}")]
    BenchFailed(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn metrics(gpu: f64, p50: f64, p99: f64) -> Metrics {
        Metrics {
            gpu_frame_ms: gpu,
            wall_p50_ms: p50,
            wall_p99_ms: p99,
        }
    }

    fn baseline(m: Metrics) -> Baseline {
        Baseline {
            device: "test".to_owned(),
            note: String::new(),
            cases: BTreeMap::from([("a".to_owned(), m)]),
        }
    }

    #[test]
    fn reads_the_bench_json_line() {
        let line = r#"{"size":"1920x1080","wall_ms_p50":4.0,"wall_ms_p99":4.3,"gpu_ms":{"frame":2.887,"taa":1.8}}"#;
        assert_eq!(
            Metrics::from_bench_line(line).unwrap(),
            metrics(2.887, 4.0, 4.3)
        );
        assert!(Metrics::from_bench_line(r#"{"wall_ms_p50":4.0}"#).is_err());
        assert!(Metrics::from_bench_line("bench: no adapter").is_err());
    }

    #[test]
    fn the_best_of_each_metric_is_kept_independently() {
        let runs = [
            metrics(3.0, 4.0, 9.0),
            metrics(2.0, 5.0, 6.0),
            metrics(4.0, 3.5, 7.0),
        ];
        assert_eq!(Metrics::best(&runs), Some(metrics(2.0, 3.5, 6.0)));
        assert_eq!(Metrics::best(&[]), None);
    }

    #[test]
    fn limits_differ_for_medians_and_the_tail() {
        let base = baseline(metrics(2.0, 4.0, 5.0));
        let within = BTreeMap::from([("a".to_owned(), metrics(2.09, 4.19, 5.49))]);
        assert!(
            compare(&base, &within, Limits::default())
                .unwrap()
                .is_empty()
        );
        let over = BTreeMap::from([("a".to_owned(), metrics(2.11, 4.0, 5.51))]);
        let slow = compare(&base, &over, Limits::default()).unwrap();
        let names: Vec<_> = slow.iter().map(|s| s.metric).collect();
        assert_eq!(names, vec!["gpu frame", "wall p99"]);
    }

    #[test]
    fn a_case_without_a_baseline_is_an_error() {
        let base = baseline(metrics(1.0, 1.0, 1.0));
        let current = BTreeMap::from([("b".to_owned(), metrics(1.0, 1.0, 1.0))]);
        assert!(matches!(
            compare(&base, &current, Limits::default()),
            Err(GateError::MissingCase(case)) if case == "b"
        ));
    }

    proptest! {
        #[test]
        fn equal_or_faster_results_never_fail(
            gpu in 0.1_f64..50.0, p50 in 0.1_f64..50.0, p99 in 0.1_f64..50.0, faster in 0.0_f64..1.0,
        ) {
            let base = baseline(metrics(gpu, p50, p99));
            let now = BTreeMap::from([("a".to_owned(), metrics(gpu * faster, p50 * faster, p99))]);
            prop_assert!(compare(&base, &now, Limits::default()).unwrap().is_empty());
        }
    }
}
