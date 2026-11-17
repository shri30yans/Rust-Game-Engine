# world-resources: singleton state in the world (build step 2)

Goal: let a game keep non-entity simulation state (the physics world, a match clock, a map) inside `World`, so systems (plain `fn(&mut World, &mut SystemContext)`) can reach it and the per-tick state hash covers it. Done means: `engine-world` has typed resources, `engine-physics::PhysicsWorld` is one, both are tested, and all four gates pass.

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §1, `engine/world/src/{world,error,lib}.rs`, `engine/core/src/hash.rs`, `engine/physics/src/world.rs` (`state_hash`).

## Design

Today a system cannot reach anything that is not a serde component, and the physics world is not one. Add a small resource store (this is stored state, not a global: each `World` owns its own).

**`engine/world/src/resource.rs`** (header: one value per type per world; stored in insertion order so the hash is deterministic; no allocation after insertion):
```rust
/// A value stored once per world. It must say how it enters the state hash.
pub trait Resource: std::any::Any + Send + Sync {
    /// Writes everything that is part of simulation state. Two equal states write equal bytes.
    fn hash_into(&self, hasher: &mut StateHasher);
}
```
`ResourceStore { entries: Vec<Box<dyn ErasedResource>> }` (private; `ErasedResource` is a private trait with `fn type_id(&self)->TypeId`, `fn as_any(&self)->&dyn Any`, `fn as_any_mut(&mut self)->&mut dyn Any`, `fn hash_into(&self, &mut StateHasher)`, blanket-implemented for every `R: Resource`). Methods on `World` (all delegate to the store):
- `insert_resource<R: Resource>(&mut self, value: R) -> Result<(), WorldError>`: `WorldError::DuplicateResource(&'static str)` (type name) if present.
- `resource<R: Resource>(&self) -> Result<&R, WorldError>` and `resource_mut<R: Resource>(&mut self) -> Result<&mut R, WorldError>`: `WorldError::MissingResource(&'static str)` if absent. Find by `TypeId` with `iter().find`, then `downcast_ref` / `downcast_mut`; the downcast cannot fail after the TypeId matched, so express it as one `Option` chain (`find_map(|e| e.as_any().downcast_ref::<R>())`), with no unreachable error branch.
- `remove_resource<R: Resource>(&mut self) -> Result<R, WorldError>`: by value; use `Vec::remove` on the position found; to get `R` back use `Box<dyn Any>` (`into_any(self: Box<Self>) -> Box<dyn Any>` on the private trait, then `downcast::<R>()`; on the impossible `Err` return `WorldError::Invariant("resource type id matched but downcast failed")` is NOT allowed, so avoid the problem: do not offer `remove_resource` at all. Resources live as long as the world).
- `state_hash` appends, after the free list and before the final terminator, `write_u32` of the resource count then each resource's `hash_into` in insertion order.
`WorldError` gains `DuplicateResource(&'static str)` and `MissingResource(&'static str)`. Export `Resource` from `lib.rs`; update the module list in the header and the `World` struct docs.

**`engine/physics/src/world.rs`**: `impl Resource for PhysicsWorld { fn hash_into(&self, h) { h.write_u64(self.state_hash()); } }`. If `PhysicsWorld` is not `Send + Sync`, stop and report exactly which field blocks it; do not wrap it in a lock.

## Tests
1. `engine/world/tests/resources.rs`: insert and read back; mutate through `resource_mut`; duplicate insert errors; missing read errors; two resource types are independent; `state_hash` differs after a resource's hashed value changes and is equal for two worlds built the same way; hash differs between a world with and without a resource; hash does not depend on a resource's address (build two worlds in different orders of unrelated allocations and compare).
2. `engine/physics/tests/api.rs` (extend): a `World` with a `PhysicsWorld` resource; stepping it through `resource_mut` changes `World::state_hash`; two identical runs hash equal.
3. `proptest`: any sequence of mutate operations on a resource gives equal hashes in two worlds that applied the same sequence.

## Do not
Touch other crates (except the one `impl` in physics and its test). No `#[allow]`, `unwrap`, `expect`, `unwrap_or*`, `.ok()`, `let _ =`, indexing that can panic, `as` casts in non-test code. No `Rc`, `RefCell`, `Mutex`, `unsafe`, `static`. Docs: `docs/ARCHITECTURE.md` §1 `world` row (resources), `docs/README.md` §7 add D36 "Resources live in the world and enter the state hash" (2026-11-11, decided: why: systems are plain functions and must not hold hidden state; the hash must cover everything that affects a tick; rejected: global singletons, per-system state).

## Gates
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
