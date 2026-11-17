# engine-app-frame: extract, the frame exchange and the late camera (build step 2, D26)

Goal: add to `engine-app` the CPU side of the pipelined frame (ARCHITECTURE §3, D26): an allocation-free `Extractor` that copies transforms from the world into a pre-sized `RenderFrame`, a two-slot `FrameExchange` that lets the simulation fill frame N+1 while the render side draws frame N (never more than one frame ahead), and the late-camera maths. Done means: all of it is tested without a window or GPU, it works with two threads and with one, and all four gates pass. The window, wgpu and the renderer are NOT touched here (they come in the client-runner brief).

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §3, `docs/README.md` §7 D26, `engine/app/src/*` (the crate you extend), `engine/world/src/{world,transform,net_id,error}.rs`.

## Design

**Layering.** No new crate and no new dependency. Everything goes in `engine/app/src/`, with new modules added to `lib.rs` and re-exported. Dev-dependency `stats_alloc.workspace = true` is already there.

**`src/extract.rs`** (header states: copies, never borrows, the world; no allocation after construction):
```rust
/// One entity's transform at the last two ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtractedTransform { pub id: NetId, pub previous: Transform, pub current: Transform }

/// What drawing needs for one simulated tick. Pre-sized; never grows.
#[derive(Debug)]
pub struct RenderFrame { tick: Tick, transforms: Vec<ExtractedTransform> /* capacity fixed at construction */ }
impl RenderFrame {
    pub fn with_capacity(capacity: usize) -> Self;      // tick = Tick::ZERO, empty
    pub fn tick(&self) -> Tick;
    pub fn transforms(&self) -> &[ExtractedTransform];
    pub fn capacity(&self) -> usize;                     // the fixed capacity (Vec::capacity right after construction, stored in a field)
}

pub struct Extractor { last: Vec<(NetId, Transform)>, scratch: Vec<(NetId, Transform)> }   // both with_capacity(capacity) at construction
impl Extractor {
    pub fn with_capacity(capacity: usize) -> Self;
    /// Fills `out` (cleared first) with every entity that has a `Transform`, in ascending `NetId` order
    /// (`World::net_ids` already yields ascending ids). `previous` is the transform the same id had at the
    /// previous `extract` call (found with `binary_search_by_key` on `last`, which is sorted); a new id gets
    /// `previous = current`. Afterwards `last` holds this call's transforms (build them in `scratch`, then
    /// `std::mem::swap`). Entities without a `Transform` are skipped; this is decided by matching the
    /// `WorldError::MissingComponent` variant (check its real name and fields in `engine/world/src/error.rs`),
    /// and EVERY OTHER error is returned. Errors with `AppError::ExtractCapacity { capacity, needed }`
    /// when more entities than `out.capacity()` (or `self` capacity) would be written; it must not grow any Vec.
    pub fn extract(&mut self, world: &World, tick: Tick, out: &mut RenderFrame) -> Result<(), AppError>;
}
```
`RenderFrame` gets `pub(crate)` helpers as needed so `Extractor` can fill it; the fields stay private. Capacity check order: count first, then write, so a failed extract leaves `last` unchanged.

**`src/exchange.rs`** (header: the one place the simulation thread and the render thread meet; it is a bounded hand-off of whole frames, not shared simulation state, so a `Mutex` and `Condvar` are allowed here and only here):
```rust
pub fn frame_exchange(capacity_per_frame: usize) -> (SimEnd, RenderEnd);
```
Two `RenderFrame`s are created up front. Shared state (behind `Arc<Shared>`): `Mutex<State>` and one `Condvar`, `State { free: Vec<RenderFrame> /* with_capacity(2) */, ready: VecDeque<RenderFrame> /* with_capacity(2) */, sim_alive: bool, render_alive: bool }`. Initially `free` holds both frames. Methods:
- `SimEnd::acquire(&mut self) -> Result<RenderFrame, AppError>`: waits until `free` is non-empty (returns the frame) or the render end was dropped (`AppError::ExchangeClosed`).
- `SimEnd::publish(&mut self, frame: RenderFrame) -> Result<(), AppError>`: pushes to `ready`, notifies; `ExchangeClosed` if render end dropped.
- `RenderEnd::next(&mut self) -> Result<RenderFrame, AppError>`: waits until `ready` is non-empty (pops the OLDEST) or the sim end was dropped AND `ready` is empty (`ExchangeClosed`); frames already published are still delivered after the sim end is dropped.
- `RenderEnd::recycle(&mut self, frame: RenderFrame) -> Result<(), AppError>`: pushes to `free`, notifies; `ExchangeClosed` if sim end dropped.
- `Drop` for both ends clears its alive flag and notifies. A poisoned mutex is a bug elsewhere: map it to `AppError::ExchangePoisoned` (never `into_inner`, never panic).
Because only two frames exist, the simulation is at most one frame ahead of the renderer. No allocation after construction (the Vecs are pre-sized, frames are moved, never cloned).

