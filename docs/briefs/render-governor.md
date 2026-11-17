# render-governor: dynamic resolution from frame time (low-end phones)

Goal: a pure, tested frame-time governor in `engine-render` that picks the internal `render_scale` each frame so a slow device holds its target frame rate, and `ClientApp` (engine-app) feeds it automatically. Done means: pure unit and property tests, one headless GPU test that shows the scale dropping under a simulated slow frame and recovering, and all four gates pass.

Read first: `AGENTS.md`, `docs/RENDERING.md` §7 (tiers), `engine/render/src/gfx/{mod,config}.rs` (`Settings`, `set_render_scale`, `gpu_times`), `engine/render/src/tier.rs`, `engine/app/src/client.rs`.

## Design
**`engine/render/src/governor.rs`** (header: pure maths, no GPU, no clock reads; the caller passes frame times):
```rust
pub struct GovernorConfig { target_ms: f32, min_scale: f32, max_scale: f32, step: f32, window: u32, headroom: f32 }
impl GovernorConfig { pub fn new(target_ms: f32, min_scale: f32, max_scale: f32) -> Result<Self, RenderError>; } // validates finite, target > 0, 0.4 <= min <= max <= 1.0; defaults step 0.05, window 30, headroom 0.85
pub struct Governor { /* config, current scale, ring of the last `window` frame times (pre-sized, no allocation per frame) */ }
impl Governor {
    pub fn new(config: GovernorConfig, start_scale: f32) -> Result<Self, RenderError>;
    /// Feed one frame time in ms (non-finite or negative is an error). Returns Some(new_scale) only when the scale should change.
    pub fn observe(&mut self, frame_ms: f32) -> Result<Option<f32>, RenderError>;
    pub fn scale(&self) -> f32;
}
```
Rules: once the window is full, if the window average is above `target_ms` lower the scale by `step` (clamped to `min_scale`); if the average is below `target_ms * headroom` raise it by `step` (clamped to `max_scale`); after any change clear the window, so one change needs a full fresh window (no oscillation). A single spike must not change the scale (use the average, test it). Scale values are rounded to multiples of `step` so there are few distinct target sizes.

**Wire-in.** `RenderError` gains `InvalidGovernor` variants only as needed (no unused ones). `ClientApp` gains an optional governor: `ClientApp::with_governor(self, GovernorConfig) -> Result<Self, AppError>`; in `frame`, after `gfx.render`, call `observe(dt * 1000.0)` and on `Some(s)` call `gfx.set_render_scale(s)`. It uses the wall-clock frame time `dt` the platform passes, so it works without GPU timestamps. Games opt in; the engine names no game. The sandbox and imagine builders enable it with a 60 Hz target (16.6 ms) on phones only (the device profile says which; read how `build_client` already chooses settings).

## Tests
1. `governor.rs` unit tests: validation errors; steady slow frames lower the scale step by step to `min_scale` and stop; steady fast frames raise it to `max_scale`; one spike in a full window changes nothing; after a change the window must refill; values are multiples of `step`; proptest: the scale is always within `[min, max]` for any sequence of finite frame times.
2. An allocation test (counting allocator, as in `engine/app/tests/no_alloc.rs`) proving `observe` allocates nothing after `new`.
3. `engine/app/tests/governor_headless.rs`: drive a `ClientApp` headless with a fake game whose `dt` is large for a while; assert the `Gfx` scale dropped below 1.0 and returned after fast frames. Needs a read accessor `Gfx::render_scale()`; add it (documented). No GPU adapter means fail loudly, not skip.

## Do not
Touch other crates beyond the two builders' settings line. No `#[allow]`, `unwrap`, `expect`, `unwrap_or*`, `.ok()`, `let _ =`, panicking indexing, truncating `as` casts, threads or clock reads in the governor. No claim of phone performance: you can only claim what you measured on this Mac.

Docs: `docs/briefs/README.md` row, `ARCHITECTURE.md` §1 render row, `docs/RENDERING.md` §7 (one short paragraph), a `README.md` §7 entry (next free number: D43) giving why the average window and step rule, and the rejected options (per-frame proportional control: oscillates; GPU-timestamp-only control: unavailable on many phones).

## Gates
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
