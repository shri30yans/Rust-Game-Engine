# small-modules: split every oversized file by responsibility, and enforce the limit in check-layers (AGENTS tenet 5)

On 2026-11-12, 48 of 331 Rust files were over 500 lines and 15 were over 1,000. The largest:
- `games/imagine/game/src/present.rs` (1,998);
- `engine/render/src/gfx/mod.rs` (1,956);
- `kits/fps/src/bot.rs` (1,763);
- `engine/ui/src/layout.rs` (1,694);
- `engine/app/src/client.rs` (1,548).

Large files hide more than one job, make reviews slow, and make parallel briefs collide. Done means:
1. No file has more than 500 lines of non-test code.
2. `check-layers` enforces that limit.
3. No behaviour changes: every test, golden and hash is identical before and after.
4. The four gates and bench-gate pass.

Run only when no other brief is editing the same crates: split one crate per commit, and merge main between crates.

## Design

**1. The gate, first.**
- `tools/check-layers` gains a module-size check. For every `.rs` file under `engine/`, `kits/`, `games/` and `tools/`, it counts non-test lines: everything outside `#[cfg(test)]` modules and outside `tests/`, ignoring blank lines and comments. More than 500 fails, naming the file and the count.
- Files over the limit when the gate lands go in a list in `layers.json` (`size_exceptions`) holding their current counts. The check fails if a listed file grows, or if it drops under the limit and is still listed. The list may only shrink, and this brief empties it.

**2. Split by responsibility.**
- Each large file becomes a folder: `foo.rs` becomes `foo/mod.rs` plus one file per job. `mod.rs` only declares, re-exports and documents the module's job and invariants.
- Name the files after what they do, for example `client/lifecycle.rs`, `client/frame.rs`, `client/travel.rs`, `client/input.rs`. Never by size (`part1.rs`).
- Public paths stay the same through re-exports, so callers do not change.
- Unit tests that need private access move to a sibling `tests.rs`. Tests that use only public items move to the crate's `tests/`.

**3. Order.** Largest first, one crate per commit: imagine game, render gfx, kits/fps, ui, app, scene, platform, then the rest.

**4. Proof of no behaviour change.**
- Before you start, record the output of `cargo test --workspace` (test names and results), every golden and every recorded hash.
- After each crate, they must match exactly.
- Run bench-gate three times at the end.

**Do not change:** behaviour, public API paths, or file formats. If a split shows a real defect (dead code, a duplicate path), list it in the report. Do not fix it in this brief unless it is a one-line deletion of unused code.

**Docs:**
- `docs/ARCHITECTURE.md` §1 (folders where they changed);
- `docs/WORKFLOW.md` (the size gate);
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
