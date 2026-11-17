# physics-rapier-036: port `physics` to rapier 0.36 and make the workspace gates green

Goal: `engine-physics` compiles against the pinned `rapier3d = 0.36.1` / `parry3d = 0.31.1`, every existing physics test and bench builds and passes unchanged in meaning, and the whole workspace passes `cargo fmt --check`.

Done means: all four gates below pass on the whole workspace.

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md`, `engine/physics/src/lib.rs` header, every file in `engine/physics/src`, `engine/physics/tests/*`. Rapier source is in `~/.cargo/registry/src/*/rapier3d-0.36.1/` (read `src/pipeline/query_pipeline.rs` and `src/control/character_controller.rs`).

## Design

Context: `physics` was written for rapier's old nalgebra API (`Point`, `Isometry`, `UnitVector`, an owned `QueryPipeline`, `ShapeCastOptions` in the prelude). rapier 0.36 is glam-native through `glamx` and uses **glam 0.33**, while the workspace uses **glam 0.30**. These are different types.

Decisions (do not change them):
1. **Keep the workspace on glam 0.30.** Do not bump glam; other crates and parallel work depend on it.
2. **One conversion boundary.** Create `engine/physics/src/convert.rs` (private module) holding every conversion between workspace `glam` 0.30 types and rapier's math types (`rapier3d::math::{Vec3, Pose/Isometry equivalent, Rot}` as 0.36 names them). Functions: `to_rapier_vec(Vec3) -> RVec3`, `from_rapier_vec(RVec3) -> Vec3`, `to_rapier_pose(Vec3, Quat) -> RPose`, `from_rapier_pose(&RPose) -> (Vec3, Quat)`, plus whatever rotation conversion is needed. Use the real 0.36 type names. Delete `vector`, `point`, `isometry`, `from_vector`, `from_point` in `world.rs` and route all callers through `convert`. No other module may name a rapier math type directly.
3. **No stored `QueryPipeline`.** In 0.36 it is a borrowed view. Remove the `query_pipeline` field from `PhysicsWorld`. Add a private `fn query_pipeline(&self, filter: QueryFilter) -> QueryPipeline<'_>` built with `self.broad_phase.as_query_pipeline(self.narrow_phase.query_dispatcher(), &self.bodies, &self.colliders, filter)` (check the exact signature in the source). Ray casts and shape casts use it. Remove any manual query-pipeline update in `step`.
4. **Character controller:** port `character.rs` to the 0.36 `KinematicCharacterController::move_shape` signature. Keep `CharacterConfig`, `CharacterState`, `CharacterInput`, `CharacterMovement` and `CharacterController` public APIs **unchanged**.
5. **Public API of the crate is unchanged.** Same types, same function signatures, same error variants. If a rapier change makes one impossible, stop and report instead of changing it.
6. **State hash order and contents are unchanged** (header of `world.rs`): identifier, translation, quaternion (x, y, z, w), linear velocity, angular velocity.
7. Run `cargo fmt` on the whole workspace (it currently fails in `engine/destruction/src/state.rs`, `engine/physics/src/character.rs`, `engine/physics/src/world.rs`, `engine/physics/tests/no_alloc.rs`).

Tests: all existing tests in `engine/physics/tests/*` and `engine/physics/src/tests.rs` must pass without weakening assertions. Add unit tests in `convert.rs`: round-trip `Vec3` and `(Vec3, Quat)` through rapier types (proptest over finite values, exact equality for vectors, quaternion equality up to sign with tolerance 1e-6).

Do not touch: any crate other than `physics`, except formatting-only changes from `cargo fmt`. Do not edit `Cargo.toml` versions.

## Gates

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