**`src/late_camera.rs`** (header: the render thread applies the newest look input just before recording, and the input sent to the server carries the same angles, so pipelining adds no aim delay, PRINCIPLES §5.1 G1):
```rust
/// Radians, validated finite. yaw wraps into (-PI, PI]; pitch is clamped to ±PITCH_LIMIT where PITCH_LIMIT = FRAC_PI_2 - 0.001.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewAngles { yaw: f32, pitch: f32 }
impl ViewAngles { pub fn new(yaw: f32, pitch: f32) -> Result<Self, AppError>;  pub fn yaw(self)->f32; pub fn pitch(self)->f32;
    /// Adds a look delta: yaw wraps, pitch clamps. Pure.
    pub fn turned(self, delta: LookDelta) -> Self; }
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LookDelta { yaw: f32, pitch: f32 }       // radians, finite
impl LookDelta { pub fn new(yaw: f32, pitch: f32) -> Result<Self, AppError>; pub const ZERO: Self; ... getters }

/// Input thread adds, simulation drains each tick, render thread peeks the not-yet-drained part.
#[derive(Clone)] pub struct LookInbox { inner: Arc<Mutex<LookDelta>> }
impl LookInbox { pub fn new() -> Self; pub fn add(&self, delta: LookDelta) -> Result<(), AppError>;
    pub fn drain(&self) -> Result<LookDelta, AppError>;  // returns the total and resets to ZERO
    pub fn peek(&self) -> Result<LookDelta, AppError>; }  // total without resetting
```
`add` accumulates with plain `f32` addition and re-validates finiteness (an overflow to infinity is `AppError::NonFiniteAngle`). Add `LookInbox: Default`? No: only `new`.
Wrapping yaw: use `rem_euclid(TAU)` then shift into `(-PI, PI]` (`if y > PI { y - TAU } else { y }`).

**`src/error.rs`**: add variants `ExtractCapacity { capacity: usize, needed: usize }`, `ExchangeClosed`, `ExchangePoisoned`, `NonFiniteAngle`, each with a `#[error(...)]` message and a doc line.

Decisions already made (do not change): copy-based extract; two frames in flight; whole-frame hand-off; late camera applies peek-after-drain; single-threaded mode is just calling `acquire, fill, publish, next, draw, recycle` on one thread (it works because `free` starts with two frames; do not add a separate code path for it).

## Tests

`tests/extract.rs`: world with 3 entities (spawn through `World::spawn`, insert `Transform::new(...)` with distinct translations): first extract gives `previous == current`; mutate one translation via `get_mut`, tick+1, extract again: that entity's `previous` is the old value and `current` the new, others unchanged; ascending id order; a spawned entity without `Transform` is skipped; a newly spawned entity between extracts gets `previous == current`; a despawned/removed entity vanishes (use the world's real removal method; check its name); capacity exceeded returns `ExtractCapacity` with the right numbers and a following successful extract (after the entity count fits again) still has correct `previous` values (proves `last` was not corrupted).
`tests/exchange.rs`: single-thread cycle: `acquire, publish, next, recycle` four times with no blocking; two frames can be acquired back to back, a third `acquire` on the same thread is impossible to test without blocking so instead run it on a second thread and prove it unblocks only after `recycle` (use `std::sync::mpsc` signalling, no sleeps); two-thread run: simulation thread publishes 200 frames each stamped with `tick` = its index (set via the pub(crate) helper used by Extractor, or by running a real `Extractor` over a world whose single entity moves each tick), render thread receives them all in order with no gaps and no repeats; dropping the render end makes `acquire` return `ExchangeClosed`; dropping the sim end after publishing still lets the render end drain published frames, then `next` returns `ExchangeClosed`; frames never exceed 2 in flight (assert with an `AtomicUsize` counter incremented on acquire and decremented on recycle, maximum observed <= 2).
`tests/late_camera.rs`: `ViewAngles::new` rejects NaN and infinity; pitch clamps at ±limit for large deltas; yaw wraps (`PI + 0.1` becomes about `-PI + 0.1`); proptest: for finite angles and deltas in ±100 rad, `turned` always yields yaw in `(-PI, PI]` and pitch within the limit, and turning by a then b equals turning by the sum of a and b when the pitch limit is not hit (compare with a tolerance of 1e-3); `LookInbox`: `add` twice then `peek` returns the sum and leaves it, `drain` returns the sum and then `peek` is `ZERO`; adding `f32::MAX` twice gives `NonFiniteAngle`; the same inbox cloned across two threads accumulates both threads' adds (use joins, not sleeps).
`tests/no_alloc.rs` (extend the existing file only by adding to its single `#[test]`; the file must still contain exactly one `#[test]`): after warm-up, 1000 `Extractor::extract` calls over a world of 64 entities allocate 0 times and 0 reallocations, and 1000 `acquire, publish, next, recycle` cycles allocate 0 times.

## Docs (same change)

`docs/ARCHITECTURE.md` §1 `app` row State: add "extract, frame exchange and late camera built (client runner next)"; `docs/README.md` §2 engine-app line likewise. Leave `docs/briefs/README.md` to the reviewer.

## Hard rules (a review fails on any of these)

No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`, no panicking indexing, no `as` numeric casts in non-test code. No `#[allow(...)]` in non-test code. No `unsafe`. No swallowed errors (`let _ =`, `.ok()`, `unwrap_or*` that hides a failure, empty `Err(_)` arms or empty `if` blocks). No silent fallbacks, stubs, TODOs, dead code or duplicate implementations. No `sleep` in tests, no `#[ignore]`. Test files may use `unwrap`/`expect` with a module-level `#![allow(clippy::unwrap_used, clippy::expect_used)]`. No `Rc<RefCell>`. `Mutex`/`Condvar` only in `exchange.rs` and `late_camera.rs` as specified.

Do not touch: other crates, `Cargo.toml` files, `layers.json`, other docs.

## Gates

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```

Run `cargo test -p engine-app` 15 times (the two-thread tests must never flake) and `cargo test --workspace` 3 times before finishing.
