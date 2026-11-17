# ui-actions: screens declare their input mode and actions; device-aware button prompts (docs/research/2026-11-12-ui-engines.md U2, U3; CommonUI)

Today each screen decides by hand whether game input passes through, whether the cursor shows, and how Back works, and button prompts are fixed text. CommonUI solves both:
- activatable widgets with an input config, restored when they pop;
- actions shown in an action bar with the glyph for the device in use.

Done means:
1. A screen's input mode and actions are document data, applied and restored by the screen stack.
2. An engine action bar shows the right glyphs for touch, gamepad (Xbox, PlayStation, Switch layouts) and keyboard and mouse, switching the moment the device changes.
3. IMAGINE uses it.
4. The four gates pass.

Starts after `ui-viewmodel` merges (both edit `engine/ui`).

Read first:
- `AGENTS.md`;
- `docs/research/2026-11-12-ui-engines.md`;
- `engine/ui/src/{stack,menus,focus}.rs`;
- `engine/platform/src/input.rs`;
- `games/imagine/game/src/{menus,present}.rs`;
- the CommonUI input guide: https://dev.epicgames.com/documentation/unreal-engine/commonui-input-technical-guide-for-unreal-engine

## Design
**1. Input mode as data.**
- Each screen definition in the document has `input: game | menu | game_and_menu`, plus `cursor: shown | hidden` and `capture: on | off`.
- The stack applies the top input-taking screen's mode to `Input` (`want_grab`, cursor) when that screen becomes top, and restores the previous one when it pops.
- Games stop setting these by hand: delete the per-screen code in IMAGINE.

**2. Actions.**
- A screen declares actions: an id (`back`, `confirm`, `tab_prev`, `tab_next`, or game-defined), a label token, and which input chords trigger them.
- The engine maps chords per device family from one data file in the engine: a default map that a game may extend.
- Back is an action, so `BackRule` becomes the `back` action's behaviour.

**3. Last-used device.**
- `engine-platform` tracks the last input device family (touch, gamepad layout, keyboard and mouse) from real input events, with a short hysteresis so stray mouse moves do not flip it.

**4. Action bar widget.**
- `<action-bar>` in a document lists the top screen's actions with glyphs for the current device. Glyph images are cooked through the `ui-kit-2` atlas from an open-licence glyph set, with the licence committed.
- On touch, actions with an on-screen button show nothing in the bar.

## Tests
- push and pop restore input modes across three layers;
- a device change switches the glyphs on the next frame;
- the chord maps resolve for each family;
- Back on every IMAGINE screen goes where MENUS_SPEC says;
- the goldens with the action bar, at phone and desktop sizes.

**Docs:**
- `docs/ARCHITECTURE.md` §1;
- a decision in `docs/README.md` §7 (input modes as screen data, actions and device glyphs);
- `docs/research/2026-11-12-ui-engines.md` U2 and U3;
- `games/imagine/docs/design/MENUS_SPEC.md`;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
