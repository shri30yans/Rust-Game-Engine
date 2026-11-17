# ui-kit-2: images, icons, nine-slice panels, gradients, clipping and scroll, font faces, safe areas (audit M6, M7; D46, D48)

Status: review. Step 1 (cleanup) by Haiku removed clipping and scroll, font faces, safe areas and the unused atlas packer; those are in [ui-kit-3](ui-kit-3.md). Step 2 done: CSS linear gradients cook and draw (render mode 5), and the placeholder atlas, image mode and nine-slice are removed. Step 3a done (groundwork, no behaviour change): the Basis encoder is the shared `tools/texture-encode` crate, and the `ui-cook` html and `engine-app` asset uploader are split into folders. Step 3b (cook side of images, icons, nine-slice) is next. Still open: resvg/KTX2 CLI, runtime atlas upload.

Goal: the engine UI can express a professional game front end. Today a document can only draw flat boxes and one font, so menus look like wireframes. Done means:
1. A UI document can show images and icons.
2. It can draw nine-slice panels and buttons, and gradient fills.
3. It can clip and scroll.
4. It can use several font faces and weights, with letter spacing, uppercase and text shadow.
5. It respects the phone's safe area.
6. Images work inside repeated rows.
7. The four gates pass.
8. A gallery document shows every feature in goldens at phone and desktop sizes.

Starts after `imagine-device-fixes` merges: both edit `engine/ui` and `tools/ui-cook`. That brief adds text measurement, fit and the overflow check.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D46, D48, D55;
- `docs/research/2026-11-11-engine-audit.md` M6, M7;
- `engine/ui/src/` (all of it);
- `tools/ui-cook/src/`;
- the UI pass in `engine/render` (`shaders/ui.wgsl` and its pipeline);
- `engine/assets` (cooked textures);
- `games/imagine/docs/design/`.

Follow Unreal's UMG and CommonUI model:
- a brush is an image plus how it is drawn (box, border or image);
- a widget style is data;
- the UI renders in one pass with batched quads from a texture atlas.

## Design

**1. Images and icons, cooked.**
- `tools/ui-cook` collects every image a document references (`<img src>`, `background-image: url(...)`, `border-image` sources) and packs them into one atlas per document set.
  - PNG is used as is.
  - SVG icons are rasterised at cook time (the `resvg` crate) at the largest size the document uses times the device scale, with mips.
  - The atlas is written as a cooked texture through `engine-assets`. The cooked document refers to atlas regions by id, never by path.
- At run time the UI pass binds the atlas and draws image quads in the same batch as boxes and text.
- `object-fit: contain | cover` for images.
- An image is tinted by `color`, so one white icon serves every state.

**2. Nine-slice and gradients.**
- `border-image: url(x) <slice> fill` (the CSS subset: four slice insets plus fill) draws a panel or button frame that scales without stretching its corners.
- `linear-gradient(<angle>, <stop>, …)` with 2 to 4 stops, as a background. It is drawn in the UI shader from the stop list, with no texture.
- `clip-path: polygon()` is out of scope. Angled panels use nine-slice images, which is how the HUD's angled style becomes cheap and crisp.

**3. Clipping and scroll.**
- `overflow: hidden` clips children to the node's box, using a scissor rectangle stack in the UI pass. Rounded clipping is out of scope.
- `overflow-y: scroll` (and `-x`) makes a scroll view:
  - touch drag with momentum and an overscroll bounce;
  - mouse wheel;
  - gamepad and keyboard focus moving into a hidden row scrolls it into view.
- Scroll offset is node state, kept across reloads by node id.
- Layout runs only when something changes, as now. Scrolling changes the draw offset, not the layout.

**4. Fonts.**
- A document set declares font faces: `@font-face` with family, weight and a source. The game ships its fonts (OFL or similar, with the licence file committed).
- Glyphs from every face share the SDF atlas, with sizes picked per face.
- Text properties:
  - `font-family`, `font-weight` (choose the nearest face);
  - `letter-spacing`;
  - `text-transform: uppercase`;
  - `text-shadow` (one shadow: offset, blur radius up to 4 px, colour), drawn from the SDF.
- Text shaping (ligatures, complex scripts) stays out of scope (M6). Say so in RENDERING or ARCHITECTURE.

**5. Safe areas.**
- `engine-platform` reports the screen's safe-area insets: the display cutout and gesture bar on Android, the notch on iOS, zero on desktop.
- Documents use `env(safe-area-inset-left)` and its siblings in padding and positions.
- A layer's root defaults to padding inside the safe area unless it says `full-bleed`.

**6. Images in repeated rows.** A repeated template row can bind an image region by id (for example, the mode thumbnail per row). This was out of scope in ui-menus-engine.

**7. Performance.**
- One draw call per atlas page per layer.
- The steady HUD and menu frames allocate nothing, using the existing counting-allocator tests extended to an image-heavy document.
- The steady menu frame costs at most 0.2 ms CPU on the M5. Report the number.

## Tests
- ui-cook:
  - atlas packing is deterministic (same bytes twice);
  - a missing image is a cook error naming the document and the attribute;
  - SVG is rasterised at the right size.
- Layout and draw:
  - nine-slice corners keep their pixel size at three box sizes;
  - gradient stops land at their positions;
  - clipping removes children outside the box;
  - a scroll view moves content and scrolls focus into view;
  - momentum stops.
- Fonts: the nearest-weight choice; letter spacing changes the measured width; uppercase.
- Safe area: a 2400×1080 screen with a 90 px left cutout keeps content inside it.
- Gallery goldens (`engine/ui/tests/gallery/`), one document using every feature, at 2400×1080 and 1920×1080.
- The counting-allocator test for an image-heavy menu frame.

**Do not touch:** `engine/netcode`, `engine/physics`, `engine/scene`, `kits/`. Change `games/` only to re-cook documents if the cooked format changes.

**Docs in the same change:**
- `docs/ARCHITECTURE.md` §1 (ui, ui-cook, render and platform rows);
- `docs/research/2026-11-11-engine-audit.md` M6 and M7 status;
- a new decision in `docs/README.md` §7 (cooked atlas and region ids, nine-slice for angled panels, scissor clipping, font faces, safe areas);
- the supported HTML and CSS subset list in the ui-cook docs;
- `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
