# build-speed-1: one integration-test binary per crate, measured edit-to-green times (D53)

Goal: changing an engine crate and running the tests is fast. Done means:
1. Each crate has at most one integration-test binary.
2. Edit-to-green times are measured before and after with a real code change, repeated runs and nothing else building.
3. Any build-setting change is kept only if the measurement shows it helps.
4. The four gates pass.

Starts after physics-shapes, render-assets, imagine-menus and imagine-combat-feel merge, because it moves test files in every crate.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D53;
- the root `Cargo.toml` profiles;
- every crate's `tests/` directory.

## What we know (2026-11-12, not yet reliable)
- About 79 integration-test files across 16 crates, each its own binary: engine/app 14, engine/ui 9, engine/netcode 9, games/imagine/game 8, and others.
- In a `--timings` run, each test binary took about 3 s to build.
- A one-line change in `engine/core` once made `cargo test --workspace` take about 137 s, against about 21 s with nothing changed.
- The test run itself takes about 20 s, with the slowest binaries at 4.6 s (`tools/cook/tests/pipeline.rs`) and 2.2 s (`engine/app/tests/governor_headless.rs`).
- Earlier env-var experiments were single runs with a comment-only edit and other builds running, so they prove nothing.

## Design
1. **Measure first** (`scripts/build-times.sh`, committed):
   - Make three real edits, each reverted after timing:
     - add `pub const BUILD_PROBE: u32 = N;` to `engine/core/src/lib.rs`, changing N each run;
     - the same probe in `kits/fps/src/lib.rs`;
     - the same probe in `games/imagine/game/src/lib.rs`.
   - For each edit, time `cargo test --workspace --no-run` and `cargo test --workspace`. Run three times and report the median.
   - Refuse to run when `git status` shows tracked changes, so the script never measures a dirty tree.
2. **One test binary per crate**, the usual Rust layout:
   - Move `tests/*.rs` into modules of one `tests/it/main.rs` per crate, with each old file as `mod name;` in `tests/it/`.
   - Fixtures keep their paths, or update `include_bytes!` and `env!("CARGO_MANIFEST_DIR")` paths.
   - Tests that need their own process stay separate and say why in a one-line comment: a counting global allocator, a GPU device, or `std::process` state. Group several of them into one binary where they can share a process. For counting-allocator tests, one binary per crate that installs the allocator once.
   - Test names and counts stay the same. Report the count before and after, which must match.
3. **Settings experiments**, each measured with the script, three runs. `debug = "line-tables-only"` and dependency `debug = false` were applied on 2026-11-12 (D76) ahead of this brief: measure them against full debug info and revert them if they are not better:
   - `[profile.dev] debug = "line-tables-only"`;
   - `split-debuginfo = "unpacked"` (check the macOS default first and skip it if it is already the default);
   - the `[profile.dev.package."*"] opt-level` for dependencies at 3 against 2.
   - Keep a change only if it is faster by more than the run-to-run spread. Otherwise leave the setting as it is and report the numbers.
5. **Disk (measured 2026-11-12).** The main `target/` was 65 GB:
   - `debug/deps` held 35 GB, including four copies of `librapier3d` at 71 MB each, because the same dependency was built with different feature sets by `cargo test -p X`, `cargo test --workspace` and clippy;
   - `debug/incremental` held 24 GB;
   - there were 1,463 test and example executables (8.8 GB).
   Every agent worktree repeats this at 10 to 17 GB, and 13 of them filled the disk, which killed a run. Fix it at the cause, and measure the size before and after:
   - **One feature set per dependency.** Unify features with a workspace-hack crate managed by `cargo hakari` (the standard tool for this), so every command builds each dependency once.
   - **Less debug info:** `debug = "line-tables-only"` for the workspace (as in item 3) and `debug = false` for dependencies (`[profile.dev.package."*"]`). Report the size and the link time.
   - **One test binary per crate** (item 2) also removes most of the executables.
   - Report `du -sh target` after a clean `cargo test --workspace`, before and after.
4. Do not add sccache, a different linker or nightly flags in this brief. If the numbers show linking dominates, say so in the report as the next step.

## Tests
- `cargo test --workspace` passes with the same test count as before.
- The script's numbers before and after are in the report and in `docs/WORKFLOW.md`.

**Do not touch** any non-test source except the probe lines the script adds and reverts, and the root `Cargo.toml` profiles if a setting proves faster.

**Docs in the same change:**
- `docs/WORKFLOW.md` (the test layout rule, how to run `scripts/build-times.sh`, the numbers);
- `AGENTS.md` if the test layout becomes a rule ("one integration-test binary per crate unless it needs its own process");
- a new decision in `docs/README.md` §7 if a profile setting changes;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
