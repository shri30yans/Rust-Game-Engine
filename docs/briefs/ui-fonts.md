# ui-fonts: real typography, with the design's faces, weights, tracking and case, matching Chrome (2026-11-12)

**Why the text looks primitive.**
- The engine draws every string in Roboto (`engine/render/fonts/`), at the weight the atlas happened to load.
- The design system uses Barlow Condensed for display type and buttons, Barlow for body text and JetBrains Mono for codes (`games/imagine/docs/design/mockups/tokens.css`), with uppercase labels and wide tracking.
- None of `font-family`, `font-weight`, `letter-spacing` or `text-transform` reaches the engine.

## Design
1. **Font assets.**
   - A game declares its faces in CSS with `@font-face` (family, weight, style, `src: url(...)` to a TTF or OTF in the game's content).
   - Commit the OFL faces the mockups use (Barlow Condensed 500, 600, 700, 800; Barlow 400, 500, 600; JetBrains Mono 500) with their licences under `games/imagine/content/fonts/` (D23: open licences only), and record them in `games/imagine/docs/ASSETS.md`.
   - The engine keeps a default face for engine-only text (the error overlay and telemetry). A game's text never falls back silently: a family that is not declared is a cook error naming the element.
2. **Cook.**
   - `font-family`, `font-weight`, `font-style`, `font-size`, `line-height`, `letter-spacing` (px and em), `text-transform` (`uppercase`, `none`) and `text-align` are resolved at cook into a typed `TextStyle { face: FaceId, size, line_height, tracking, case }`.
   - The cooker applies `uppercase` to static text. Bound text is transformed at bind time, through one function.
3. **Runtime.**
   - Each face and weight is a face in the SDF glyph atlas, with glyphs rasterised on demand with a budget, as today.
   - Measurement and drawing use the same shaper and the same tracking, with one measurer (`hud-fit` deletes the fake one).
   - Kerning stays on.
4. **Chrome conformance for text.**
   - For fixtures with each face, weight, size, tracking and case, the run widths are within 1 px of Chrome's (`getBoundingClientRect` on a span), baselines within 1 px, and the pixel comparison of glyph coverage is under a threshold you justify.
   - Store the Chrome results as fixtures, using the harness from `ui-css-layout`.
5. **Phones.** Atlas memory with all the faces loaded is reported for the home screen and the HUD, and must stay within the UI texture budget in `docs/RENDERING.md`.

Done means:
- items 1 to 5 complete end to end;
- the IMAGINE screens use the design's faces (only the CSS changes);
- the four gates pass;
- the APK builds.

Starts after `ui-css-layout` merges, and runs at the same time as `ui-css-paint`: keep your `css.rs` changes small. Do not merge other branches; the reviewer integrates overlapping briefs at merge.

Read first:
- `AGENTS.md`;
- `docs/research/2026-11-12-ui-engines.md` (U9 text shaping);
- `engine/render/src/ui.rs` and the font atlas module;
- `engine/ui/src/layout.rs` (`TextMeasure`);
- `tools/ui-cook/src/`;
- `games/imagine/docs/design/mockups/tokens.css`.

Do not touch the `engine/render` scene and lighting passes, `engine/netcode`, or `engine/physics`.

Docs in the same change:
- `docs/ARCHITECTURE.md` §1;
- `docs/research/2026-11-12-ui-engines.md` (the supported CSS table);
- a decision in `docs/README.md` §7;
- `games/imagine/docs/ASSETS.md`;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
