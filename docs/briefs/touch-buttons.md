# touch-buttons: visible fire, jump and reload buttons on touch

Problem (owner, playing the APK): there is no visible shoot button. Today fire is a hidden second finger on the right half, jump and reload are hidden quick taps. Replace those hidden gestures with on-screen buttons. One canonical path: the hidden gestures are removed, not kept beside the buttons.

Read first: `AGENTS.md`, `engine/platform/src/input.rs` (touch code and its tests), `games/imagine/game/src/{present,hud}.rs`, `games/sandbox/game/src/present.rs`, `engine/render/src/ui.rs`.

## Design
**`engine-platform` `Input`** gains a touch-button layout, set by the game once:
```rust
/// Identifies a touch button. The game chooses the numbers; the engine never names a game.
pub struct TouchButtonId(pub u8);
pub struct TouchButton { pub id: TouchButtonId, centre: Vec2 /* fractions of the screen, 0..1 */, radius: f32 /* fraction of the SHORT side */ }
impl TouchButton { pub fn new(id, centre: Vec2, radius: f32) -> Result<Self, PlatformError>; } // finite, centre in 0..=1, radius in (0, 0.5]
impl Input {
    pub fn set_touch_buttons(&mut self, buttons: &[TouchButton]) -> Result<(), PlatformError>; // at most MAX_TOUCH_BUTTONS = 8, unique ids; pre-sized storage, no allocation per frame
    pub fn touch_buttons(&self) -> &[TouchButton];
    pub fn touch_button_held(&self, id: TouchButtonId) -> bool;     // finger currently down on it
    pub fn touch_button_pressed(&self, id: TouchButtonId) -> bool;  // went down this frame; cleared by end_frame
    pub fn touch_button_rect... (pixel centre and radius for drawing: `fn touch_button_px(&self, id) -> Option<(Vec2, f32)>`)
}
```
Routing in `touch_start`: a touch that lands inside a button's circle (pixel centre = fraction × screen size; radius = fraction × min(width, height)) is captured by that button and never becomes stick or look; it releases in `touch_end`. Different buttons can be held at once, and a look finger plus a button finger work together. Everything else behaves as now. REMOVE `fire_touch`, the second-finger fire, `tap_watch`, `jump_tap`, `reload_tap` and their constants and tests; `Input::fire()` becomes the keyboard fire key only.

**IMAGINE** (`games/imagine/game`): define `FIRE`, `JUMP`, `RELOAD` ids; layout for landscape: Fire = large circle bottom right (centre ~ (0.88, 0.72), radius 0.14), Jump = (0.74, 0.86) radius 0.09, Reload = (0.93, 0.42) radius 0.07 (check no overlap and that the right-half look area remains large; unit test the layout is valid and circles do not overlap). `present.rs` reads `touch_button_held(FIRE)` for fire, `touch_button_pressed(JUMP)` for jump, `touch_button_pressed(RELOAD)` for reload, besides the keyboard/mouse paths. `hud.rs` draws the buttons from `input.touch_buttons()` (ring + label "FIRE", "JUMP", "R"; brighter while held) ONLY when the device has touch input: pass a `show_touch_buttons: bool` from the presenter, true on Android and false on desktop (read how the game already distinguishes platforms; desktop must not draw them).
**Sandbox**: the same: a `JUMP` button replaces `input.fire()` as jump on touch; draw it with the sandbox's existing presenter text/ui if it has one, else document that the sandbox draws no HUD and add the minimal ring drawing via `Ui` (small, in its present.rs).

## Tests
1. `input.rs` unit tests: a touch inside a button is held and does not move the stick or look; outside still does; two buttons at once; button plus look finger together; `pressed` is an edge cleared by `end_frame`; releasing clears held; invalid buttons (radius 0, NaN, duplicate id, more than 8) are errors; proptest: arbitrary touch sequences never panic and never leave a button held after all touches end.
2. IMAGINE: layout test (valid, no overlap, all inside the screen at 16:9 and 20:9); a gameplay-level test that a simulated touch on the FIRE circle yields a fire command and a touch on empty right half does not.
3. The Android build script test still passes; rebuild both APKs with `scripts/build-android.sh imagine` and `scripts/build-android.sh sandbox` and report sizes and `apksigner verify` output.

## Do not
No `#[allow]`, `unwrap`, `expect`, `unwrap_or*`, `.ok()`, `let _ =`, panicking indexing, truncating `as` casts in non-test code. Do not touch `engine/render` or `engine/app/src/client.rs` (another task is changing them). Do not claim it was tested on a phone.

Docs: `docs/briefs/README.md` row to `review`; `games/imagine/docs/README.md` touch controls paragraph (replace the gesture text, and the "Not done" line stays honest); ARCHITECTURE §1 platform row if it lists touch; the owning doc: one short decision (next free number, D43 is taken by render-governor, use D44) recording buttons replacing hidden gestures and why.

## Gates
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
