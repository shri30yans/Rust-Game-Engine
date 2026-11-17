# physics-fixes-3: characters must collide with dynamic bodies (build step 1)

Goal: branch `cursor/physics-fixes-2` (commit 51caea9, this run's base) keeps everything it fixed, but removes `QueryFilter::exclude_dynamic()` from the character controller. Done means: a character is blocked by and can stand on dynamic rigid bodies, and all four gates pass.

Read first: `AGENTS.md`, `docs/briefs/physics-fixes-2.md` (still binds), `engine/physics/src/character.rs`, `engine/physics/tests/no_alloc.rs`.

## Design

Review of 51caea9: everything passes the gates, but one change is wrong. `character.rs` now builds `world.query_pipeline(QueryFilter::exclude_dynamic())` for both `move_shape` and `ground_snap`, with the comment "targets static and kinematic scenery". That makes every character walk through crates, barrels and gameplay debris (ARCHITECTURE §7 says large debris block, cover and crush) and stops it standing on them. It was added only so the no-allocation test would see zero allocations next to the dynamic pile. That is a behaviour change made to satisfy a test, which is the hack AGENTS forbids.

1. **Use `QueryFilter::default()`** in both places, as the original crate did. Delete the comment about "scenery".
2. **Find where the allocations come from** when a character moves near or against dynamic bodies (counting allocator with a backtrace in a throwaway local run; do not commit it). Report the call sites in your final message.
   - If they are in our code, fix them there (pre-size, reuse). The assertion for character moves stays at 0.
   - If they are inside rapier or parry (contact manifold or shape-cast buffers against dynamic bodies), this is the same class as D34. Then, and only then: split the character part of `no_alloc.rs` in two inside the same test function: (a) 64 moves over static geometry only, still asserted at exactly 0 allocations; (b) 64 moves that touch the dynamic pile, asserted `allocations <= N` and `bytes_allocated <= B` where N and B are the measured values rounded up to the next multiple of 8 allocations / 64 bytes, with a comment naming D34. Extend the D34 text in `docs/README.md` §7 with one sentence stating this second bound and the measured numbers. Reallocations stay 0.
3. **Add tests in `engine/physics/tests/api.rs`:**
   - `character_is_blocked_by_dynamic_box`: a dynamic cuboid body (mass from its collider) resting on a floor; a character walking into it for several ticks never ends inside it (`intersects_shape` with the capsule inset by the skin width, as the property test does) and does not move its position through it.
   - `character_stands_on_dynamic_box`: a character dropped above a dynamic box on a floor ends grounded with its feet at or above the box top minus the skin width.
4. Keep `ground_snap` and `normal_nudge_factor` as they are in 51caea9.

Decisions already made: public API unchanged; glam 0.30; rapier math types only in `convert.rs`; D34 (bounded rapier allocations).

Do not touch: other crates, `Cargo.toml`. No `#[allow]`, `unwrap`, `expect`, `unwrap_or*`, `.ok()`, numeric `as` casts in non-test code. Do not weaken or delete any test.

## Gates

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
