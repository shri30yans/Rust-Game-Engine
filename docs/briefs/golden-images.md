# golden-images: one screenshot test path that cannot record a broken frame (2026-11-12)

**What went wrong.** `world-pipeline-2` recorded three sample-arena goldens (`games/sandbox/game/tests/fixtures/sample-arena-{desktop,phone,lockdown}.png`). All three are the same pure-white 640×360 image, and the test passes by comparing white to white. Earlier, the black world at High was recorded the same way.

**Why it was possible.**
- Every golden test has its own copy of decoding, difference and recording code:
  - `games/imagine/game/tests/headless_render_golden.rs`;
  - `games/sandbox/game/tests/headless_render_golden.rs`;
  - `engine/ui/tests/{menu_golden,gallery}.rs`;
  - `engine/render/tests/*_headless.rs`.
- There are several recording switches (`IMAGINE_RECORD_MENU_GOLDEN`, `IMAGINE_RECORD_VIEWMODEL_GOLDEN`, `IMAGINE_RECORD_COMBAT_GOLDEN`, ...).
- Only one test (the IMAGINE floor-luminance check) judges the picture at all. Nothing shared exists that could refuse a blank image.

## How Unreal does it
Automation screenshot tests go through one framework: one comparison tool, one tolerance setting per test (`FAutomationScreenshotOptions`), one place where new reference shots are approved, and a report of the differences. ([Screenshot comparison tool](https://dev.epicgames.com/documentation/unreal-engine/screenshot-comparison-tool-in-unreal-engine))

## Design
1. **One golden module.**
   - Put `render::golden` in `engine-render`, behind a `golden` feature that only test targets enable, with the PNG decoder as an optional dependency of that feature.
   - It owns decode, difference (mean and maximum per channel), the tolerance type, recording, and the failure message, which names the file and the measured values and writes the actual image beside it.
   - Delete every per-test copy.
2. **One recording switch:** `GOLDEN_RECORD=<name-pattern>`. A test records only when its golden's name matches. Delete the per-test environment variables.
3. **Plausibility is part of every golden.** `Golden::new(name).expect(Plausible { .. })` checks the picture itself, before comparing and before recording:
   - **Not uniform, always.** The luminance standard deviation is above a floor, and fewer than 98% of pixels equal the most common value. A uniform or near-uniform image can never be recorded or pass.
   - **Optional per test:** a luminance range for a named region (the IMAGINE floor check moves onto this), and "this region is not the clear colour".
   - Recording runs the same checks and refuses a frame that fails them.
4. The white sample-arena goldens are on the `world-pipeline-2` branch, not on main. That branch is finished after this one and fixes them with this module (see its brief).
5. Port every golden test in the workspace to the module, with tolerances unchanged unless the report says why.

Tests:
- The module's own tests:
  - uniform images are rejected, both in recording and in comparison;
  - a one-pixel difference within tolerance passes;
  - the failure message names the file.
- Every existing golden passes through the module.

Runs now, beside `asset-source`, which also edits how the game tests build their clients. Change only the comparison and recording parts of those test files, so the reviewer can integrate both.

Read first:
- `AGENTS.md`;
- `docs/PRINCIPLES.md` §3 ("Pictures are judged, not only compared");
- every golden test file named above.

Docs in the same change:
- `docs/WORKFLOW.md` (how to record a golden);
- **D77** in `docs/README.md` §7;
- `docs/briefs/README.md` status `review`.

## Answer to the stop on 2026-11-12 (decision)

**The module is for images only.** `engine/ui/tests/menu_golden.rs` and `gallery.rs` compare draw-list text snapshots, not pictures. They are a different kind of test (a snapshot test, like `insta`) and stay as they are: do not port them, and do not add a text API.

- "Port every golden test" means every PNG golden: `games/imagine/game/tests/headless_render_golden.rs`, `games/sandbox/game/tests/headless_render_golden.rs`, and any `engine/render/tests` test that compares or records a PNG reference.
- `GOLDEN_RECORD` replaces the image-recording variables only.
- If an `engine/render` headless test only asserts properties and has no reference image, leave it.

**Small choices are yours.** For naming, plumbing or where a function lives, pick the simplest option that keeps one path, say it in the report, and continue. Stop only when the goal itself conflicts.

## Gates
scripts/gate.sh
