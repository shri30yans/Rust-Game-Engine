# touch-from-layout: touch controls are hit-tested against the drawn layout; nothing on screen can be "invalid" (phone crash, 2026-11-12)

**Status:** review

 On a realme RMX5085 (2800×1280, Mali-G720, Android 16), the app closed about two seconds after a match started. logcat:

```
E/imagine: platform error: invalid touch button: the centre must be finite and within 0..=1 on each axis
```

**Why that was possible: two design flaws.**
1. **A second, weaker copy of the layout.**
   - The HUD is laid out in pixels and drawn: `UiScreen::draw`, with real font measurement.
   - The touch controls come from a second layout of the same document: `UiScreen::touch_buttons`, with `NoFont`, so text has no size.
   - That copy is converted into `ButtonRegion { centre: fraction, radius: fraction }` and then into `TouchButton::new`, which rejects a centre outside `0..=1`.
   - A layout can legitimately place an element partly or wholly off screen (CSS position, transitions, content that overflows). So valid layout output was fed into a type that can refuse it at run time, and the two layouts can also disagree about where a button is.
2. **A presentation error escalates to a stop.**
   - `ClientApp::prepare_input` returns the error, and the frame is skipped.
   - The same layout fails again on every frame, and the 120-consecutive-failure rule then stops the client.

**Fix by design, following Unreal (Slate hit-tests the geometry it painted, and there is no second layout for input):**
- one layout per frame;
- input is tested against the regions that were drawn;
- presentation can never stop the client;
- "a control off screen" is caught by a test at every device size, never at run time.

Done means:
1. The fraction-based touch path is deleted.
2. HUD touch controls use the same drawn-widget hit testing as the menus.
3. The phone case has a test.
4. The four gates pass.
5. `scripts/build-android.sh imagine` builds.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 (the touch and HUD decisions, and D64 travel recovery);
- `engine/core/src/geometry.rs` (`Viewport`, clipped `ScreenRect`, and `TouchRegion`);
- `engine/ui/src/layout.rs` and `engine/app/src/{ui_present.rs, client.rs}` (drawn regions, `draw_ui`, and `prepare_input`);
- `engine/platform/src/input.rs` (`set_touch_regions`, drawn-region routing, and `drag_look`);
- `games/imagine/game/src/{present.rs, touch.rs, hud.rs}`;
- `games/imagine/ui/hud.html`;
- `games/imagine/game/tests/e2e/`.

## Design

**1. One layout, drawn regions are the touch areas.**
- `UiScreen::draw` already returns the drawn button regions. Make them pixel rectangles from the same layout pass that drew them: `TouchRegion { id, rect: ScreenRect, shape: Rect | Circle, drag_look }`.
  - A `ScreenRect` is made only by `Viewport::clip(rect) -> Option<ScreenRect>`, so it is always inside the screen.
  - A region wholly off screen yields `None` and is not touchable. That is not an error.
- `Input` keeps the regions from the last drawn frame, in a pre-sized buffer with no per-frame allocation, and resolves a touch-down to the topmost region containing it. Held touches keep their button, as today, and `drag_look` behaves as today.
- **Delete:**
  - `UiScreen::touch_buttons` and its `NoFont` layout;
  - `ButtonRegion`'s fraction fields;
  - `TouchButton::new` and its fraction validation;
  - `Presenter::fit_touch_buttons`, `Presenter::touch_buttons` and `Presenter::touch_layout_epoch`;
  - `ClientApp::buttons_fitted_to`;
  - `Input::set_touch_buttons`, if the new path replaces it.
  No shim and no second path.
- The menus and the HUD use one hit-testing path. If menu pointer hit testing and HUD touch hit testing can be one function, make them one.
- The first frame has no drawn regions yet, so touches on it hit nothing. Say so in the docs; it is one frame.

**2. Presentation never stops the client.**
- In `ClientApp`, classify the failure source. Errors from UI layout, binding or drawing are logged once per distinct message, shown in the banner, and that layer keeps its last good frame.
- The 120-consecutive-failure stop applies only to failures that cannot recover by themselves: GPU device lost after recreate fails, or simulation errors in standalone play. Write that rule in the module header and in a decision.
- **Test:** a presenter whose HUD layout fails on every frame for 300 frames. The client keeps running, the banner shows the error, and the menu still works.

**3. Off-screen controls are a test failure, at every device size.**
- Extend the engine-ui layout check from `imagine-device-fixes` (overflow): every interactive widget (button, touch control) is fully inside the safe area.
- Run it for the HUD, with the HUD states a match really produces:
  - match start;
  - full scoreboard rows with the longest roster names;
  - kill feed full;
  - every warning shown;
  - left fire on and off;
  - ADS on and off;
  - the pause gear.
- Sizes: 1920×1080, 2400×1080, 2340×1080, 2800×1280, 1600×720 and 640×360.
- **Find which control was off screen on the phone.** Reproduce with match-start HUD state at 2800×1280. Report the element, its computed rect, and why: which CSS rule and which state. Fix the HUD document at that cause.

**4. Tests.**
- The e2e suite gets a phone run at 2800×1280 with touch. It starts a match and plays 600 ticks with touches on fire, jump, reload, scope and crouch.
- Unit tests:
  - `Viewport::clip`, for inside, partly outside and wholly outside;
  - topmost-region hit testing;
  - a held touch keeps its button when the layout moves.
- The counting-allocator test still shows 0 bytes for steady HUD frames.

**Do not touch:** `engine/render`, `engine/netcode`, `engine/physics`, `engine/scene`, `kits/`.
- `ui-kit-2` is running at the same time and edits `engine/ui` (images, nine-slice, clipping, fonts). Keep `engine/ui` edits to the region type and the layout check.
- `device-tier` and `world-pipeline-2` edit `engine/app`. Keep `client.rs` edits to `prepare_input`, the failure classification and the removed fields.

**Docs in the same change:**
- `docs/ARCHITECTURE.md` §1 (ui, app and platform rows);
- a new decision in `docs/README.md` §7:
  - input hit-tests drawn geometry, with one layout per frame;
  - `ScreenRect` exists only clipped to the screen;
  - presentation errors never stop the client;
  - off-screen controls are caught by the layout check;
- `games/imagine/docs/README.md` (touch);
- `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
scripts/build-android.sh imagine
