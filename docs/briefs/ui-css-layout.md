# ui-css-layout: a cooked screen lays out exactly as the browser shows it (2026-11-12)

The owner saw the IMAGINE HUD and asked why the health bar is not visible.

**What went wrong.**
- `games/imagine/ui/hud.html` puts the health `<progress class="bar">` inside `<div class="wrap">`. Neither element sets a width.
- In a browser a `div` is `display: block`, so the bar stretches to the full track width.
- `tools/ui-cook` (`look()` in `html.rs`) gives every element `Direction::Row`, and `engine/ui/src/layout.rs` builds every taffy node as `Display::Flex`. So the bar is an auto-width flex item with no content, and its width is 0.
- The track draws and the bar does not.

**Why it was possible.** There are two layout engines with different defaults. Screens are written in HTML and CSS and viewed in a browser, but the engine applies its own rules: every element is a flex row. Nothing compares the two, so a screen that looks right in the browser can be wrong in the game, silently.

## How other engines prevent this
- **Unreal (UMG) and Unity (UI Builder).** The designer is the runtime layout, so there is one engine.
- **Unity UI Toolkit.** USS is a documented CSS subset, and its differences are stated, for example that items stack vertically by default. ([USS properties](https://docs.unity3d.com/Manual/UIE-USS-SupportedProperties.html))
- **Bevy UI.** It uses taffy, which implements the CSS Block, Flexbox and Grid algorithms to spec. We already use taffy 0.15, which has `Display::Block`. ([taffy](https://docs.rs/taffy))

## Design
1. **CSS's own defaults, through taffy's spec algorithms.**
   - `LayoutDef` gains `display: Display::{Block, Flex}`.
   - The cooker maps `display` exactly as CSS does: a `div`, `body` or `progress` with no `display` is `Block`, and `display: flex` is `Flex`.
   - `flex-direction` defaults to `row`, as CSS says, and is valid only on flex containers. On a block it is a cook error naming the element.
   - `engine/ui/src/layout.rs` builds taffy nodes with the matching `taffy::Display`.
   - Text elements (`p`, `span`) keep their current behaviour; say what that is in the report.
   - Bump `FORMAT_VERSION` and re-cook every `.ui.ron` with `tools/ui-cook`.
2. **Browser conformance test, which is the guard for this class.**
   - Lay out every cooked screen at three sizes: 1280×720, 1920×1080 and 2800×1280.
   - Compare each node's engine rectangle with the rectangle a real browser reports for the same HTML (`getBoundingClientRect`), within 1 px.
   - Use a headless Chromium driven from a tool (`tools/ui-conform`, for example with the `headless_chrome` crate, or a small script the cooker calls). Search for the most dependable way and cite it.
   - Text width may differ because of fonts. Compare text boxes on position and height only, and say so.
   - If no browser is installed the test is skipped with a clear message. CI on this machine must run it.
3. **Visible content never has zero size.** Add a layout check: a visible bar, text or button with zero width or height is an error. Run it on every screen in the conformance test and in the IMAGINE HUD and menu tests.
4. **Fix the screens** the new defaults change, so all of them pass the conformance test. Show the HUD health bar full width, at 100/100 and at 25/100, in the report.

Done means:
- items 1 to 4 complete end to end;
- the four gates pass;
- the APK builds.

Starts after `haiku/ui-kit-2` step 3a is merged, because it splits `tools/ui-cook/src/html.rs` into a folder.

Read first:
- `AGENTS.md`;
- `docs/research/2026-11-12-ui-engines.md`;
- `tools/ui-cook/src/` (`html/`, `css.rs`);
- `engine/ui/src/doc.rs` and `engine/ui/src/layout.rs`;
- `games/imagine/ui/*.html`.

Do not touch `engine/render`, `engine/netcode`, or `engine/physics`.

Docs in the same change:
- `docs/ARCHITECTURE.md` §1 (ui and ui-cook rows);
- a decision in `docs/README.md` §7 (CSS defaults via taffy; browser conformance);
- `docs/briefs/README.md` status `review`.

## Answers to the stop on 2026-11-12 (Kiro asked; these are decisions)

1. **One user-agent stylesheet, shared by both sides.**
   - Write `tools/ui-cook/ua.css`. It is the subset's definition of browser defaults:
     - `*{margin:0;padding:0;box-sizing:border-box}`;
     - `html,body{width:100%;height:100%}`;
     - `p,span,progress{display:block}`;
     - `button{display:block;border:0;background:none;font:inherit;color:inherit;text-align:inherit}`.
   - The cooker applies it as the lowest-priority rules, and the browser harness injects the same file into every page. One file, so the two sides cannot drift.
   - The subset uses border-box sizing.
   - Add to it only what the conformance tests need, with a one-line comment per rule.
2. **No inline formatting.** An element holds either child elements or one text run, never both. Mixed content is a cook error naming the element.
   - Text elements are block-level boxes, so `p` and `span` compare exactly.
   - `input` and `select`: see the third answers below (they stay, as sized replaced elements).
3. **Bindings and repeated rows.** The harness renders each IMAGINE screen with a sample binding file, `games/imagine/ui/<screen>.sample.ron`, using the same values on both sides:
   - for the browser, the harness substitutes `{key}` with the sample string and repeats each template element by its sample row count;
   - for the engine, it binds the same values.
   - Feature fixtures in `tools/ui-cook/tests/fixtures/layout/` are static HTML with no bindings.
4. **Node identity and rectangles.** The cooker records each node's source path (for example `body>div.hp>div.track>progress.bar:1`) in the document as `Node::source`.
   - Layout errors and overflow errors name nodes by this path, so the field is read in production.
   - `UiLayout::node_rects()` returns every node's rectangle with its source path, including unpainted containers.
   - The harness collects the same paths from the DOM with a small script.
   - Chrome is installed at `/Applications/Google Chrome.app`; drive it headless (`--headless=new`, plus a dump script or the DevTools protocol, whichever is simplest and dependable).

## Answers to the second stop on 2026-11-12 (decisions)

1. **Units: the browser renders at the engine's logical scale, exactly as a phone browser does with its device pixel ratio.**
   - The engine's `px` is a logical pixel equal to `short_side / 720` physical pixels.
   - The harness sets Chrome's device scale factor to `short_side / 720` and its viewport to `physical / dsf` CSS px. At 1920×1080 that is a DSF of 1.5 and a 1280×720 viewport; at 2800×1280 it is a DSF of 1.7778 and a 1575×720 viewport. Use the DevTools protocol, `Emulation.setDeviceMetricsOverride`.
   - Then CSS `px` equals engine `px`, while `%` and `vmin` are fractions of the same screen on both sides.
   - Compare rectangles in physical pixels: the CSS rect times the DSF against the engine rect.
   - Record this rule in the decision. It is the single meaning of `px` for every game's UI.
2. **Repeated rows.** The engine's `Content::Repeat` node is structural and has no box in the browser. Leave Repeat nodes out of the comparison and compare their expanded rows. The harness expands `<template>` into sibling elements with the same source paths the cooker gives the rows (for example `...>template:0[3]` for row 3).
3. **The harness may instrument the page:**
   - inject `ua.css`;
   - set the device metrics;
   - substitute sample bindings and expand templates;
   - add one script that collects `getBoundingClientRect()` for every element with its source path.
   It must not change the page's own CSS.

## Answers to the third stop on 2026-11-12 (decisions; they replace the earlier line about `input` and `select`)

**Form controls stay, as the engine's native widgets with an explicit box.** The earlier answer was wrong: `settings.html` uses `<input type="checkbox">`, `<input type="range">` and `<select>` for bound widgets (`Content::Toggle`, `Slider`, `Choice` in `tools/ui-cook/src/html/widgets.rs`). Keep those mappings. Do not invent replacement markup.

This follows Unreal and the CSS spec:
- In UMG, CheckBox, Slider and ComboBox are native widgets that the layout sizes like any box.
- In CSS they are *replaced elements*: the browser draws their inside, and the layout only knows their box.

The rules:
- **The box is defined by the author, never by a browser default.** A control must set both `width` and `height` in the document's CSS. A missing size is a cook error naming the element. Then its box is identical in the browser and the engine.
- **`ua.css` neutralises the browser's look of the control:** `input,select{appearance:none;display:block;margin:0;padding:0;border:0;font:inherit}`, with a one-line comment.
- The conformance test compares the control's box like any element. It does not compare what is drawn inside it: the engine draws its own toggle, track and value.
- No other `input` types: `text`, `number` and the rest stay a cook error until a brief adds them.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
