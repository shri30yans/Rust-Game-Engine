# ui-preview: edit a screen and see it at once, rendered by the engine, plus screenshots and reference comparison for agents (PLAN §6.3, D46, D53)

Goal: changing a screen takes seconds, not a compile.
- A person saves an HTML or CSS file and sees the result in the preview window within half a second, at phone and desktop sizes, with mock data, drawn by the engine's own renderer.
- An agent runs one command that writes PNG screenshots of any screen in any state and size, and a comparison against a reference image. It then looks at the images and fixes what differs.

Done means:
1. `tools/ui-preview` exists, with a window mode and a headless shot and compare mode.
2. Every IMAGINE screen has preview states.
3. The workflow is documented.
4. The four gates pass.

This tool changes `PLAN.md` §6.3: it renders with the engine, not in a web page, because only the engine shows what the game will draw.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D41, D46, D48, D53;
- `games/imagine/docs/PLAN.md` §6.3;
- `engine/ui/src/` (documents, layout, menus, bindings, hot reload);
- `tools/ui-cook/src/` (the cooker as a library);
- `engine/render` (UI pass, headless renderer);
- `engine/platform` (window);
- `games/imagine/ui/`;
- `games/imagine/game/src/{menus,hud}.rs` (which bindings each screen reads);
- the headless golden tests in `games/imagine/game/tests/`.

## Design

**1. One binary, built once.**
- `cargo run -p ui-preview --release -- <game> [screen]`.
- After the first build, editing screens needs no compile: the tool cooks HTML in-process with the `ui-cook` library on every save.
- It depends only on engine crates and tools, never on a game crate (D24). A game provides its screens through data alone:
  - `games/<game>/ui/*.html` and their assets;
  - `games/<game>/ui/preview/<screen>.ron`, the preview states (§3);
  - the game's theme and fonts, which the documents already reference.

**2. Window mode (for people).**
- It opens one window showing the selected screen in device frames. It has presets 2400×1080 phone (with a 90 px cutout), 2340×1080, 1600×720, 1920×1080 and 2560×1440, and shows one or two at a time. Keys cycle the frame, the screen and the state.
- It watches the screen's HTML, CSS, images and fonts. On save it re-cooks and redraws within 500 ms.
- A cook error shows as a red banner with file, line and message, and keeps the last good version (D41).
- **Overlays toggled by keys:**
  - layout boxes;
  - safe area;
  - text overflow (any text outside its box is outlined in red, using the engine-ui overflow check from `imagine-device-fixes`);
  - touch targets smaller than 48 dp, outlined.
- Input works: hover, press, focus, scroll and transitions play. Actions are logged to the console, not executed. Bindings come from the preview state.
- The background behind a screen is one of:
  - a flat colour;
  - a still image (for example a render of the menu showcase);
  - a cooked 3D scene drawn by the engine, if the brief that builds the showcase provides one.
  The state file chooses which.

**2a. Inspector (docs/research/2026-11-12-ui-engines.md U7).** A key toggles an inspector in the window. Hovering an element shows its box, its classes, its resolved style and its bound fields with live values. A small frame panel shows layout and paint time. The same overlay is available in desktop debug builds of a game, using the dev-tools layer from PLAN §6.4.

**3. Preview states.**
- `ui/preview/<screen>.ron` lists named states. Each sets the screen's view model fields (`ui-viewmodel`), deserialised into the model type, so a stale state is a load error: text, numbers, lists of rows for repeated templates, visibility, and focus or selection.
- Every screen has at least:
  - `default`;
  - `long_text`: the longest real names and labels, and a long localisation stand-in;
  - `empty`: no rows, nothing selected;
  - `full`: maximum rows, for example a lobby filled to capacity.
- A state that sets a binding the screen does not have, or misses one it needs, is an error naming the file and the binding. This check also runs as a test, so states cannot go stale.

**4. Headless shot and compare mode (for agents and CI).**
- `ui-preview <game> shot [--screen S] [--state T] [--size WxH] --out DIR` renders PNGs through the headless renderer. File names are `<screen>__<state>__<W>x<H>.png`. With no filters, it renders every screen, state and size.
- `ui-preview <game> compare --shot A.png --reference B.png --out DIR` writes three files:
  - `side_by_side.png`, the shot and the reference at the same height;
  - `diff.png`, a heat map;
  - `report.ron`, with a structural similarity score (SSIM on luminance) per quarter of the image, plus overall.
  The reference may be a different size; it is fitted by height. The score is a guide for a reviewer, not a pass or fail gate.
- `ui-preview <game> check` runs, for every screen, state and size, the overflow check, the touch-target check and the binding check. It exits non-zero on any failure. Add it to the IMAGINE test suite as a test that calls the same library function.

**5. The agent review loop. Write it in `docs/WORKFLOW.md`, and in `games/imagine/docs/design/README.md` for UI work.** For each screen:
1. Shoot it at 2400×1080 and 1920×1080 with `default` and `long_text`.
2. Compare it with its approved reference mockup in `games/imagine/docs/design/mockups/`.
3. Open `side_by_side.png` and `diff.png` with the image viewer.
4. Write a short review: what differs, what fails the design checklist (hierarchy, alignment to the 8 px grid, contrast, text fit, touch-target size, spacing rhythm, state colours).
5. Fix it, and repeat until the review finds nothing material.
6. Put the final shots and the review in the report.

The reviewer (Claude) reads the same PNGs.

**6. Tests.**
- Preview states load, and the binding check fails on a stale state.
- Shot output is deterministic: same bytes twice for a still screen.
- `compare`: identical images score 1.0, and a shifted image scores lower.
- The `check` command passes on every current IMAGINE screen, or the failures are listed and fixed in this brief if they are text overflow. Coordinate with `imagine-device-fixes`, which owns the overflow fix; if it has not merged, report the failures instead.
- A watch test: writing a changed HTML file triggers a re-cook and a new frame, measured under 500 ms.

**Do not touch:** `engine/netcode`, `engine/physics`, `engine/scene`, `kits/`. Change `engine/ui` and `tools/ui-cook` only to expose library entry points that are needed.

`imagine-device-fixes` is changing `engine/ui` (text fit, the overflow check) and `ui-kit-2` follows it. Merge main before you finish and use their APIs; do not duplicate them.

**Docs in the same change:**
- `games/imagine/docs/PLAN.md` §6.3 (engine-rendered preview);
- `docs/WORKFLOW.md` (the agent review loop);
- `games/imagine/docs/design/README.md`;
- `docs/ARCHITECTURE.md` §1 (a new tools row and `layers.json`);
- a new decision in `docs/README.md` §7 (the preview renders with the engine; preview states as checked data; agents review screenshots against references);
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
