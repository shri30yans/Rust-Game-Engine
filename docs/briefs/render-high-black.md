# render-high-black: render features as a checked frame plan; the world draws lit at every tier (2026-11-12)

The owner saw the IMAGINE combat frame and asked why the arena is pure black.

**What I measured.** I rendered the same IMAGINE match frame headless (`games/imagine/game/tests/headless_render_golden.rs`, `match_client`) at 1280×720.

| Settings | World | Sky and viewmodel |
|---|---|---|
| `RenderTier::Low`, TAA off | correct: grey floor, orange crates, tan building, sun shadows | correct |
| `RenderTier::High`, TAA on | black | correct |
| `RenderTier::High`, TAA off | black | correct |
| High with `gtao: false` | black | correct |
| High with `auto_exposure: false` | black | correct |
| High with `depth_prepass: false` (GTAO still on) | the whole frame is black, sky included | (UI only) |

So the fault is in the High path around the depth prepass and what reads it. It is not GTAO alone, and not exposure. The pistol is drawn by its own pass and stays lit.

**Why it was possible (three design flaws).**
1. **Features are independent booleans.** `RenderFeatures { depth_prepass, gtao, auto_exposure, taa }` can hold combinations that cannot run, such as GTAO without the prepass. `Settings::check` only covers TAA, and nothing checks at build time that every pass's inputs were produced.
2. **A golden only says "same as before".** The combat golden recorded the black frame as its reference, so the broken picture became the expected one.
3. **No way to look inside a frame.** Finding the cause took six hand-edited renders. The engine has no view modes such as lighting only or AO only.

## How other engines prevent this
- **Unreal, Render Dependency Graph.** Each pass declares the resources it reads and writes. The graph is validated (`r.RDG.Validation`) and unused passes are culled. A pass cannot read a texture nobody produced. ([RDG docs](https://dev.epicgames.com/documentation/en-us/unreal-engine/render-dependency-graph-in-unreal-engine))
- **Bevy, required components.** `ScreenSpaceAmbientOcclusion` is declared `#[require(DepthPrepass, NormalPrepass)]`, so turning AO on brings its prepasses with it. AO without its inputs cannot be expressed. ([bevy_pbr ssao](https://doc.qu1x.dev/bevy_trackball/src/bevy_pbr/ssao/mod.rs.html))
- **Unreal buffer visualisation, and Unity's Rendering Debugger.** These are view modes (base colour, lighting only, AO, depth, shadows) that isolate a fault in one switch.

## Design
Use the simplest version of those ideas, following Unreal's model (D55) without building a general RDG.

1. **Frame plan, built once per settings change.**
   - A `FramePlan` is an ordered list of passes. Each pass names the targets it reads and writes, as an enum of our targets: depth, velocity, AO, HDR colour, history, and so on.
   - `FramePlan::build(settings)` returns `Result<FramePlan, RenderError>` and fails when a pass reads a target no earlier pass writes.
   - The frame loop runs the plan. It never re-checks the booleans.
   - A new feature adds a pass with its reads and writes, and nothing else.
2. **Features that imply their inputs** (the Bevy rule).
   - Replace the boolean struct with a type in which GTAO and TAA imply the depth prepass. For example, `Prepass::{Off, On { gtao: bool, taa: bool }}`, or a builder that adds the prepass whenever GTAO or TAA is asked for.
   - No value of the type can mean "GTAO without the depth it reads". Delete `RenderError::TaaNeedsPrepass` and the check, because the state can no longer happen.
   - Unit test: every tier's features build a valid plan.
3. **Find and fix the black-world bug.**
   - Name the pass, the binding or state, and why in the report.
   - Show one 1280×720 IMAGINE frame per tier (Low, Medium, High, Ultra), each with the floor, walls and crates lit by the sun and with ambient light in shadow.
4. **Debug view modes.**
   - `ViewMode::{Lit, BaseColour, LightingOnly, AmbientOcclusion, Depth, Shadow}` in `engine-render`, chosen in settings.
   - Each one is a pass in the plan, not a branch in every shader.
   - Expose it in the developer telemetry panel and as a headless test option.
5. **Plausibility checks beside goldens.**
   - A small helper in the render tests asserts that a region of the frame has mean luminance within a range.
   - The IMAGINE combat test asserts the floor below the crosshair is lit at every tier. A black world fails even if someone re-records the golden.
   - Goldens are re-recorded only once the frames are correct. Say so in the report.

Done means:
- items 1 to 5 complete end to end;
- the four gates pass;
- `scripts/build-android.sh imagine` builds.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D55;
- `engine/render/src/tier.rs` and `engine/render/src/gfx/` (config, frame, pipelines, setup);
- `engine/render/src/screen_space.rs`;
- `engine/render/src/shaders/scene.wgsl` and `scene_lighting.wgsl`;
- `games/imagine/game/tests/headless_render_golden.rs`.

Do not touch `engine/ui`, `tools/ui-cook`, `games/imagine/ui`, or `engine/netcode`.

Docs in the same change:
- `docs/ARCHITECTURE.md` §1 (render row);
- a decision in `docs/README.md` §7 (frame plan and implied features);
- `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
cargo run -p check-layers
scripts/build-android.sh imagine
