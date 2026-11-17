# render-perf-1: dynamic resolution without stutter, honest GPU timers, shadow culling per cascade (audit C3, H8, H1, M2, M8, M9)

Goal: fix the measured render problems that do not need the retained render world. Done means: (1) changing the render scale allocates nothing on the GPU and keeps TAA history; (2) every GPU pass has its own timestamp pair and the bench reports each pass alone plus the frame (first start to last end); (3) each shadow cascade draws only what its own light box can see, and the far cascade is cached while nothing in it changes; (4) the bench numbers before and after are in the report and in `docs/RENDERING.md` §7.

Read first: `AGENTS.md`; `docs/research/2026-11-11-engine-audit.md` §A (C3, H1, H8, H10, M2, M8, M9), §D, §I; `docs/README.md` §7 D17, D54; `docs/RENDERING.md` §3 and §7; `engine/render/src/{lib,targets,taa,timer,screen_space,governor}.rs`, `engine/render/src/gfx/{mod,frame,config,setup,pipelines}.rs`, the shaders under `engine/render/src/shaders/`, `engine/render/examples/bench.rs`, `engine/render/tests/`.

## Design

**Scope:** `engine/render` only (its source, shaders, examples, tests). The public `Gfx` API keeps its shape: `set_render_scale(f32)` stays, so `engine/app` does not change (another brief is editing `engine/app/src/client.rs` now).

**1. Viewport rendering (C3).**
- Allocate every scaled target (HDR colour, depth, motion, GTAO, TAA history pair, bloom chain) once at the **largest** scale the tier allows (1.0) for the current surface size. They are rebuilt only on surface resize or tier change, never on a scale change.
- A frame renders into the top-left `viewport = ceil(surface × scale)` region: `set_viewport` and `set_scissor_rect` on every scaled pass. Every pass that samples a scaled target gets the viewport size and the UV scale (`viewport / full`) in its uniform block, and clamps samples to the viewport rect (half a texel in), so nothing outside the viewport is ever read (bloom down-sample edges, TAA neighbourhood, GTAO).
- **TAA history across a scale change:** history was written at the previous viewport. Store `prev_uv_scale` in the TAA uniforms and reproject with it; history stays valid. Only resize, tier change and camera cuts invalidate history.
- Composite up-scales the viewport to the swapchain with the existing filter.
- `set_render_scale` becomes: clamp, store, update uniforms. A unit test proves it creates no texture, view or bind group (count them through a small creation counter in `setup.rs` used only in tests, or by keeping texture ids and comparing).
- The governor's `governor_no_alloc` test must still pass.

**2. GPU timers (H8, M8).**
- One timestamp pair per **pass** (each shadow cascade, prepass, GTAO estimate and denoise, scene, each bloom level down and up, TAA, composite), with a pass enum generated in one place. The query set is sized from that enum.
- Report: each pass's own duration; the stage totals as sums of their passes; `frame` = earliest start to latest end. Document in the module header and in PERFORMANCE §1 that on tiled GPUs passes overlap, so the stage sum can exceed the frame and the frame time is what budgets use.
- Bench JSON reports the sample count actually in use (M8) and the per-pass table.

**3. Shadow culling per cascade (H1) and a cached far cascade (M2).**
- Each cascade has its own orthographic light box. Test every instance's bounding sphere (from mesh bounds × instance transform, computed once per instance per frame, no allocation) against each box, extended toward the light so off-screen casters still cast. Each cascade gets its own instance range list; the near cascade never draws far objects.
- The far cascade (the last one) is redrawn only when its light box moves by more than one texel (it is texel-snapped already, or make it so) or when an instance inside it was added, removed or moved this frame. Otherwise its depth layer is kept. On tiers with a single cascade this does nothing.
- Keep the instance buffer layout as is (the compact layout is the next brief).

**4. Small fixes.** `srgb()` uses the exact piecewise sRGB curve (M9), with a test against known values. Check the TAA resolve (H10) after the timers are honest: if it is still over 1.3 ms at 1080p on High, try `Rg11b10Ufloat` history where the adapter can render and filter it, and report the numbers either way; keep the change only if it is measurably faster with no visible banding in `taa_headless`.

**Do not change:** the immediate-mode `draw` API, the instance and vertex formats (later briefs), `engine/app`, any game.

**Tests:**
- Headless GPU tests (skip with a printed reason when no adapter): scale change 1.0 → 0.7 → 1.0 creates no texture; TAA history stays valid across a scale change; a pixel outside the viewport is never sampled (fill the outside with a sentinel colour and assert the output has none of it).
- Shadow culling unit tests (pure CPU): sphere vs cascade box, a caster behind the camera but inside the light box is kept, a far object is not in the near list; far-cascade cache invalidation rules.
- Timer: pass count equals query count / 2; stage sums.
- Bench before and after: `cargo run -p engine-render --example bench --release` for 40×40 and 80×80 blocks, TAA on and off; put both tables in the report and update PERFORMANCE §1.

**Docs in the same change:** `docs/RENDERING.md` §7 (numbers, timer note), `docs/research/2026-11-11-engine-audit.md` (mark C3, H1, H8, M2, M8, M9 fixed, H10 with its result), `docs/ARCHITECTURE.md` §1 render row, `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
