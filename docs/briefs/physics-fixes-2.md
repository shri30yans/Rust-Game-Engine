# physics-fixes-2: remove the depenetration hack, bound rapier's step allocations (build step 1)

Goal: branch `cursor/physics-fixes` (commit a7a4555, this run's base) passes all four gates, with no workaround code in the character controller. Done means: `cargo test --workspace` is green and the diff in `engine/physics` is smaller than a7a4555's.

Read first: `AGENTS.md`, `docs/briefs/physics-rapier-036-fixes.md` (still binds), `engine/physics/src/character.rs`, `engine/physics/tests/*`, rapier 0.36.1 `src/control/character_controller.rs`.

## Design

Review of a7a4555 found these problems. Fix each.

1. **Delete `depenetrate_config_shape` and its call** in `character.rs`. It is a brute-force workaround: it swallows an error with `.unwrap_or(...)`, casts `step as f32` (a truncating-style `as` cast), searches six axes by trial, and hides whatever really makes the character end a skin-width inside geometry. Keep `ground_snap` (it follows the previous brief) and keep `normal_nudge_factor: config.skin_width` only if it is what makes the failing ramp/wall property test pass.
2. **Find the real cause of the `character_never_ends_inside_random_static_wall_or_ramp` failure without the hack.** Remove the depenetration, run the test, minimise the failing input, and fix the cause in the controller configuration (candidates: `normal_nudge_factor`, `offset`, the `up` vector, `max_slope_climb_angle` / `min_slope_slide_angle`, `autostep`) or in how `move_character` uses `movement.translation`. If the test's own check ("ends not inside") uses a tolerance, state in a comment why the tolerance equals `skin_width` (rapier keeps the capsule `offset` away from surfaces, so a strict zero-overlap test after a move is wrong by design); the check must still fail for a character that ends deeper than `skin_width`. **Do not widen it beyond `skin_width`, and do not shrink input ranges.** If no fix exists inside the unchanged public API, stop and report the minimal failing input.
3. **Allocation decision (owner approved, recorded as D34).** Rapier 0.36's narrow phase allocates about 3 small buffers per step, upstream does too, and it cannot be avoided without forking rapier. Change `engine/physics/tests/no_alloc.rs` as follows, and nothing else in it:
   - The step assertion becomes a bound with the measured current value as the limit: `allocations <= 3` and `bytes_allocated <= 640`, with a comment naming D34 and saying the allocations are inside rapier's narrow phase. Reallocations must stay `0`.
   - Keep the character-move assertions at exactly `0` allocations. If the ground-snap shape cast allocates, fix that in our code; do not widen it.
   - Do not touch the allocator probe at the end of the test.
4. **`remove_body` lookup.** Keep the `StaleBody` fix from a7a4555. Collect the attached collider handles into a small `Vec` once (removal is not a hot path); that is fine as written.
5. **Docs (same change).**
   - Add to `docs/README.md` §7 (before the "UI look" section, after D33's block, in decision-number style):
     `**D34: Rapier's per-step allocations are accepted and bounded** (2026-11-11, decided)`. Text: rapier 0.36 allocates a few small buffers per step inside its narrow phase (PairTransition and stats vectors); upstream master does the same; removing them means forking rapier. We accept at most 3 allocations / 640 bytes per warmed step on the test scene, enforced by `engine/physics/tests/no_alloc.rs`; our own physics code (character moves, queries) stays at zero. Why: a fork costs ongoing maintenance and the amount is tiny against a 60 Hz budget. Reopen if a phone profile shows allocator pressure or rapier fixes it. Rejected: vendoring rapier, waiting for upstream.
   - `docs/ARCHITECTURE.md` §1 `physics` row State: `built`; `docs/README.md` §2 physics line: built, tested.
   - `docs/briefs/README.md`: leave statuses to the reviewer.

Decisions already made: glam 0.30; rapier math types only in `convert.rs`; public API of `engine-physics` unchanged; state-hash order unchanged.

Tests: existing suite unchanged (except the bound in item 3); keep the two tests added in a7a4555.

Do not touch: other crates; `Cargo.toml` versions; no `#[allow]`, `unwrap`, `expect`, `unwrap_or*`, `.ok()`, `as` numeric casts in non-test code.

## Gates

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
