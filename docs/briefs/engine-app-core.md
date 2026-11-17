# engine-app-core: the host a game plugs into, headless slice (build step 2, D24/D25/D26)

Goal: a new crate `engine/app` (`engine-app`, lib name `app`) with the fixed-tick loop, the game registration trait, the job pools owned by the app, and a headless runner. Done means: a test game runs N ticks headless, yields one state hash per tick, the hashes are identical with 0, 1 and 4 workers and across repeated runs, and all four gates pass. This slice has no window, no GPU and no render thread; those come in `engine-app-frame`.

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §0 ("How a game plugs in") and §2.1–2.2, `engine/world/src/{lib,schedule,world}.rs`, `engine/core/src/{tick,rng}.rs`, `engine/jobs/src/pool.rs`, `engine/platform/src/profile.rs`.

## Design

**Layering.** Layer 4. Add `{ "pattern": "engine/app", "layer": 4 }` to `tools/check-layers/layers.json` and `"engine/app"` to the workspace `members`. Dependencies: `engine-core`, `engine-world`, `engine-jobs`, `engine-platform` (only for `DeviceProfile`), `thiserror.workspace = true`. Dev: `proptest`, `serde` (derive; workspace). No other dependency. The crate must not name a game, mode, map or weapon.

**Files** (each starts with a `//!` header saying its job and invariants; every public item has docs; `#![forbid(unsafe_code)]` in `lib.rs`):

