# Engineering rules (apply to every change, human or agent)

Start at `docs/README.md` (map of the docs, current state, how to continue in a new session). The engine is a platform that many games are built on (`docs/ARCHITECTURE.md` §0). Why we build it this way and how decisions are recorded: `docs/PRINCIPLES.md`. The code is split into four folders:
- `engine/`: game-agnostic crates;
- `kits/`: optional genre gameplay;
- `games/<name>/`: one folder per game, with its crates, content and docs;
- `tools/`.

Layout, layers and the decoupling rules are in `docs/ARCHITECTURE.md` §0–1.

## Core tenets, in priority order

When two tenets pull against each other, the earlier one wins.

1. **Fix the cause, not the symptom.** For every bug, ask why the system allowed it, then change the design so that whole class of mistake cannot happen again. Correcting the bad value alone is not a fix.
   - Report both: what went wrong, and what let it happen.
   - Example: the phone crash, where an off-screen touch button closed the game, became one layout per frame, touch tested against drawn regions, screen rectangles that exist only clipped, and presentation errors that never stop the client.
2. **Correct by construction.** Make illegal states unrepresentable.
   - Check data once, at the boundary (cook, load or bind), never per frame.
   - A run-time error that valid input can trigger is a design bug.
   - Every legal order of events, such as an OS lifecycle or input arriving early, is handled with no error.
3. **One canonical path per concern.**
   - No second copy of anything: no parallel layout, no duplicate type, no shim.
   - Nothing half-built: no stubs, no "wired later" comments, no fields nothing reads, no test fixtures in production code.
   - If something is incomplete, it is absent.
   - The canonical path for each concern (start-up, content, input, rendering, UI, errors) is listed in `docs/ARCHITECTURE.md` §14, with the Unreal design it follows. Never add a branch for a platform, build type or game to any of them.
4. **Simple, clean and readable.**
   - Use the fewest moving parts that are still correct. When in doubt, follow Unreal (`docs/PRINCIPLES.md` §3).
   - Names say what a thing is; comments say why.
   - A reader should understand a module from its header and its public items.
5. **Small files in clear folders.**
   - One responsibility per file. Aim for 300 lines of non-test code per file; 500 is the hard limit.
   - A subsystem gets a folder whose `mod.rs` only declares, re-exports and documents.
   - Tests that do not need private access live in `tests/`. Unit tests over about 150 lines move to a sibling `tests.rs` (`#[cfg(test)] mod tests;`).
   - Split by what the code does, never by size alone (no `part1.rs`).
6. **The engine is a platform**. Engine and kits never know a game. A game is data and its own crates on top.
7. **Robust where players are.**
   - Nothing in presentation, input, assets or the OS lifecycle can close the game. It reports the problem and keeps the last good state.
   - Changes that touch devices are checked on the real test phones, not only on desktop.
8. **Measured, not claimed.**
   - Tests come with the code.
   - Every performance claim has a number from a bench or a device.
   - Every correctness claim has a passing test.
9. **Player experience first, then performance.** Build what the player sees and feels. Optimise after a measurement shows the need.

## Quality bar

1. Tenet 3 applies: no duplicate implementations, no compatibility shims, no `// TODO later` hacks.
2. Tenet 2 applies: encode invariants in types (newtypes for ids and units, enums over flags, builders that cannot produce invalid values).
3. No `unwrap`, `expect`, `panic!`, indexing that can panic, or `as` casts that can truncate in non-test code. Use typed errors (`thiserror`) and propagate with `?`. Lints in the workspace root deny these (`clippy::unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `cast_possible_truncation`).
4. `unsafe` is forbidden by default (`#![forbid(unsafe_code)]`). Any exception needs a `// SAFETY:` comment, a test, and a line in the owning doc (today only `engine-gpu`, `docs/RENDERING.md` §2, and the Android entry and logging, `docs/ARCHITECTURE.md` §4).
5. No hidden global state, no singletons, no `static mut`, no thread-local RNG and no wall-clock reads in simulation code.
6. No allocation in per-tick hot paths: pre-size buffers, reuse scratch space, and prove it where it matters (a bench or a counting-allocator test).
7. Every public item has docs. Every module has a short header explaining its job and its invariants.
8. Tests come with the code: unit tests, property tests (`proptest`) for invariants, and determinism and round-trip tests where relevant. A change without tests is not done.
9. Layering: obey the dependency rule in `docs/ARCHITECTURE.md` §1. `cargo run -p check-layers` must pass.
10. Decoupling (`docs/ARCHITECTURE.md` §0):
    - Engine and kit code never names a game, mode, map or weapon, and never opens a path into `games/`.
    - A game uses only the engine, kits and its own crates.
    - A feature that only one game uses lives in that game.

## Definition of done

- Each change passes `scripts/gate.sh`: fmt, layers, and clippy and tests of the crates it changed. It does not run the whole workspace; parallel runs each testing everything was most of the build time.
- Before merging to main, `scripts/gate.sh --full` (fmt, layers, workspace clippy and tests) and `scripts/build-android.sh imagine` pass. A failure there is fixed at its cause before anything else merges.
- Every item in the brief's "Done means" is done end to end. A partly built feature is removed, not left in, and the report lists what was left out.
- No files outside the task scope were changed, except to fix a defect found along the way.
- No file you touched grows past the tenet 5 limit; split it in the same change.
- Docs changed in the same change:
  - the brief's status in `docs/briefs/README.md`;
  - the crate's state in `docs/ARCHITECTURE.md` §1;
  - any new or changed decision, written into the doc that owns its topic (Decision, Why, Rejected; no separate log);
  - for game work, that game's `games/<name>/docs/`.

## Style

Rust 2024. See tenets 4 and 5.

## Who builds

Claude designs, builds, tests and keeps the docs true, in this repository, on a branch per task when the owner wants one. No other coding agent is used (owner, 2026-11-18). Parallel work, when it helps, uses Claude subagents on files that do not overlap, and Claude reviews their diffs against these tenets before anything merges. The process is in `docs/WORKFLOW.md`.
