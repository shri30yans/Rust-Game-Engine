# ui-core: `ui` crate, slice 1 (theme tokens, screen layer stack, focus navigation)

Goal: a new engine crate `ui` (package `engine-ui`) holding the renderer-independent UI model: design tokens loaded from a theme file the game hands in, the screen layer stack, and spatial focus navigation. No drawing in this slice; widgets and draw-list output to `render` come next.

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §0 (platform rules), and as the first game that will use it: `games/imagine/docs/design/DESIGN_SYSTEM.md`, `MENUS_SPEC.md` (layer stack and navigation rules), `COMPONENTS.md`, `theme.ron`.

Platform rule (D24): `engine-ui` serves every game, so it knows no game's token names. Token names (`ink-900`, `signal`, …) are data from the game's theme file. The crate defines the *shape* of a theme file and resolves names to typed ids at load time; it never reads a file by a fixed path.

## Design

Layering: depends on `engine-core`, `serde`, `ron` (add `ron = "0.10"` to `[workspace.dependencies]`; if 0.10 is unavailable use the latest 0.x and note it), `thiserror`, `glam` (+ `proptest` dev). Workspace member `engine/ui` + `engine-ui = { path = "engine/ui" }`. `layers.json`: `{ "pattern": "engine/ui", "layer": 3 }`. It must not depend on `render`, `platform`, `world`, `physics` (the input it needs is its own `NavInput` enum).

Files:
- `engine/ui/src/lib.rs`: header + re-exports.
- `engine/ui/src/error.rs`: `UiError`: `Parse`, `UnknownToken { group, name }`, `InvalidColor(String)`, `ContrastViolation { fg, bg, ratio, min }`, `EmptyStack`, `LayerOrder`.
- `engine/ui/src/color.rs`: `pub struct Srgb8 { r, g, b }` parsed from `#RRGGBB`; `relative_luminance()` and `contrast_ratio(a, b)` per WCAG 2.x.
- `engine/ui/src/theme.rs`:
  - Serde mirror `ThemeFile` for the generic theme shape, with these sections:
    - `name`
    - `colors` (name → `#RRGGBB`)
    - `contrast_rules` (fg, bg, min ratio)
    - `scrim`
    - `fonts`
    - `type_scale`
    - `space`, `radius` and `sizes` (named lengths)
    - `glyphs` (named strings)
    - `motion`
    - optional `touch`

    Unknown sections are an error. IMAGINE's `theme.ron` already matches this shape, except that `notch` becomes `sizes` and `team_glyphs` becomes `glyphs`. Make that rename in `games/imagine/docs/design/theme.ron` and in the design docs that name those sections.
  - Validated `Theme` built only by `Theme::from_ron(text: &str, serial: ThemeSerial) -> Result<Theme, UiError>`.
  - Each token kind has a typed id newtype: `ColorId(u16)`, `TypeId`, `SpaceId`, `RadiusId`, `SizeId`, `GlyphId`, `MotionId`.
    - `Theme::color_id(&str) -> Result<ColorId, UiError>` (`UnknownToken` when missing) resolves a name once, at load.
    - Each id carries the serial of the theme that made it (a counter passed into `from_ron`, never a global). `Theme::color(ColorId) -> Result<Srgb8, UiError>` returns `UiError::ForeignId` for an id made by another theme. Add `ForeignId` to `UiError`.
  - A game can wrap ids in its own enum later; that lives in the game crate, not here.
  - `from_ron` checks every `contrast_rules` entry and fails with `ContrastViolation`.
- `engine/ui/src/stack.rs`:
  - `pub enum Layer { Game, GameMenu, Menu, Modal, Toast }` with total order as in MENUS_SPEC.
  - `pub struct ScreenId(u32)`.
  - `pub struct ScreenStack`: one stack of screens per layer. `push(Layer, ScreenId)`, `pop_top() -> Result<(Layer, ScreenId), UiError>` (Back pops the top screen of the highest non-empty layer, except `Toast`, which never takes focus), `focused() -> Option<(Layer, ScreenId)>` = top screen of the highest non-empty layer that accepts input (`Toast` does not), `clear(Layer)`.
- `engine/ui/src/focus.rs`:
  - `pub struct WidgetId(u32)`; `pub struct FocusRect { id: WidgetId, min: Vec2, max: Vec2 }`.
  - `pub enum NavInput { Up, Down, Left, Right, Next, Prev }`.
  - `pub struct FocusOverride { from: WidgetId, input: NavInput, to: WidgetId }` (explicit RON overrides, MENUS_SPEC rule 1).
  - `pub struct FocusNav` with pre-allocated scratch: `fn navigate(&mut self, current: WidgetId, input: NavInput, rects: &[FocusRect], overrides: &[FocusOverride]) -> Option<WidgetId>`. Rules: an override wins; otherwise pick the candidate whose centre lies in the input direction's 90° cone from the current centre, minimising `primary_distance + 2 × orthogonal_distance`; ties broken by lowest `WidgetId`. `Next`/`Prev` walk reading order (top-to-bottom, then left-to-right by centre, with a row tolerance of 4 logical px).

Tests (`engine/ui/tests/`):
- `theme.rs`: uses a small generic fixture, `engine/ui/tests/fixtures/theme.ron` (names like `text`, `surface`, `accent`; no game's names), and checks:
  - every name in the fixture resolves;
  - a known pair's contrast matches a hand-computed WCAG value ±0.05;
  - an edited copy with a failing contrast rule returns `ContrastViolation`;
  - an unknown section or name returns `UnknownToken` or a parse error;
  - a bad hex returns `InvalidColor`;
  - an id from one theme used on another returns `ForeignId`.

  The engine tests must not read anything under `games/`. IMAGINE's real theme gets its own load-and-contrast test in the `imagine` game crate (build step 2).
- `stack.rs`: Toast never focused; Back pops Modal before Menu; popping an empty stack errors; proptest: after any sequence of push/pop, `focused()` equals a reference model.
- `focus.rs`: a 3×3 grid navigates as expected in all directions; overrides win; no candidate → `None`; navigation never returns `current`; proptest: the result is always one of the rects; no allocation after warm-up (counting allocator).

Do not touch other crates' source. The only file outside `engine/ui` you change is IMAGINE's `theme.ron` (and the design docs naming its two renamed sections).

## Gates

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
Rules reminder (review greps for these and reads the code): no `unwrap`/`expect`/`panic!`/`#[allow]` in non-test code, no `unwrap_or*`/`.ok()`/`let _ =` that hide an error, no numeric `as` casts that can truncate, no silent fallbacks or invented default values, no `Rc<RefCell>`/`Arc<Mutex>`, no duplicate structs, errors that name what actually went wrong (one precise variant per cause, never a reused unrelated variant). Test files may use `unwrap`/`expect` with a module-level `#![allow(clippy::unwrap_used, clippy::expect_used)]`. Per-frame paths (`FocusNav::navigate`, `ScreenStack` queries) must not allocate after warm-up; ring or stack types must keep working after any number of operations.
