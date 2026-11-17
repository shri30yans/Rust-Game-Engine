# shader-hot-reload: GX1, WGSL shader library and hot reload in `render` (dev feature)

Goal: with the cargo feature `hot-reload` enabled on `engine-render`, editing any file in `engine/render/src/shaders/` while the `smoke` example runs rebuilds the affected pipelines within one second, without restarting. A shader with errors is reported through `tracing`/stderr and the previous pipeline keeps running. Without the feature, behaviour and binary contents are exactly as today (shaders embedded at compile time).

Read first: `AGENTS.md`, `docs/RENDERING.md` §2, `engine/render/src/lib.rs`, `engine/render/src/wgsl.rs`, `engine/render/src/gfx/mod.rs` (especially `Gfx::new`, lines ~218–880, where every shader module and pipeline is created), `engine/render/src/tiles.rs`, `engine/render/examples/smoke.rs`.

## Design

1. **One canonical shader source path.** Create `engine/render/src/shaders.rs` (replaces `wgsl.rs`; delete `wgsl.rs`):
   - `pub(crate) enum ShaderId { Scene, Shadow, Post, Gtao, GtaoDenoise, Ui, Tiles }`, with `ShaderId::ALL`.
   - `fn files(self) -> &'static [&'static str]`: the file names that make up each module (Scene = `scene.wgsl`, `scene_patterns.wgsl`, `scene_lighting.wgsl` concatenated in that order; the others one file each).
   - `pub(crate) struct ShaderLibrary` that returns the WGSL source for an id. Default build: sources from `include_str!` (one `match`). With `feature = "hot-reload"`: sources read from `concat!(env!("CARGO_MANIFEST_DIR"), "/src/shaders/")`, falling back to the embedded text if a read fails (log it).
   - `pub(crate) fn create_module(device, id, &ShaderLibrary) -> Result<wgpu::ShaderModule, RenderError>`: wraps creation in `device.push_error_scope(wgpu::ErrorFilter::Validation)` / `pop_error_scope` (block with `pollster`, already a workspace dependency; add it to render's deps) and returns a typed `RenderError::ShaderCompile { id, message }` instead of panicking.
   - Every place that creates a shader module today (`gfx/mod.rs` lines ~237–243, `tiles.rs:52`) goes through `create_module`. No `include_wgsl!` remains.
2. **Pipelines rebuildable per shader.** Refactor `Gfx::new` so every render/compute pipeline is created by a private function taking `(&wgpu::Device, &PipelineLayouts, &wgpu::ShaderModule, ...)`, where `PipelineLayouts` is a struct holding the bind group layouts and pipeline layouts that `new` already builds. Store `PipelineLayouts` in `Gfx`. Add `fn rebuild(&mut self, id: ShaderId) -> Result<(), RenderError>`, which recreates the module and only the pipelines that use it, swapping them in only if all succeed.
3. **Watcher (feature-gated).** `engine/render/src/hot_reload.rs`, compiled only with `hot-reload`: `pub(crate) struct ShaderWatcher` storing the last modified time per file (`std::fs::metadata`, no new dependency). `fn poll(&mut self) -> SmallList<ShaderId>` returns the ids whose files changed (check every file at most every 250 ms; the caller passes `std::time::Instant::now()`, render is not simulation code). `Gfx::begin_frame` calls it and `rebuild`s each changed id, logging success or the compile error.
4. **Feature wiring:** `[features] hot-reload = []` in `engine/render/Cargo.toml`. `cargo run -p engine-render --example smoke --features hot-reload` is the documented way to use it; add that line to the `engine/render/src/lib.rs` header.
5. Do not change any WGSL file, any visual result, or any public API besides adding the feature.

Tests:
- Unit (no GPU): `ShaderId::ALL` covers every `.wgsl` file in `engine/render/src/shaders/` exactly once (read the directory in the test); the Scene concatenation order is the one above.
- With `hot-reload`: `ShaderWatcher::new(root: &Path)` takes the shader directory as a parameter and `poll(now: Instant)` takes the clock, so the test uses a temp dir, sets mtimes explicitly with `std::fs::File::set_modified`, and passes synthetic instants (no sleeps). Assert: unchanged → empty; one file changed → its id once; a change to `scene_lighting.wgsl` → `Scene`; polls closer than 250 ms apart do not re-read.
- GPU test, skipped with a printed reason when no adapter is available (follow the existing headless pattern in `platform/src/headless.rs`): `create_module` on broken WGSL returns `ShaderCompile` and does not panic; `rebuild(ShaderId::Post)` succeeds on the real sources.
- Run `cargo run -p engine-render --example smoke` (if a display is available) and `--example bench` before and after; report both outputs.

Do not touch other crates.

## Gates

```
cargo fmt --check
cargo clippy -p engine-render --all-targets -- -D warnings
cargo clippy -p engine-render --all-targets --features hot-reload -- -D warnings
cargo test -p engine-render
cargo test -p engine-render --features hot-reload
cargo run -p check-layers
```
