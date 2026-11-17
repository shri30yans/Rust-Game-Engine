# ui-menus-engine: interactive menu screens in the engine: widgets, lists, a screen stack with focus, input and transitions (PLAN §4, D9, D10, D46)

Goal: any game can build full menus (home, mode select, lobby, loadout, settings, results) as UI documents with no layout or input code of its own. Done means: (1) documents can hold focusable widgets (button, toggle, slider, choice, tabs) and repeated lists bound to game data; (2) an engine `Menus` object runs the D9 layer stack: routes mouse, touch, keyboard and gamepad to the top focusable screen, moves focus spatially, handles Back, plays enter/exit transitions, and reports typed `UiEvent`s to the game; (3) `ui-cook` compiles the HTML subset for the new widgets; (4) a steady menu frame allocates nothing; the four gates pass.

Read first: `AGENTS.md`; `docs/README.md` §7 D9, D10, D11, D46, D48, D55; `games/imagine/docs/design/{MENUS_SPEC,COMPONENTS,DESIGN_SYSTEM,MOTION_SOUND,ACCESSIBILITY,TOUCH_SPEC}.md` (what the widgets must do and look like, generically); `engine/ui/src/*` (document, retained layout, bindings, `stack.rs`, `focus.rs` already exist but are not wired); `engine/app/src/ui_present.rs`; `tools/ui-cook/src/*`.

## Design

Follow Unreal's **CommonUI** (D55): a stack of *activatable* screens per layer, input routed only to the top active screen, a Back action that deactivates it, focus restored to the last focused widget when a screen becomes top again, and widgets that report actions instead of running game code. Adapted to our documents (D46): the game never touches layout.

**1. Widgets in the document format (`engine/ui`).** New `Content` kinds, each focusable, each with a document-unique `id` string:
- `Action { id, label: Template }` (a button that emits `UiEvent::Action(id)`),
- `Toggle { id, label, value: flag binding }`,
- `Slider { id, label, value: number binding, min, max, step }`,
- `Choice { id, label, options: list binding, selected: number binding }` (left/right cycles),
- `Tabs { id, tabs: list binding, selected: number binding }` (LB/RB and Q/E switch).
Widget state (hovered, focused, pressed, disabled via a flag binding) is visible to styling through state styles in `StyleDef` (`hover`, `focus`, `pressed`, `disabled` overrides); the focus ring follows MENUS_SPEC rule 2 from the theme.
- **Lists:** `Repeat { source: list binding, max: u16, template: Node }` instantiates the template per list item; item fields are read as `{item.name}` inside the template. Capacity is `max` (validated, pre-sized), so no allocation when the list changes length within it. Bindings gain typed lists of records (`ListBinding` with named fields).

**2. `Menus` (`engine/ui/src/menus.rs`), the runtime.**
- Holds one retained `UiLayout` per open screen, the existing `ScreenStack` (layers `Game < GameMenu < Menu < Modal < Toast`), and per-screen remembered focus.
- `push(layer, screen_document)`, `pop(layer)`, `replace(layer, ...)`; `frame(input: &MenuInput, bindings, screen_size, now) -> &[UiEvent]` and `draw_list()`. `MenuInput` is an engine type (pointer position and buttons, touches, navigation directions, confirm, back, tab left/right, scroll) so `engine/ui` stays free of platform types; `engine-app` maps its `Input` to it.
- Focus moves with `focus.rs`'s spatial search, wraps per screen, starts on the screen's `default_focus` id (the primary action, MENUS_SPEC rule 1), and is restored on return.
- Pointer and touch hit-test the laid-out widget rects; touch shows pressed state instead of focus (rule 2).
- Back pops the top screen of the top focusable layer, unless the screen declares `back: Action(id)` (for "Quit game?" confirms), which emits that action instead.
- `UiEvent::{Action(WidgetId), Toggled(WidgetId, bool), Changed(WidgetId, f32), Selected(WidgetId, u16), Back(ScreenId)}`, with ids resolved to typed handles at load so the game matches on handles it looked up once, not strings per frame.
- **Transitions:** enter and exit as opacity plus a short offset, timings from the theme's motion tokens; a screen being removed keeps drawing until its exit finishes; input goes only to the new top screen at once.

**3. `ui-cook`.** HTML subset: `<button data-action>`, `<input type="checkbox" data-bind>`, `<input type="range" min max step data-bind>`, `<select data-bind data-options>`, a tab strip (`role="tablist"`), and `<template data-repeat="list" data-max="n">`. Unknown attributes and elements are refused as today.

**4. `engine-app`** (new file `src/menus_present.rs` and one export line in `lib.rs` only; `client.rs` is being edited by another run): maps `platform::Input` (mouse, touch, keys, gamepad via the existing input layer) to `MenuInput`, and draws `Menus` through the existing `UiScreen` path.

**Tests:**
- Document validation for every widget (unique ids, ranges, `max` bound, bad bindings → typed errors).
- Focus: spatial moves on fixture layouts, wrap, restore on return, default focus.
- Input routing: only the top focusable screen receives events; toasts never take focus; Back behaviour with and without a `back` action.
- Pointer and touch hit tests including skewed ancestors (touch areas match drawing, D48).
- `Repeat`: list growth and shrink within `max` without allocation; a list longer than `max` is an error.
- Transition timing: a popped screen draws until its exit ends and receives no input.
- Counting allocator: 1,000 menu frames with hover, focus moves and a changing list allocate 0 bytes after warm-up.
- `ui-cook` round trips for each new HTML element; golden draw lists for a fixture menu (home-like screen with tabs, a list of cards and a settings form).

**Do not touch:** `engine/render`, `engine/netcode`, `engine/physics`, `engine/assets`, `kits/`, `games/`, `engine/app/src/client.rs`. IMAGINE's screens are the next brief.

**Docs in the same change:** `docs/ARCHITECTURE.md` §1 ui row; `docs/README.md` §7 D46 (new widget kinds) and D9 (the runtime that implements it); `docs/briefs/README.md` status `review`.

## Decisions after the first design review (2026-11-12)
1. **Touch:** `platform::Input::touches()` lists this frame's raw touch points (`TouchPoint { id, pos, phase: Began|Moved|Ended }`). The list is pre-sized to 16 and cleared each frame. The existing stick, look and button routing is unchanged.
2. **Gamepad** is not built yet, so it is out of scope. `MenuInput` is device-neutral (pointer, touches, nav, confirm, back, tab prev/next, scroll) and is filled from mouse, touch and keyboard. A later gamepad brief fills the same type.
3. **Screen fields:** `Document.screen: Option<ScreenDef { default_focus, back: BackRule::{Pop, Action(id)} }>`, validated against the document's widgets. In HTML: `<body data-default-focus data-back>`.
4. **State styles:** `StyleDef.states` holds `hover`, `focus`, `pressed` and `disabled` `StyleDelta`s, which override fill, border, opacity, text colour and scale. Precedence is disabled > pressed > focus > hover, and states are allowed only on widgets. Touch shows pressed and never focus. In CSS: `:hover`, `:focus-visible`, `:active`, `:disabled`.
5. **Motion:** `MenuMotion { screen, modal, exit_ms, slide_px }` is built from theme tokens; IMAGINE uses `m-screen`, `m-modal`, 120 ms and 24 px. A cubic-bezier evaluator lives in `engine/ui`. `set_reduce_motion` turns every transition into a 120 ms fade. Input goes to the new top screen at once.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