`src/error.rs`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("tick rate must be between 1 and 1000 Hz, got {0}")]
    InvalidTickRate(u16),
    #[error("max catch-up ticks must be at least 1")]
    ZeroCatchUp,
    #[error("world error: {0}")]
    World(#[from] engine_world::WorldError),
    #[error("job pool error: {0}")]
    Jobs(#[from] jobs::JobError),
    #[error("tick counter overflowed")]
    TickOverflow,
}
```
(Check the exact public names the crates export: `engine_world::WorldError`, the jobs lib name is `jobs`, core is `engine_core`. If a path differs, use the real one.)

`src/clock.rs`: deterministic fixed-step accumulator. No wall clock is read here; the caller passes elapsed nanoseconds.
```rust
/// Simulation ticks per second, 1..=1000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickRate(NonZeroU16);
impl TickRate { pub fn new(hz: u16) -> Result<Self, AppError>; pub fn hz(self) -> u16; pub fn step_nanos(self) -> u64; /* 1_000_000_000 / hz, integer division */ pub fn dt_seconds(self) -> f32; /* 1.0 / f32::from(hz) */ }

/// What `FixedClock::advance` decided for one rendered frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advance { pub ticks: u32, pub dropped_ticks: u32, pub remainder_nanos: u64 }
impl Advance { /// `remainder / step` as f64 in [0,1): how far the next tick is, for render interpolation.
    pub fn alpha(&self, rate: TickRate) -> f64; }   // use f64::from on u32 values; remainder < step <= 1e9 fits u32: convert with u32::try_from and return a typed error instead of casting -> make `alpha` return Result<f64, AppError> with a variant `#[error("clock remainder out of range")] ClockRange`.

pub struct FixedClock { rate: TickRate, max_catch_up: NonZeroU32, accumulator_nanos: u64 }
impl FixedClock {
    pub fn new(rate: TickRate, max_catch_up_ticks: u32) -> Result<Self, AppError>;   // ZeroCatchUp when 0
    /// Adds `frame_nanos`; returns how many whole ticks to run. If more than `max_catch_up` ticks are owed,
    /// run `max_catch_up`, drop the rest (reported in `dropped_ticks`) and keep only the sub-tick remainder.
    pub fn advance(&mut self, frame_nanos: u64) -> Advance;
}
```
Use `saturating_add` for the accumulator; ticks owed = `accumulator / step`, remainder = `accumulator % step`.

`src/game.rs`:
```rust
/// Identity of a game for the network handshake (ARCHITECTURE §0 rule 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameInfo { pub name: &'static str, pub protocol_id: u32, pub protocol_version: u16 }

/// What a game hands the app. The engine never opens a game file by a fixed path.
pub trait Game {
    fn info(&self) -> GameInfo;
    /// Register components and add systems in update order.
    fn setup(&self, world: &mut engine_world::World, schedule: &mut engine_world::Schedule) -> Result<(), AppError>;
}
```

`src/config.rs`:
```rust
pub struct AppConfig {
    pub tick_rate: TickRate,
    pub seed: u64,
    pub max_catch_up_ticks: u32,
    /// Overrides the device profile's job worker count (D35: phones can be sized wrong). None = use the profile.
    pub job_workers: Option<usize>,
}
```
Plain public fields, `#[derive(Debug, Clone)]`.

`src/app.rs`:
```rust
pub struct App<G: Game> { game: G, world: World, schedule: Schedule, tick: Tick, clock: FixedClock, jobs: JobPool, background: BackgroundPool }
impl<G: Game> App<G> {
    /// Builds the world (`World::new(seed)`), the schedule (`Schedule::new(&world.rng().clone(), tick_rate.dt_seconds())`), runs `game.setup`, builds `JobPool::new(workers)` where `workers = config.job_workers.unwrap_or(profile.job_workers())`, and `BackgroundPool::new(background_workers)` where background_workers is 0 for `DeviceClass::Web` and 1 otherwise.
    pub fn new(game: G, config: &AppConfig, profile: &DeviceProfile) -> Result<Self, AppError>;
    pub fn game(&self) -> &G;
    pub fn world(&self) -> &World;
    pub fn world_mut(&mut self) -> &mut World;   // for tests and the sandbox's scene setup
    pub fn tick(&self) -> Tick;                  // number of ticks run so far
    pub fn jobs(&self) -> &JobPool;
    pub fn background(&self) -> &BackgroundPool;
    /// Runs exactly one tick: `schedule.run_tick(&mut world, tick)` then `tick = tick.next()`. Errors on overflow with `TickOverflow` (check `tick.raw() == u32::MAX` first; do not wrap).
    pub fn step(&mut self) -> Result<(), AppError>;
    /// Feeds one frame's elapsed time to the clock and runs the ticks it asks for. Returns the `Advance`.
    pub fn advance(&mut self, frame_nanos: u64) -> Result<Advance, AppError>;
    /// State hash of the world now.
    pub fn state_hash(&self) -> Result<u64, AppError>;
}
```
`src/headless.rs`: `pub fn run_headless<G: Game>(app: &mut App<G>, ticks: u32) -> Result<Vec<u64>, AppError>`: runs `ticks` ticks and returns the state hash after each (pre-sized `Vec::with_capacity`). No wall clock, no sleeping.

`src/lib.rs`: header, `mod` list, public re-exports of every public type above.

Decisions already made (do not change): fixed tick from a nanosecond accumulator; no wall clock in this crate; integer arithmetic for the clock; the pools belong to the app; systems cannot reach the pool yet (a later brief adds that; do not touch `engine-world`); a game never reaches into the app.

## Tests

`tests/clock.rs`: `TickRate::new(0)` and `new(1001)` error; `new(60).step_nanos() == 16_666_666`; `FixedClock` — exact step gives 1 tick and 0 remainder; half a step gives 0 ticks then another half gives 1; 10 steps in one frame gives 10 ticks; cap: with `max_catch_up = 5`, 12 steps in one frame gives `ticks=5, dropped_ticks=7`, remainder below one step; `new(rate, 0)` errors. Proptest: for random frame-time lists whose total never exceeds the cap per call (use a high cap such as 10_000), the sum of `ticks` equals `total_nanos / step_nanos` however the total is split into frames; `remainder < step` always; `ticks + dropped_ticks` equals the ticks owed.
`tests/app.rs`: a `TestGame` defined in the test file registers a `Counter(u32)` component (serde derive, `Replication::Replicated`), spawns one entity in setup, and adds a system that increments the counter and draws one number from `ctx.rng` and adds it into a second component `Noise(u32)` (wrapping add). Tests: `step` ten times gives `tick().raw() == 10` and `Counter == 10`; `advance` with three steps of nanoseconds runs three ticks; same seed and same ticks give identical hash vectors in two separate runs; a different seed gives a different hash vector; `run_headless` returns `ticks` hashes and the last equals `state_hash()`; hash vectors are identical for `job_workers: Some(0)`, `Some(1)` and `Some(4)`; `job_workers: None` uses `profile.job_workers()` (build the profile with `DeviceProfile::server()` and compare `app.jobs().workers()`); `DeviceClass::Web` gives a background pool that runs a spawned job inline (build a `DeviceProfile` by hand with the public fields for Web); tick overflow: construct the app, call `step` in a loop is too slow, so add a `#[cfg(test)]`-free public method is NOT allowed; instead test `TickOverflow` through a unit test inside `src/app.rs` that sets the private tick field to `Tick::new(u32::MAX)` and calls `step`.
`tests/no_alloc.rs`: exactly ONE `#[test]` (the allocator counter is process-wide). After warm-up, `FixedClock::advance` called 1000 times allocates 0 times and 0 reallocations.

## Docs (same change)

`docs/ARCHITECTURE.md` §1 `app` row State: `slice 1 built: fixed tick, game registration, headless runner; window, pipelined frame and runners come next`; `docs/README.md` §2: the `engine-app` line becomes "slice 1 built (headless), rest planned". Leave `docs/briefs/README.md` statuses to the reviewer.

## Hard rules (a review fails on any of these)

No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`, no panicking indexing, no `as` numeric casts in non-test code. No `#[allow(...)]` in non-test code. No `unsafe`. No swallowed errors (`let _ =`, `.ok()`, `unwrap_or*` that hides a failure, empty `Err(_)` arms or empty `if` blocks). No silent fallbacks, no stubs, TODOs, dead code or duplicate implementations. No `sleep` in tests, no `#[ignore]`. Test files may use `unwrap`/`expect` with a module-level `#![allow(clippy::unwrap_used, clippy::expect_used)]`.

Do not touch: other crates, other files except `Cargo.toml` (members), `tools/check-layers/layers.json`, the two doc lines above.

## Gates

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```

Run `cargo test -p engine-app` 10 times before finishing; all must pass.
