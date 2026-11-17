# physics-rapier-036-fixes: make the rapier 0.36 port pass every physics test (build step 1)

Goal: finish the port on branch `cursor/physics-rapier-036` (commit 6079c2e, base of this run). Done means: all four gates pass on the whole workspace with **no test weakened or deleted**, and the review items below are fixed.

Read first: `AGENTS.md`, `docs/briefs/physics-rapier-036.md` (the original brief; its decisions still bind), `engine/physics/src/*`, `engine/physics/tests/*`, and rapier source in `~/.cargo/registry/src/*/rapier3d-0.36.1/src/control/character_controller.rs`.

## Design

Three tests fail; one banned pattern was introduced. Fix them as follows.

1. **`character_detects_ground_and_jumps` (`tests/api.rs`).** Cause (verified in rapier source, `move_shape`): in 0.36 `snap_to_ground` runs only when the character was already grounded at its starting pose, and "grounded" uses a prediction distance of `offset + 0.05` (0.06 with the default skin 0.01). The test starts the capsule bottom 0.1 above the floor, inside our documented `ground_snap_distance` (0.15), so nothing snaps and `grounded()` is false. Our public contract (`CharacterConfig::ground_snap_distance`, unchanged since the original crate) is: a character falling within that distance of ground lands on it and is grounded. Keep that contract in `engine/physics/src/character.rs` as one explicit step after `move_shape`: if `!movement.grounded`, the vertical velocity is not upward (`<= 0`) and the resulting translation does not move up, shape-cast the capsule straight down by `ground_snap_distance` from the end pose using `world.query_pipeline(...)`, with `target_distance = skin_width`, `stop_at_penetration = false`; on a hit, move down by the hit distance and report grounded. Pure helper `fn ground_snap(...)` in `character.rs`, one place, doc comment saying why (rapier 0.36 only snaps when already grounded). If rapier's own snap already ran (`movement.grounded`), do nothing. The public API of `CharacterConfig`, `CharacterState`, `CharacterInput`, `CharacterMovement`, `CharacterController` stays unchanged.
2. **`character_never_ends_inside_random_static_wall_or_ramp`.** Re-run after fix 1. If it still fails, find the actual cause (print the failing proptest input; minimise it) and fix it in `character.rs` / `convert.rs`. Candidates to check first: `up`/slope constants now passed as a rapier `Vec3` (check `max_slope_climb_angle` / `min_slope_slide_angle` semantics unchanged in 0.36), the character pose built with `Quat::IDENTITY`, and the same snap issue. Do not loosen tolerances or shrink the input ranges. If the cause is a rapier 0.36 behaviour that cannot be fixed inside the unchanged public API, **stop and report with the minimal failing input**.
3. **`warmed_physics_hot_paths_allocate_nothing` (`tests/no_alloc.rs`).** `step()` allocates 3 times (592 bytes) per warmed step. Find each allocation site (counting allocator that records a backtrace in a throwaway local run; do not commit that instrumentation). Likely suspects: `broad_phase_pair_events` use, `soft_bodies`, 0.36 BVH refit, `Vec`s inside `PhysicsPipeline`/`NarrowPhase`. Fix what is ours (pre-size, reuse, call `reserve`, warm in the test's existing warm-up). If the allocations are inside rapier and cannot be avoided, **stop and report** the call sites; do not weaken the test and do not mark it ignored.
4. **Review items.**
   - `world.rs::remove_body` uses `.unwrap_or_default()` on a missing body, hiding a failure. Look the body up first and return `PhysicsError::StaleBody(id)` when it is absent; delete `rigid_body_attached_colliders` (inline `body.colliders().to_vec()` after the lookup, or iterate without allocating if possible).
   - `sync_collider_in_broad_phase` silently returns when `!self.colliders.contains(handle)`: that case is unreachable right after an insert, so remove the check.
   - The query tests in `tests/api.rs` (ray, shape, intersection after insert/remove) must pass; add one test `queries_see_collider_removed_and_inserted_between_steps`: insert, query hits, remove, query misses, with no `step()` in between.
5. **Docs (same change).** Set `physics-rapier-036` to `merged` in `docs/briefs/README.md` is the reviewer's job; you only update `docs/ARCHITECTURE.md` §1 `physics` row to "built" (remove "does not compile") and `docs/README.md` §2 physics line accordingly, **only if all gates pass**.

Decisions already made: glam stays 0.30; all rapier math types stay inside `convert.rs`; public API of `engine-physics` unchanged; state-hash order unchanged.

Tests: the existing suite unchanged plus `queries_see_collider_removed_and_inserted_between_steps`. Add `character_snaps_to_ground_within_snap_distance_and_not_beyond` (capsule 0.1 above floor is grounded after one move; 0.5 above is not).

Do not touch: any crate other than `physics`, except ARCHITECTURE/README lines named above. No `#[allow]`, no `unwrap`/`expect` in non-test code.

## Gates

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
