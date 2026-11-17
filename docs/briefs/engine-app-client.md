# engine-app-client: the single-threaded client runner (build step 2, D26)

Goal: add to `engine-app` a client runner that opens a window, runs the fixed tick, extracts, blends the last two ticks and draws with `engine-render`, all on the main thread ("single-threaded mode", D26). The threaded mode (frame exchange between two threads) is NOT in this brief. Done means: the pure parts are unit tested, one headless GPU test renders a frame of a scripted game and checks pixels, and all four gates pass.

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §0-1 and §2.2, `docs/README.md` §7 D24 and D26, `engine/app/src/*`, `engine/platform/src/{lib,window,headless,input}.rs`, `engine/render/src/gfx/{mod,config}.rs`, `engine/render/examples/smoke.rs`.

## Design

**Dependencies.** `engine/app/Cargo.toml` gains `render.workspace = true`, `glam.workspace = true` and `wgpu.workspace = true` (all already in the workspace). Layering is already fine (app is layer 4, render layer 2). Nothing in `engine/app` names a game.

**`engine/platform/src/lib.rs`**: add one blanket impl so a runner can keep ownership of its app and read an error afterwards:
```rust
impl<T: PlatformApp + ?Sized> PlatformApp for &mut T { /* forward all four methods */ }
```
Add a unit test with a recording `PlatformApp` that checks every method is forwarded.

**`src/error.rs`** gains: `Render(#[from] render::RenderError)`, `Platform(#[from] platform::PlatformError)`, `ClientNotReady` (a frame arrived before `init`), `AlphaRange` is NOT new (the clock already has `Advance::alpha`). Do not add variants you do not use.

**`src/blend.rs`** (header: pure maths, no GPU):
```rust
/// Blend two transforms: translation and scale by lerp, rotation by slerp, `alpha` in [0, 1].
pub fn blend(previous: &Transform, current: &Transform, alpha: f32) -> Result<Transform, AppError>;
```
It builds the result with `Transform::new` (so invalid values are errors, via `AppError::World`). `alpha` outside [0, 1] or non-finite returns a new `AppError::BlendAlpha(f32)`. Tests: alpha 0 gives previous, alpha 1 gives current, midpoint of two translations, slerp of two rotations is unit length, bad alpha errors, proptest that the result is always a valid transform for alpha in [0, 1].

