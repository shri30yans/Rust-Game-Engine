# gx2-taa: motion vectors and temporal anti-aliasing (GX2, build step 2)

Goal: High and Ultra tiers (and Medium when `settings.taa` is true) get temporal anti-aliasing: a sub-pixel jitter on the projection, a motion-vector buffer, and a resolve pass that blends the jittered HDR frame with the reprojected history. This removes the "no anti-aliasing on High/Ultra" gap listed in `docs/RENDERING.md` §3. Done means: the pure maths is unit tested; a headless GPU test proves edges are smoother and a static scene converges (not shimmering) while a moving object does not ghost; a measured cost (ms) is recorded; all four gates pass.

Read first: `AGENTS.md`, `docs/RENDERING.md` §3, §5.5, §7, `engine/render/src/{tier,targets,screen_space,shaders}.rs`, `engine/render/src/gfx/{mod,config,setup,pipelines,frame}.rs`, `engine/render/src/shaders/{scene,post,gtao}.wgsl`, `engine/render/examples/{smoke,bench}.rs`, `engine/app/src/client.rs` (how a game supplies the camera).

## Design

**Settings.** `Settings` gains `taa: bool` (default true for High/Ultra via `RenderTier::settings`, false for Test/Low/Medium; Low and Medium keep MSAA). When `taa` is true, `effective_msaa()` is 1. `RenderFeatures` gains `taa: bool` mirroring it, and TAA requires the depth prepass (High and Ultra already have it); constructing a `Gfx` with `taa` true on a tier without the prepass is a typed `RenderError::TaaNeedsPrepass`, not a silent disable.

**Jitter (pure, `src/taa.rs`, no GPU).** `pub fn halton(index: u32, base: u32) -> f32` and `pub struct Jitter` producing the 16-sample (2,3) Halton sequence in [-0.5, 0.5) pixel units, `Jitter::next(&mut self) -> Vec2` cycling, `Jitter::offset_ndc(sample, width, height) -> Vec2` (= sample * 2 / size), and `pub fn jittered_projection(proj: Mat4, ndc: Vec2) -> Mat4` which adds the offset to the third column terms x/y (works for the engine's projection convention: read how `Camera` builds it and test the result against a known pixel shift). Tests: Halton values for the first 8 indices against hand-computed numbers, sequence length and range, jitter shifts a projected point by exactly the expected number of pixels, `next` cycles after 16.

**Motion vectors.** The prepass writes velocity into a new `Rg16Float` render target `velocity` (same size as depth): `velocity = (uv_current - uv_previous)`, computed from the unjittered current and previous clip positions. `Instance` gets `pub prev_model: [[f32; 4]; 4]`; `Instance::new(m)` sets it equal to `m`; add builder `pub fn previous(mut self, m: Mat4) -> Self`. Camera motion comes from `Globals` carrying `prev_view_proj` (unjittered). The `ClientApp`/`Presenter` path needs no change unless the sandbox moves objects; the sandbox presenter should pass the previous-tick transform it already has in the `RenderFrame` (`previous`) via `.previous(...)` for dynamic bodies. Update every `Instance` construction in the workspace that is a struct literal.

**Resolve (`src/shaders/taa.wgsl`, composed through the existing `ShaderLibrary`).** Inputs: current HDR (jittered), history HDR (previous resolved output), velocity, depth. Steps per pixel: take the velocity of the closest depth in the 3x3 neighbourhood; sample history at `uv - velocity` with a Catmull-Rom 5-tap filter; clamp history to the min/max (variance clipping, gamma 1.0) of the 3x3 current neighbourhood in YCoCg; blend factor 0.1 normally, raised toward 1.0 when the history sample is off screen or when the clamp distance is large; output to the history target for next frame and to the target the bloom and tone-map passes read. History textures are a ping-pong pair created in `targets.rs`, recreated on resize (history reset: first frame after creation or resize uses blend 1.0, tracked by a `history_valid: bool` in the renderer, no read of uninitialised memory). The pass runs after the scene pass and before bloom/exposure so exposure and bloom see the resolved image. Optional sharpening is out of scope.

**Frame order.** shadows, prepass (depth+normal+velocity), GTAO, scene (using the jittered projection; GTAO and shadows keep using the unjittered one), TAA resolve, bloom/exposure, tone map. Add `Stage::Taa` to the GPU timer so the cost is measured.

**Bench.** Extend `examples/bench.rs` to print the TAA stage time on High. Record the measured number (machine, resolution, ms) in `docs/RENDERING.md` §7 next to the tier table; if the machine has no timestamp queries, say that.

## Tests
1. Unit: everything in `taa.rs`; the `Settings`/tier mapping (`taa` on High/Ultra, off elsewhere, `effective_msaa` is 1 with TAA); `TaaNeedsPrepass`.
2. `tests/taa_headless.rs` (one `#[test]`, real GPU, fails clearly if there is no adapter): render a scene of a rotated cube against the sky for 16 frames with TAA off and again with TAA on at 256x256; count pixels along the silhouette whose luminance is strictly between background and cube (edge softness): TAA on must produce at least 25 percent more distinct intermediate values AND the mean absolute difference between frame 15 and frame 16 on a static scene must be below 0.01 (converged, no shimmer). Second scenario: move the cube one pixel-ish per frame with correct `prev_model`; the pixel on the trailing edge's luminance one frame after the object leaves must be within 0.05 of the background (no ghost trail); then repeat with `prev_model` deliberately equal to `model` and assert the ghost IS present (proves the velocity path is what removes it).
3. Existing render tests and the smoke example still pass; no test weakened.

## Do not
Add dependencies. Touch other crates beyond the `Instance` construction sites and the sandbox presenter `.previous(...)` use. Add `#[allow]`, `unwrap`, `expect`, `unwrap_or*`, `.ok()`, `let _ =`, panicking indexing or truncating `as` casts in non-test code. Leave a flag that silently disables TAA. Claim quality or speed without the numbers from step Bench.

## Docs
`docs/RENDERING.md` §3 anti-aliasing row and §9 GX2 note ("TAA built; the rest of GX2 pending"), `docs/ARCHITECTURE.md` §1 render row, `docs/README.md` §2 render row, `docs/briefs/README.md` row `gx2-taa` set to `review`.

## Gates
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
cargo run -p engine-render --example bench --release
```
