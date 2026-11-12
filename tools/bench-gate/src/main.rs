//! `bench-gate`: builds the render bench once, runs every case [`RUNS`] times, and compares
//! the best results with `bench/baseline/<device>.json`.
//!
//! `cargo run -p bench-gate -- --device apple-m5` checks (exit 1 on a slowdown);
//! `cargo run -p bench-gate -- --device apple-m5 --record "why"` writes the baseline. Run it
//! on an idle machine: other load moves wall times far more than any limit here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use bench_gate::{Baseline, CASES, GateError, Limits, Metrics, RUNS, compare};

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("bench-gate: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, GateError> {
    let mut device = None;
    let mut record = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" => device = args.next(),
            "--record" => record = Some(args.next().unwrap_or_default()),
            other => return Err(GateError::BenchFailed(format!("unknown argument {other}"))),
        }
    }
    let device =
        device.ok_or_else(|| GateError::BenchFailed("--device NAME is required".into()))?;
    let path = PathBuf::from("bench/baseline").join(format!("{device}.json"));

    let bench = build_bench()?;
    let mut current = BTreeMap::new();
    for case in CASES {
        let mut runs = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            runs.push(run_case(&bench, case.args)?);
        }
        let best = Metrics::best(&runs)
            .ok_or_else(|| GateError::BenchFailed(format!("{} produced no runs", case.name)))?;
        println!(
            "{:<16} gpu frame {:>6.3} ms   wall p50 {:>6.3} ms   wall p99 {:>6.3} ms",
            case.name, best.gpu_frame_ms, best.wall_p50_ms, best.wall_p99_ms
        );
        current.insert(case.name.to_owned(), best);
    }

    if let Some(note) = record {
        let baseline = Baseline {
            device,
            note,
            cases: current,
        };
        std::fs::create_dir_all("bench/baseline")?;
        std::fs::write(&path, serde_json::to_string_pretty(&baseline)? + "\n")?;
        println!("recorded {}", path.display());
        return Ok(true);
    }

    let baseline: Baseline = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    let slow = compare(&baseline, &current, Limits::default())?;
    for s in &slow {
        println!(
            "SLOWER {} {}: {:.3} ms -> {:.3} ms ({:+.1} %)",
            s.case,
            s.metric,
            s.baseline_ms,
            s.current_ms,
            (s.current_ms / s.baseline_ms - 1.0) * 100.0
        );
    }
    if slow.is_empty() {
        println!("no slowdown against {}", path.display());
    }
    Ok(slow.is_empty())
}

/// Builds the bench in release and returns its path.
fn build_bench() -> Result<PathBuf, GateError> {
    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "-p",
            "engine-render",
            "--example",
            "bench",
        ])
        .status()?;
    if !status.success() {
        return Err(GateError::BenchFailed("cargo build failed".into()));
    }
    Ok(PathBuf::from("target/release/examples/bench"))
}

fn run_case(bench: &Path, args: &[&str]) -> Result<Metrics, GateError> {
    let output = Command::new(bench).args(args).output()?;
    if !output.status.success() {
        return Err(GateError::BenchFailed(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.starts_with('{'))
        .ok_or_else(|| GateError::BadBenchOutput("no JSON line".into()))?;
    Metrics::from_bench_line(line)
}