**`src/client.rs`**:
```rust
/// What a game supplies to be drawn and controlled. The engine never names a game (D24).
pub trait Presenter {
    /// Called once with the renderer, to upload meshes and fonts.
    fn setup(&mut self, gfx: &mut Gfx) -> Result<(), AppError>;
    /// Called once per rendered frame, before ticks run: turn this frame's input into world state
    /// (for example write a command component). `look` is the accumulated view angles after this frame's mouse movement.
    fn apply_input(&mut self, world: &mut World, input: &Input, look: ViewAngles) -> Result<(), AppError>;
    /// Whether the window should grab the mouse this frame (`ClientApp` sets `input.want_grab` from it).
    fn grab_mouse(&self) -> bool;
    /// The camera for this frame, from the blended frame and the view angles. Fails with `AppError` when the frame lacks what it needs.
    fn camera(&self, frame: &RenderFrame, alpha: f32, look: ViewAngles) -> Result<Camera, AppError>;
    /// Queue draws for this frame. `blended` yields `(NetId, Transform)` already blended; see below.
    fn draw(&self, frame: &RenderFrame, alpha: f32, gfx: &mut Gfx) -> Result<(), AppError>;
}

pub struct ClientApp<G: Game, P: Presenter> { /* private */ }
impl<G: Game, P: Presenter> ClientApp<G, P> {
    pub fn new(game: G, presenter: P, config: &AppConfig, profile: &DeviceProfile, settings: Settings) -> Result<Self, AppError>;
    pub fn app(&self) -> &App<G>;
    pub fn presenter(&self) -> &P;
    /// The first error raised inside a platform callback, if any (taken, leaving None).
    pub fn take_error(&mut self) -> Option<AppError>;
}
impl<G: Game, P: Presenter> PlatformApp for ClientApp<G, P> { ... }

/// Opens a window and runs the client until the window closes or an error occurs.
pub fn run_client<G: Game, P: Presenter>(title: &str, size: (u32, u32), client: &mut ClientApp<G, P>) -> Result<(), AppError>;
```
Behaviour of the `PlatformApp` impl (callbacks cannot return errors, so the first error is stored in the client, `input.quit = true` is set, and every later callback does nothing):
- `init`: build `Gfx::new(gpu, format, size, settings)`, call `presenter.setup`, create the `Extractor` and one `RenderFrame` pair with capacity = a new `AppConfig` field `max_entities: usize` (add it to `AppConfig`, default 4096, must be at least 1 else `AppError::ZeroEntities`; update the existing config tests).
- `resize`: forward to `Gfx::resize`.
- `frame(target, input, dt)`: (1) add `input.mouse_delta` (scaled by a `LookSensitivity(f32)` newtype, default 0.004 rad per pixel, per axis since D49, validated finite and > 0, constructor `LookSensitivity::new`) to the view angles with `ViewAngles::turned`, but only when `input.grabbed`; (2) `presenter.apply_input`; (3) convert `dt` to nanoseconds with checked arithmetic (error `AppError::ClockRange` when negative, non-finite or not representable) and call `App::advance`; (4) if at least one tick ran, `Extractor::extract` into the back `RenderFrame` and swap it with the front one (so front = newest, `previous` fields hold the prior tick); on the very first frame extract once so a frame exists; (5) `alpha` = `Advance::alpha`; (6) `gfx.begin_frame(presenter.camera(..))`, `presenter.draw(..)`, `gfx.render(target, dt)`.
- A pure helper `pub(crate) fn blended_transform(frame: &RenderFrame, id: NetId, alpha: f32) -> Result<Transform, AppError>` is exposed to presenters through a public method `RenderFrame::blended(&self, id: NetId, alpha: f32) -> Result<Option<Transform>, AppError>` (None when the id is not in the frame; binary search on the ascending ids, `get`, no indexing). Put it in `extract.rs` and test it.

**Docs.** `docs/ARCHITECTURE.md` §1 `app` row: add "client runner (single-threaded)"; `docs/README.md` §2 row likewise; add a short note to ARCHITECTURE §3 that the threaded mode is still to come.

## Tests (all four required)
1. Unit tests for `blend`, `LookSensitivity`, `RenderFrame::blended`, config validation.
2. `tests/client_logic.rs`: a fake `Presenter` (counts calls, records `alpha`) and a tiny test `Game` that moves one entity one metre per tick; drive `ClientApp` without a GPU by calling a `pub(crate)`-free path: factor the tick-and-extract part of `frame` into `ClientApp::simulate(&mut self, dt: f32, input: &Input) -> Result<f32 /* alpha */, AppError>` (public, documented) and test it: no tick for a tiny dt, one tick at exactly one step, catch-up cap respected, alpha in [0, 1], the extracted `current` is one metre ahead of `previous`, `blended(id, alpha)` lies between them.
3. `tests/headless_render.rs` (one `#[test]`): uses `platform::headless::run(&mut client, (256, 256), 3, Some(path))` where the presenter draws one lit cube from the test game and the screenshot path is inside `std::env::temp_dir()` under a name unique to the process; read the PNG back with the `png` crate (add `png.workspace = true` as a dev-dependency) and assert the centre pixel differs from a corner pixel (the cube is visible) and that no error is stored (`take_error()` is `None`). If no GPU adapter exists on the machine this test must fail with a clear error, not skip silently: that is a real finding the owner wants to see.
4. A no-allocation test is NOT required for `frame` (the renderer allocates through wgpu); do not claim one. Do add an assertion in test 2 that `simulate` after a warm-up call does not grow `RenderFrame::capacity()`.

## Do not
Touch other crates except the one platform blanket impl and its test. Add no `#[allow]`, `unwrap`, `expect`, `unwrap_or*`, `.ok()`, `let _ =`, panicking indexing or truncating `as` casts in non-test code. Use no threads, `Mutex` or `Condvar` in `client.rs`. Leave the threaded mode, audio, networking and any game-specific code out.

## Gates
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
