# physics-determinism-bench: measure Rapier with and without `enhanced-determinism` (D54 item 8, audit J4, M5)

Goal: numbers, not opinions, for the decision on cross-platform bit-identical physics. Done means: a bench that runs the same physics workload with the feature on and off reports time per step (p50, p99) and whether same-binary runs stay bit-identical without the feature, and the report and `docs/research/2026-11-11-engine-audit.md` J4 hold the table. No default changes: `enhanced-determinism` stays on in the workspace until the owner decides.

Read first: `AGENTS.md`; `docs/README.md` §7 D6, D7, D34, D54; `docs/research/2026-11-11-engine-audit.md` J4 and M5; `engine/physics/` (all), `engine/physics/benches/physics.rs`; the root `Cargo.toml` rapier line.

## Design

- Add a cargo feature to `engine-physics`, `cross-platform-determinism`, **on by default**, that turns on `rapier3d/enhanced-determinism`. The workspace dependency line drops the feature and `engine-physics` owns the choice (one place). Check that `rapier3d`'s `simd-stable` (or the current name of its SIMD feature) is enabled when the determinism feature is off, and say what the feature list is in each mode.
- A bench `engine/physics/benches/determinism.rs` (criterion if already used in the workspace, else the existing bench style): a scene representative of a match: 24 character controllers moving on scripted inputs, 300 dynamic boxes (debris) falling onto a static floor with 200 static boxes (walls), 3,600 steps at 60 Hz. Report: mean, p50, p99 step time; total time; the state hash at steps 600, 1,800 and 3,600.
- Run it with `--features` on and off (`--no-default-features`), twice each, on this machine (release profile). Same-binary repeatability: the two runs of the same mode must give the same hashes; record whether they do.
- No change to netcode, gameplay or tests' expected hashes.

**Tests:** the existing physics tests pass in both modes (`cargo test -p engine-physics` and `cargo test -p engine-physics --no-default-features`).

**Do not touch:** anything outside `engine/physics`, the root `Cargo.toml` rapier line (only to move the feature into `engine-physics`), and `docs/`. Another brief edits the root `Cargo.toml` profile section: change only the rapier dependency line.

**Docs in the same change:** `docs/research/2026-11-11-engine-audit.md` J4 and M5 with the measured table and a one-line recommendation; `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
