# ui-css-paint: the engine draws every visual CSS feature the IMAGINE mockups use, matching Chrome (2026-11-12)

The owner saw the menus on a phone emulator and asked why the UI looks primitive.

**Why it looks primitive.**
- The approved mockups (`games/imagine/docs/design/mockups/*.html` and `*.png`) use 61 `letter-spacing`, 34 `text-transform: uppercase`, 33 `box-shadow`, 23 `linear-gradient`, 17 `clip-path: polygon` (angled corners), 12 `border-radius`, 9 `radial-gradient`, 7 `mask`, `skewX`, SVG icons and the Barlow Condensed and Barlow fonts.
- `tools/ui-cook` accepts almost none of these, so the shipped screens (`games/imagine/ui/*.html`, 26 KB in all) were cut down to flat boxes and text the cooker could take.
- The design existed; the engine could not draw it.

**The fix of the cause.** The engine supports the CSS paint features the design system needs, and each one is proven to match Chrome. Designers and agents then author in a browser at full speed, and the engine reproduces it. This is the paint half of `ui-css-layout` (which does layout), and the same principle: one rendering truth, tested against the browser.

## Design

**1. Features, end to end** (cook, document, draw list, UI shader), only what the mockups use:
- `border-radius`, with up to four radii. Draw rounded boxes as SDF quads, with antialiased edges and borders, in the existing single UI pass.
- `box-shadow`: outer shadows with offset, blur and spread, plus `inset`. Use the analytic Gaussian rounded-box shadow (an SDF approximation). Allow several shadows per element.
- `clip-path: polygon(...)` with px, % and `calc(% - px)` points. Accept only convex polygons and reject the rest at cook (they are only used for angled corners). Draw them as an SDF of the convex polygon, so edges stay antialiased and children are clipped to them.
- `linear-gradient` (exists), `radial-gradient(ellipse at x y, stops)`, and `repeating-linear-gradient` (the PLAY button stripes).
- `mask: linear-gradient | radial-gradient`: an alpha mask multiplied over the element and its children. Implement it with the same gradient evaluator, not a second one.
- `transform`: `translate`, `skewX` and `scale`, in px and %, as one 2D affine per element that applies to its children. Hit testing uses the inverse transform, so touch regions stay exact (see `touch-from-layout`).
- `opacity`, inherited multiplicatively.
- Anything else in a mockup that is visual and not text or image. Text is `ui-fonts`; images and SVG icons are `ui-kit-2` images.

**2. One shader path.**
- Every box is one instanced quad carrying:
  - its shape (rect, rounded or convex polygon of up to 8 points);
  - its paint (solid or gradient id);
  - a shadow;
  - a clip-stack index and a mask.
- No per-feature pipelines and no extra render passes: the whole UI is still one pass, with a draw count independent of the feature mix. Report draw calls and the UI pass time for the home mockup on this Mac and on the emulator or phone.

**3. Chrome conformance by pixels.**
- Extend `ui-css-layout`'s browser harness so that each feature fixture HTML (`tools/ui-cook/tests/fixtures/paint/*.html`, one per feature with edge cases) renders in headless Chrome and in the engine headless at 1×.
- Compare them with a perceptual threshold: mean absolute error under 1.5 levels, and 99th-percentile error under 12 levels, ignoring text. Store the Chrome PNGs as fixtures so CI does not need Chrome; regenerate them with one command.
- A feature is done only when its fixture passes.

**4. Validation once, at cook.** Unknown or unsupported values are cook errors that name the file, the element and the property. No silent drops.

Done means:
- items 1 to 4 for every feature listed;
- the home mockup's panels, buttons, chips, angled corners, shadows, stripes and masks render through the engine with only text and images differing, shown side by side with Chrome in the report;
- the four gates pass;
- the APK builds.

Starts after `ui-css-layout` merges. Runs at the same time as `ui-fonts`: both edit `tools/ui-cook/src/css.rs`, so keep your changes there small. Do not merge other branches; the reviewer integrates overlapping briefs at merge.

Read first:
- `AGENTS.md`;
- `docs/research/2026-11-12-ui-engines.md`;
- `docs/briefs/ui-css-layout.md`;
- `games/imagine/docs/design/mockups/` (`tokens.css`, `home.html`, `hud-angled.html`, `results.html`);
- `tools/ui-cook/src/`;
- `engine/ui/src/` (`brush.rs`, `layout.rs`, `checks.rs`);
- `engine/render/src/ui.rs` and `engine/render/src/shaders/ui.wgsl`.

Do not touch the `engine/render` scene, the lighting and shadow passes (`render-high-black` is running there), `engine/netcode`, or `engine/physics`.

Docs in the same change:
- `docs/ARCHITECTURE.md` §1;
- `docs/research/2026-11-12-ui-engines.md` (the supported CSS table: features and limits);
- a decision in `docs/README.md` §7;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
