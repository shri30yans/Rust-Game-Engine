# Engine audit and plan (2026-11-11)

> Dated evidence, not the source of truth. The current design is in the topic docs listed in [../README.md](../README.md) §1; old doc names and decision numbers cited below refer to docs merged on 2026-11-17 (see [../README.md](../README.md#7-former-decision-numbers) §7).

A full review of the engine as it stands at commit `6579bc7`, written before any code changes. It answers ten questions (A–J below): what is wrong, what is missing, what the engine should become, how a frame should be drawn, what "fast" means in numbers, which methods to use, how to handle different hardware, in what order to build, how to catch slowdowns, and which past decisions to look at again now.

**Status (owner, 2026-11-11):** gameplay comes first; this plan waits until the game is built, apart from fixes the gameplay work needs. The gameplay order is in IMAGINE's [PLAN.md](../../games/imagine/docs/PLAN.md) §8.1, Since D51 the build order is IMAGINE's PLAN §9 (milestones M0–M8); this audit's roadmap (§H) is folded into it, and each performance fix lands inside the milestone step that needs it. The findings and targets here stay the reference.

Scope rule: this file is a review and a plan. The decisions themselves still live in [decision index](../README.md), the renderer plan in [RENDERING.md](../RENDERING.md), and the layout in [ARCHITECTURE.md](../ARCHITECTURE.md). Once the owner approves items here, each one moves into those files and gets a brief.

## 0. Summary

**State of the code.** About 47,000 lines of Rust and 1,200 lines of WGSL across 11 engine crates, 1 kit, 2 games and 2 tools. All four gates pass today: `fmt`, `clippy -D warnings`, `check-layers`, and 704 tests with none failing. The code is disciplined: typed errors, no panics, enforced layers, determinism tests, allocation-counting tests and a frame-time governor. This is a real base, not a demo.

**The main problem is not the code. It is that the target hardware has never been measured.** Every number we have comes from an Apple M5 laptop. The Android builds have never run on a phone. And the phone build starts on the **Medium** tier with 4× MSAA, 2048² shadows, 32 lights and full resolution, which is the opposite of "low-end first".

**Same-conditions A/B** (`engine-render` bench, Apple M5, Metal, 1920 × 1080, `--grid 80 --taa on`, 240 frames):

| Run | Load average | Main GPU frame / wall p50 | Branch GPU frame / wall p50 |
|---|---:|---:|---:|
| Owner's back-to-back measurement | ~30 | 7.75 / 10.00 ms | 6.63 / 9.52 ms |
| Independent back-to-back rerun | 28.56 main; 29.87 branch; 29.80 after | 13.71 / 16.87 ms | 6.313 / 11.142 ms |

The retained-scene change was benchmarked back-to-back on Apple M5 (Metal), 1920 × 1080, using the same three `bench-gate` cases and frame counts. No other build or benchmark was active during the measured runs. System snapshots around the pair showed 87.7% and 88.0% CPU idle; two user sessions were logged in. The 1-minute load average rose from 1.89 before the pair to 3.82 afterward, while 5/15-minute averages were 2.23/2.44 before and 2.84/2.66 afterward.

| Case | Before GPU / wall p50 / p99 | After GPU / wall p50 / p99 |
|---|---:|---:|
| grid40 TAA | 2.411 / 3.306 / 3.413 ms | 2.384 / 2.956 / 3.006 ms |
| grid80 TAA | 2.885 / 5.024 / 5.243 ms | 2.789 / 3.469 / 3.529 ms |
| grid80 no-TAA | 2.155 / 4.213 / 4.378 ms | 2.061 / 2.727 / 2.803 ms |

The post-change `cargo run -p bench-gate -- --device apple-m5` passed all three cases against `bench/baseline/apple-m5.json`. GPU frame values were effectively flat; the wall-time improvement tracks the removal of per-frame visible-record repacking. A separate 100-frame grid80 TAA sample reported CPU medians of 0.117 ms cull, 0.017 ms upload/index flatten, and 0.161 ms encode/submit.

The owner-measured branch is faster by 1.12 ms GPU frame and 0.48 ms wall p50. The independent rerun also showed a lower branch frame, while its wall p50 varied with load. It reported MSAA 4 on main and 1 on the branch because TAA now enforces one sample, so that run is not a same-sample-count comparison.

On Apple and other tiled GPUs, independently timed passes overlap. In the owner's measurement, bloom passes read about 5 ms each inside a 6.6 ms frame; the independent rerun read about 3.1–3.3 ms per bloom pass inside a 6.313 ms frame. Those values are for relative comparison only. **Only `frame` is valid for budgets on tiled GPUs.** A composite reading of 0.000 ms is the same timing effect, not evidence that the composite is free.

**The ten most important actions**, in order:
1. Get the reference phones (one low-end Android, one mid-range Android, one iPhone) and run the bench on them. Nothing else in this plan can be judged without them (A-C2).
2. Choose the tier from the device, not `Settings::default()`; phones start on Low (A-C1).
3. Make render-scale changes free: allocate targets once at the largest size and draw into part of them (A-C3).
4. Stop one bad frame from closing the game (A-C4).
5. Fix the GPU timers so each pass is measured on its own (A-H8).
6. Cull shadow casters per cascade and give shadows a position-only vertex stream (A-H1, A-H3).
7. Shrink the instance record from 160 to about 64 bytes and the vertex from 52 to about 24 bytes (A-H2, A-H3).
8. Build `engine-assets` and the cooker: textures, glTF meshes, LODs. Most of the rendering plan waits on this (A-H12).
9. Move the renderer to a kept ("retained") list of instances, so unchanged objects cost nothing each frame (A-H4).
10. Turn on a benchmark baseline in CI that fails a change that slows things down (§I).

## A. Audit of the current engine

Severity: **Critical** blocks the low-end goal or can lose a player's session; **High** a large, measured or near-certain cost; **Medium** a real cost that is not yet urgent; **Low** cleanup. Size: **S** under a day, **M** a few days, **L** a week or more.

### A.1 Critical

| # | Finding | Evidence | Effect | Fix | Size |
|---|---|---|---|---|---|
| C1 | Phones run the Medium tier at full quality. The tier benchmark (D19) is not built, and the device profile is built without its GPU part, so the known-bad GPU list is never checked. | `games/imagine/game/src/lib.rs:60-66` passes `Settings::default()` and `DeviceProfile::detect(class, None)`; `engine/render/src/gfx/config.rs:67-80` makes the default Medium, MSAA 4×, 2048² shadows, 32 lights, PCF, scale 1.0. | A low-end Android phone gets the desktop-laptop setting. The governor can lower resolution only to 70 %, which cannot cover MSAA 4× and 32 lights on a weak GPU. | `engine-app` picks the tier: device class and GPU profile give a starting tier (phones: Low), then the short start-up bench (D19) moves it up or down and saves the result. Pass the GPU profile into `detect`. | M |
| C2 | No measurement on any phone. | GRAPHICS §7 lists the reference phones as "to be chosen"; README §2 says the APKs were never run on a device. | Every performance claim for the main target is a guess. Thermal limits, driver bugs and tiled-GPU behaviour are unknown. | Choose and buy the phones; run `bench` on them with a fixed scene list; record cold and 10-minute numbers. | S (work) + hardware |
| C3 | **Fixed (see A.3.1):** Every change of render scale rebuilds every render target. | `Gfx::set_render_scale` → `rebuild_targets` (`engine/render/src/gfx/mod.rs`), called by the governor in `engine/app/src/client.rs` `run_frame`. It also throws away the TAA history. | The governor lowers the scale exactly when the frame is already late, and the rebuild (new textures, new bind groups, new screen-space pipelines) makes that frame later. On phones, allocating GPU memory mid-game is a classic hitch. | Allocate targets once at the largest scale. Render into a viewport of the wanted size and pass the size to the passes that sample (TAA, bloom, composite). Scale changes then cost nothing and keep TAA history. | M |
| C4 | **Fixed:** a single frame error no longer ends the game. | `ClientApp::frame` retains the renderer, classifies errors at their phase, reports skipped frames in its engine-owned banner, and resets the failure count after a good frame. Client tests cover recovery after draw failures and stopping after 120 consecutive failures. | A transient prepare, camera, telemetry, draw or UI error no longer closes the client; prolonged consecutive failure still stops it. | Keep frame errors recoverable and stop after 120 consecutive failures; initialization and simulation errors stop immediately. | fixed |

### A.2 High

| # | Finding | Evidence | Effect | Fix | Size |
|---|---|---|---|---|---|
| H1 | **Fixed (see A.3.1):** Shadow cascades have no culling of their own. Both cascades draw every instance that is visible or near the camera. | `engine/render/src/gfx/frame.rs:184-220` draws `so + both` for each cascade; `draw_ex` keeps anything within `splits[1] + r + 30 m`. | Shadow cost grows with the whole nearby scene (bench: 0.46 → 1.27 ms). The near cascade draws far objects it can never show. On phones shadows are often the largest pass. | Test each instance's bounding sphere against each cascade's box and keep one list per cascade. Cache the far cascade and redraw it every second or fourth frame, or when destruction changes something in it. | M |
| H2 | **Implemented:** instances occupy stable 64-byte GPU slots indexed by `PrimitiveId`; previous transforms use a separate 48-byte storage stream. Dirty ranges write only affected slots. | `RenderScene`, `Gfx::upload_scene_changes`, and `FrameStats`. | Headless tests prove 0 bytes for a static 1,000-primitive scene after frame one, and a TAA move writes 64 bytes, then 48 previous-transform bytes, then 0. | Keep the slot indexing and upload counter covered by tests. | fixed |
| H3 | **Implemented:** procedural meshes use 12-byte positions and 12-byte attributes; shadow and depth prepass bind only positions. The depth prepass reconstructs its normal from position derivatives. Small meshes use u16 indices. | `Vertex`, `VertexAttributes`, `shadow_bufs()`, and the prepass pipeline in `engine/render/`. | Headless TAA tests and normal/record encoding tests pass. | Cooked Quantized16 uploads remain C4 and are outside this brief. | fixed |
| H4 | **Implemented:** `Gfx::draw*` is removed. One dense SoA cull loop emits slot indices into reusable per-mesh ranges; flattened ranges address one grow-only GPU index buffer. Shaders index stable instance storage by those slot IDs. | `RenderScene::cull`, `Gfx::submit_scene`, `Gfx::flatten_visible_indices`, and scene/shadow shaders. | A 1,000-frame moving-scene allocator test proves extract, retained updates, and culling allocate 0 bytes after warm-up. All three render gate cases pass; grid80 TAA wall p50 improved from 5.024 to 3.469 ms in the back-to-back run above. | Keep the single retained path and allocator/performance coverage. | fixed |
| H5 | **Implemented:** `World::for_each1/for_each2` provide dense `(NetId, items)` iteration without exposing hecs entities; extractor and render-frame lookups use generation-checked NetId slot tables. Presenter bit comparison remains the change detector. | `World::for_each1/for_each2`, `Extractor`, `RenderFrame`, and `ScenePresenter`. | Extract-only 240-sample release p50/p99: main 40×40 0.0730/0.1304 ms, 80×80 0.1668/0.2056 ms; branch 40×40 0.0357/0.0420 ms, 80×80 0.1028/0.1388 ms. The combined 1,000-frame moving extract/update/cull counting allocator test passes with 0 allocations and 0 reallocations. | Keep dense query, slot table, and no-allocation test. | fixed |
| H6 | The job pool is created but nothing uses it. Rapier's parallel feature is off, the threaded render mode is not built. | `JobPool` appears only in `engine/app/src/app.rs`; `engine/physics/Cargo.toml` has no `parallel` feature. | The whole frame runs on one core. Phones have slow single cores and 2–4 fast ones. | In order, once phone numbers show the CPU is the limit: threaded render mode (D26); culling in chunks on the pool; physics `parallel` with the pool handed to Rapier; structural solve in chunks. Each keeps the 0 / 1 / all-workers hash test. | L |
| H7 | On Low and Medium there is no depth prepass and opaque work is not sorted front to back, while the main fragment shader is expensive (procedural patterns with noise, up to 32 lights, 3×3 PCF). | `RenderTier::features` turns the prepass on only for High and Ultra (`engine/render/src/tier.rs`); draws go in mesh-id order. | On Apple GPUs hidden-surface removal hides the cost. Mali and Adreno only partly reject hidden pixels, so overdraw pays the full shader. | Sort bins by the distance of their nearest visible instance (cheap and coarse). Measure prepass on and off on the phones. | S |
| H8 | **Fixed with backend limitation (see A.3.1):** GPU stage timers overlap and are wrongly attributed. | `Stage::Bloom` starts at the first bloom pass and ends at a later one (`frame.rs:506-516`); on tiled GPUs consecutive passes overlap, so a stage spanning several passes counts its neighbours' time. Bench: the stages add up to more than the frame; bloom doubles with TAA on. | Budgets in GRAPHICS §7 cannot be checked; the "bloom mystery" in GRAPHICS §7 is most likely this. | One timestamp pair per pass; report each pass alone; report the frame as first start to last end. Note in GRAPHICS that on tiled GPUs pass times overlap and the frame time is the number that matters. | S |
| H9 | **Within budget (see A.3.1):** GTAO costs 1.1–1.3 ms at half resolution on the M5, twice its budget. | Bench table above. | On phones it is out of reach; on desktop it eats the headroom. | Fewer directions per pixel and let TAA accumulate across frames (spatio-temporal sampling, the usual way to make GTAO cheap); bilateral upsample. Keep it High and up only. | M |
| H10 | **Improved (see A.3.1):** TAA costs about 2.0–2.4 ms at 1080p on the M5 (budget 1.3 ms with all post). | Bench table; part of this may be H8's overlap. | If real, it is a big share of the desktop budget. | First fix H8 and measure again. Then check the resolve's texture reads and target formats (history in `Rgba16Float`; consider `Rg11b10Ufloat`). | S to M |
| H11 | The UI rebuilt its whole layout tree every frame. | Previously `UiLayout::frame` created a new `TaffyTree` every call. | Allocated and spent CPU on a HUD that rarely changes shape. | **Fixed:** `UiLayout::load` builds a retained tree; unchanged frames return the cached draw list, and paint-only binding changes skip layout. Geometry changes mark affected nodes dirty. The measured HUD has zero allocations on clean and paint-only frames; taffy uses 2 temporary allocations (744 bytes) on a dirty layout. | fixed |
| H12 | **Implemented for the host render path:** the app-owned uploader connects store events to typed GPU packs, cooked Float32/Quantized16 meshes, worker-prepared textures and frame-budgeted uploads; reload preserves the last good resources and primitive ids. | `engine-app::AssetUploader`, `engine-assets::prepare_texture`, `Gfx::add_cooked_mesh`, `Gfx::add_texture`, `RenderScene::replace_mesh` / `replace_material`, `FrameStats::asset_upload_bytes`; cooked pistol in IMAGINE. | Host tests cover 40 MiB budget spreading, pending-to-ready, live-primitive reload, mismatch rejection, missing-texture failure, unload memory release, reusable slot generations, textured High/Low PBR pixel sampling, and the idle uploader allocator. The textured bench case is recorded. All three validation gates failed against the historic baseline; the host load average was 12.80, with a game and extension host each using about one core. The IMAGINE viewmodel golden has not been re-recorded. | Re-run the performance gate on a quiet M5 and record the IMAGINE viewmodel golden; phone/web device runs remain future work. | in progress |

### A.3 Medium

| # | Finding | Evidence | Fix | Size |
|---|---|---|---|---|
| M1 | Light tiles store a 32-bit mask, so 32 lights is a hard limit, and each tile re-projects all 8 corners of every light. | `shaders/tiles.wgsl` | Project each light once (a small first pass) and test tiles against the rectangle. Replace with clusters (GX4) or the per-object list on the Direct path (GRAPHICS §6). | M |
| M2 | **Fixed (see A.3.1):** Shadows are redrawn every frame even when nothing in them moved. | `frame.rs` shadow loop | Cache static casters per cascade; redraw on change (destruction already sends events). | M |
| M3 | Particles are built on the CPU each frame and capped at 12,000. | `Gfx::particle` | Fine until GX3 (GPU particles). Keep the CPU path as the Direct-path fallback. | — |
| M4 | The HDR target is `Rgba16Float` on every device. | `targets.rs` | Use `Rg11b10Ufloat` where the GPU can render to it: half the bandwidth for the main colour target and bloom. | S |
| M5 | Rapier runs with `enhanced-determinism`, which excludes its optional `simd8` path. | `engine/physics/Cargo.toml` feature `cross-platform-determinism` | J4 remeasured the default deterministic mode and opt-in `simd8` mode after every character move began excluding its required body. Default Rapier features are `dim3`, `f32`, `std`, plus workspace `serde-serialize`; A adds `enhanced-determinism`, and C adds `simd8`. | measured |
| M6 | Text uses fontdue with a single-size SDF atlas; there is no text shaping. D11 says parley and MSDF. | `engine/render/src/font.rs` | Needed for localisation (Arabic, Hindi, Thai, CJK), rich text and sharp text at any size. Move to parley (shaping) plus an MSDF atlas. | M |
| M7 | Text input and rich widgets remain. CSS linear gradients are cooked and drawn by `engine/ui` and the UI pass in `engine/render`. Images, icons and nine-slice are not drawn yet; the placeholder atlas was removed, so none of them can render until step 3 of [briefs/ui-kit-2.md](../briefs/ui-kit-2.md) lands. Still open, moved to [briefs/ui-kit-3.md](../briefs/ui-kit-3.md): scroll views, clipping, font faces and text styles, safe areas. | `engine/ui`, `tools/ui-cook`, `engine/render` UI pass | Finish ui-kit-2 step 3 (cook and draw images, icons and nine-slice; resvg; runtime atlas upload), then ui-kit-3 (clipping and scroll, font faces, safe areas). | L |
| M8 | **Fixed (see A.3.1):** Bench JSON reports `"msaa":4` when TAA has turned MSAA off. | bench output | Report the sample count in use. | S |
| M9 | **Fixed (see A.3.1):** `srgb()` uses a 2.2 power curve, not the exact sRGB curve. | `engine/render/src/lib.rs` | Use the piecewise sRGB formula so colours from design files match. | S |

### A.3.1 Render-perf-1 status (2026-11-11)

| Finding | Status | Result |
|---|---|---|
| C3 | Fixed | Full-size targets are retained across scale changes; the active viewport is clamped in scaled passes and TAA history stays valid. A GPU test confirms scale changes do not rebuild targets. |
| H1 | Fixed | Instance spheres are tested against each cascade light box and each cascade has a separate instance list. CPU tests cover outside spheres, a caster behind the camera, and far-only visibility. |
| H8 | Fixed with backend limitation | Every listed pass has its own timestamp pair and the frame spans the earliest start to latest end. On tiled GPUs, pass times overlap and are relative-comparison data only; only frame is budget-valid. The Apple M5 composite query returns 0.000 ms as part of the same effect; see GRAPHICS §7. |
| M2 | Fixed | The far layer is reused while its snapped light matrix and caster instance bytes are unchanged. |
| M8 | Fixed | Bench JSON reports active MSAA and the GPU sample count; `passes_overlapping` explicitly labels per-pass timings as overlapping on tiled GPUs. |
| M9 | Fixed | `srgb()` uses the piecewise sRGB transfer curve with known-value checks. |
| H10 | Improved, 2026-11-12 | The real TAA cost is the GPU frame with TAA on minus off (per-pass times overlap, H8): 1.42 ms → 0.72 ms at 1080p, grid 80, on the M5 at load 1 (frame 3.584 → 2.887 ms against 2.167 ms without TAA; wall p50 4.71 → 4.00 ms; reproduced across runs). History is a 5-tap bilinear Catmull-Rom instead of 16 loads; the closest-depth search reads 5 pixels (cross) instead of 9. The converged image matches the old resolve (mean difference 0.00/255, max 20). Still open: compact history (`Rg11b10Ufloat` needs the adapter-specific format feature) and a compute version sharing the 3×3 neighbourhood through workgroup memory. |
| GTAO budget | Within budget | Measured by A/B (GTAO off in the High tier): 0.58 ms against the 0.6 ms budget. The earlier 1.1 ms was the overlapping per-pass timer (H8), not real cost. |

### A.4 What is already right (keep it)

- **Layers and the platform split** (D24), checked by a tool on every gate run.
- **Fixed tick and interpolation** between the last two ticks; **late camera** for aim.
- **Determinism tests** with state hashes, and **allocation-counting tests**.
- **Instancing by mesh**: 10,000 instances in 13 draws.
- **The governor** for dynamic resolution, apart from C3.
- **Device profile** with performance-core detection (D35), and a known-bad GPU list (once C1 wires it up).
- **Typed errors everywhere**, no panics in engine code.
- **hecs** (an archetype store) is the right kind of ECS for iteration-heavy work; there is no reason to change it. Sparse-set stores are faster at adding and removing components but slower to iterate, and our hot work is iteration.
- **Rapier for physics**. Writing our own would take a year to reach where Rapier is; Jolt is better known but is C++, which means unsafe bindings and a second build system.

## B. Missing features

**Fundamental** (a commercial engine cannot ship without these):

| Feature | State | Needed for |
|---|---|---|
| Asset loading, cooking, handles and hot reload | slice 1 implemented; render upload remains next | everything visual (H12) |
| Textured PBR materials, compressed textures (BC / ASTC / ETC2), mipmaps | planned (GX2) | image quality, memory |
| glTF meshes from Blender, mesh LODs | planned | real content |
| Tier choice from the device and the start-up bench | not built (C1) | phones |
| Audio: mixer, 3D sound, streaming music | planned (`engine-audio`, kira) | any game |
| Skeletal animation: clips, blending, state graph, GPU skinning | planned (`engine-animation`) | characters, guns |
| Spatial grid for "what is near" | planned (`engine-spatial`) | culling, network relevance, AI, sound |
| Save data and settings storage per platform | not planned anywhere | settings, progress |
| Crash and error reporting from devices | not planned | phones in the wild |
| Gamepad input | not built | desktop, phones with controllers |
| Localisation: string tables, shaping, font fallback | not planned | release outside English |
| Accessibility: text scale, colour-blind modes, reduce motion, subtitles | partly in QUALITY | release |
| Profiler with per-system CPU timing | telemetry only (frame times) | every performance task |

**Advanced** (needed for the quality bar in QUALITY.md, after the base):
GPU particles and decals (GX3); 4-cascade shadows with PCSS and contact shadows (GX4); 3D light clusters (GX4); physically based sky, aerial perspective, volumetric fog (GX5); reflection probes and SSR (GX6); probe GI (GX7); histogram exposure, AgX, colour grading (GX8); GPU-driven culling with HZB (GX9); texture streaming (GX11); temporal upscaling; HLOD stand-ins; nav mesh rebuilt after destruction; netcode slices 2+ (snapshots, prediction, lag compensation); sessions and matchmaking; entity inspector and debug drawing (`engine-devtools`); replay viewer.

**Not mentioned anywhere yet, and worth adding**:
- **Pipeline cache on disk** (wgpu `PipelineCache` on Vulkan) so the first launch on Android does not compile every shader each time. On Android, shader compilation is a large part of start-up time.
- **Frame pacing on Android** (Swappy-style, through the presentation mode and the 120 Hz request already in D38). Uneven frame delivery feels worse than a lower steady rate.
- **Thermal state reading** (Android `PowerManager` thermal status, iOS `thermalState`) fed into the governor and tier choice, so the game lowers quality before the phone throttles it.
- **Memory pressure handling** (Android `onTrimMemory`, iOS memory warnings) that drops streamed mips and cosmetic debris.
- **Debug drawing** (lines, boxes, spheres from any crate, compiled out in release). Every later system is easier to build with it.
- **Determinism and replay capture from real matches**, which the design already makes possible.
- **Content validation in CI**: every level, UI document and tuning file loads and passes validation.
- **Build-time tracking**: clean and incremental build times recorded, since iteration speed is a stated goal.

**Optional** (superseded by D83 on 2026-11-17: terrain, large worlds and streaming are now engine work against the engine test scenes; the text below is kept for history) (only if a game needs them): terrain with height maps and clipmaps; water; foliage instancing with wind; day–night cycle; weather; vehicles; cloth; soft bodies; planetary gravity; portals. IMAGINE is an arena shooter in a city; none of these are on its plan. They should be built when a game asks for them (D24: "if a feature is only useful to one game, it lives in that game"), with the general part in the engine. Gravity is already a per-world setting in `PhysicsConfig`; several gravity sources are a small addition (a force applied per body before the step) when a game needs them.

## C. Proposed architecture

The layers in ARCHITECTURE §1 stay. Four changes:

1. **`engine-assets` (layer 1) becomes real** and owns every file on disk after cooking: meshes, textures, levels, UI documents, sounds. Other crates ask for a typed handle; the background pool loads it; the render thread uploads within a per-frame byte budget.
2. **`engine-render` is split inside into four modules** (not crates, to avoid needless boundaries):
   - `world`: the retained render world (instance slots, materials, meshes, lights), changed only by extract.
   - `visibility`: frustum, cascade and distance culling over slot arrays; later the GPU-driven path.
   - `passes`: one module per pass with its own targets and timer.
   - `backend`: wgpu setup, the pipeline cache, the GPU timer.
3. **`engine-spatial` (layer 1)** as already decided (D27), shared by culling, network relevance, AI and sound.
4. **`engine-devtools` (layer 3)** gets a CPU profiler with named scopes from the start (`tracing` spans, already a dependency), so every later system is measured from day one.

**Data flow of a frame** (target):

```text
platform  ──input──▶  app: fixed tick(s)
                        world step: game systems → physics → destruction → events
                      extract (changed only)  ──▶  render world (slots, dirty ranges)
assets (background) ──loaded resources──▶ render: upload within byte budget
render thread:  late camera → cull (CPU or GPU) → bins → passes → present
audio thread:   events + listener → kira mixer
telemetry:      CPU scopes + GPU pass times + memory → overlay, bench JSON
```

Simulation never reads render or audio state. Render, audio and UI read simulation state only through extract and events. This is already the rule; the retained render world makes it cheap.

## D. Rendering pipeline: from submission to the screen

Two render paths, chosen once at start-up from the device profile (D28, D33). Phones start on the Direct path until a phone bench shows the GPU-driven path is faster and stable there.

**1. Extract (main thread, under 0.5 ms desktop / 1 ms phone).** Changed transforms, spawns, removals, material changes and destruction events go into the render world's dirty lists.

**2. Update the render world (render thread).** Dirty instance slots are copied into the GPU instance buffer with one write per dirty range. Asset uploads run within the byte budget.

**3. Cull.**
- Direct path (CPU, on the job pool): grid cells against the frustum, then instance spheres; distance ranges (D32) drop small far things; per-cascade lists for shadows.
- GPU-driven path: a compute pass does the same per instance, then the two-pass HZB occlusion test, and writes indirect draw arguments.

**4. Bin and sort.** Opaque work is grouped by pipeline, material and mesh; only the bin keys are sorted; bins are ordered front to back by their nearest instance. See-through work is sorted back to front by a 64-bit key.

**5. Passes**, in order (tiers in brackets):

| Pass | Low (phones) | Medium | High / Ultra |
|---|---|---|---|
| Shadows | 1–2 cascades, 1024², far cascade cached | 2–3 cascades, 2048² | 4 cascades, PCSS, contact shadows |
| Depth + motion prepass | off unless the phone bench says it helps | measured | on |
| GTAO | off | off or quarter-res | half / full res, temporal |
| Opaque lighting | forward, per-object light list (≤ 8) | forward+, tiles | forward+, clusters |
| Sky | analytic | analytic | physically based + aerial perspective |
| See-through, particles | sorted, half-res particles | sorted | sorted, full |
| Anti-aliasing | MSAA 2× / 4× resolved on tile | TAA or MSAA (measured) | TAA, temporal upscale |
| Bloom | 3 levels, quarter res | 4 levels | 5 levels |
| Composite | tonemap + grade in one pass with UI | same | + film grain, sharpen, DOF on aim |

On tiled GPUs (all phones, Apple), passes that can share one render pass are merged, and depth and MSAA samples that are not needed later are discarded instead of stored (`StoreOp::Discard`), so they never leave the chip.

**6. UI** is drawn last in the composite pass from one vertex buffer and one atlas (one draw today; keep it that way).

**7. Present** with the platform's frame pacing at 30, 60 or 120 Hz.

## E. Performance targets

All targets are measured with the bench scenes in §I, not assumed. Frame times are wall-clock from present to present. "1 % low" is the 99th-percentile frame time.

| Measure | Low-end Android (Low) | Mid Android / iPhone (Medium) | Desktop reference (High) |
|---|---|---|---|
| Frame rate | 30 fps steady, 60 fps where it holds | 60 fps | 60 fps at 1440p; uncapped above |
| 99th-percentile frame | ≤ 1.25 × target | ≤ 1.25 × target | ≤ 1.2 × target |
| After 10 minutes (heat) | ≥ 90 % of the cold frame rate | ≥ 90 % | — |
| Main-thread CPU per frame | ≤ 8 ms at 30 fps | ≤ 6 ms | ≤ 4 ms |
| Simulation tick (60 Hz) | ≤ 4 ms | ≤ 3 ms | ≤ 2 ms |
| Extract | ≤ 1 ms | ≤ 1 ms | ≤ 0.5 ms |
| GPU frame | ≤ 28 ms at 30 fps | ≤ 14 ms | ≤ 12 ms (GRAPHICS §7 table) |
| Draw calls | ≤ 150 | ≤ 300 | ≤ 1,000 |
| Instance upload per frame | ≤ 64 KB (static is free) | ≤ 128 KB | ≤ 512 KB |
| GPU memory | ≤ 600 MB | ≤ 1 GB | ≤ 2.5 GB |
| CPU memory (process) | ≤ 700 MB | ≤ 1 GB | ≤ 2 GB |
| Cold start to menu | ≤ 6 s | ≤ 4 s | ≤ 3 s |
| Match load | ≤ 10 s | ≤ 6 s | ≤ 4 s |
| Allocations per tick, per frame | 0 after warm-up | 0 | 0 |
| Shader compile during play | 0 | 0 | 0 |
| APK / app size | ≤ 150 MB base, rest downloaded | same | — |

These are starting targets for the owner to approve. Once phone measurements exist, the Low row should be checked against them and tightened, not loosened, wherever the hardware allows.

## F. Methods and data structures per system

| System | Method | Why | Rejected |
|---|---|---|---|
| Entities | hecs archetypes; `NetId` map at the edge (keep) | fast iteration; ids safe for the network | sparse sets (slower to iterate), own ECS (no gain) |
| Change tracking | a `changed_at: Tick` on the components the renderer reads | extract only changed things | per-frame full copy (today) |
| Render instances | slot arrays (struct of arrays) with a free list; dirty ranges | stable GPU indices, O(changed) uploads | per-frame rebuild |
| Culling (CPU) | sparse grid cells (D27) → sphere vs 6 planes, chunked on the job pool | small scenes, destructible world, nothing baked | octree / BVH (rebuild cost on destruction; the grid is enough at arena scale) |
| Culling (GPU) | compute frustum + two-pass HZB, indirect draws | standard in Bevy, Unreal, Wicked | hardware occlusion queries (latency, mobile cost), portals and PVS (break with destruction) |
| Shadow culling | sphere vs each cascade's box; static cache per cascade | cost follows what each cascade shows | — |
| Draw order | bins by pipeline / material / mesh, front-to-back by nearest instance; radix sort for see-through | cheap, gives early-Z on Mali/Adreno | full per-instance sort |
| LOD | meshopt simplification in the cooker; screen-size switch; dithered cross-fade under TAA | automatic, no popping | runtime simplification |
| Far detail | visibility ranges and HLOD stand-ins (D32) | one mechanism for LOD, cut-off and stand-ins | — |
| Meshlets | later and only for High/Ultra, after the GPU-driven path | our scenes are buildings, not scanned rock | Nanite-style as a goal |
| Textures | KTX2 + Basis, transcoded to BC7/BC5, ASTC or ETC2; mip streaming by screen size (D31) | one source, small downloads | separate per-platform sources |
| Atlases | for UI and decals; not for world materials (use texture arrays) | atlases break mipmaps and wrap | world atlasing |
| Lights | forward+; per-object list (Direct), tiles then clusters (GPU-driven) | cheapest per tier | deferred (G-buffer bandwidth on phones, poor MSAA) |
| Shadows | cascaded shadow maps, cached far cascade; atlas for local lights | standard and scalable | virtual shadow maps (too heavy for phones) |
| AO | GTAO with temporal accumulation (High+) | best quality per ms | SSAO (worse), none on Low |
| GI | probes (spike first, D18) with a cheap phone mode (see J5) | works with destruction | lightmaps (break with destruction) |
| Anti-aliasing | MSAA on tiled GPUs, TAA on desktop; temporal upscale later | MSAA is near free on tile memory | FXAA as the main AA (blurry) |
| Physics | Rapier: BVH broad phase, fixed 60 Hz step, CCD only on fast small bodies | mature, Rust, deterministic | own physics, Jolt (C++), Avian (tied to Bevy's ECS) |
| Character | Rapier kinematic controller (keep) | predictable | dynamic-body characters |
| Structural destruction | load graph with per-part solve and a per-tick budget (keep) | bounded spikes | full re-solve |
| UI layout | taffy with a kept tree; relayout only on change | flexbox people know; no per-frame work | per-frame rebuild (today) |
| Text | parley shaping + MSDF atlas | all scripts, sharp at every size | bitmap fonts |
| Audio | kira, a fixed voice limit with priorities, distance attenuation, occlusion from a ray per sound per few ticks | low CPU | full acoustic simulation |
| Animation | own runtime (D14): sampled clips, blend tree, GPU skinning, animation LOD (lower rate when far) | simple, fast | — |
| Jobs | rayon pool with fixed chunks (keep, D25) | deterministic | work stealing per system graph like Bevy (harder to keep deterministic) |
| Memory | pre-sized arrays, per-frame scratch arenas reset each frame, pools for debris and particles | zero allocation in play | general allocation in hot paths |

## G. Hardware and graphics API strategy

**Superseded by D82 (2026-11-17): the engine moves to its own Vulkan layer.** Original recommendation, kept for history: **Keep wgpu.** It gives Vulkan (Android, Windows, Linux), Metal (Apple), DX12 (Windows) and WebGPU from one renderer, in Rust, without unsafe code in our crates. Writing our own backends would cost a team-year per API and repeat work wgpu already does. The cost is that some features arrive late (mesh shaders, some mobile extensions) and wgpu adds some CPU per call; with instancing and a retained render world our call count stays small, so that overhead stays small. Only `engine-render` and `engine-platform` name wgpu, so the door to a console backend stays open (D33).

**Per platform:**

| Platform | API | Baseline | Fallback |
|---|---|---|---|
| Android | Vulkan 1.1 | Android 10+, Mali G52 / Adreno 610 class or newer | wgpu's OpenGL ES 3.1 backend only if the device survey shows many target phones with broken Vulkan; decide after C2 |
| iOS | Metal | A13 or newer | — |
| Windows | DX12 | DX12 feature level 11_1 GPU | Vulkan |
| macOS | Metal | Apple silicon | — |
| Web | WebGPU | later; Direct path, single-threaded | — |

**Capability detection.** The device profile already holds GPU features and limits. Add: compute support, storage buffer count in vertex shaders, indirect-draw-first-instance support, `Rg11b10Ufloat` render support, ASTC / BC / ETC2 support, timestamp queries, total memory, and the known-bad list match. From these come three choices: render path, texture format and starting tier. Nothing checks an OS or a GPU by name outside the profile.

**The same game at different settings.** A tier is plain data (`Settings` and `RenderFeatures`), so every pass reads its tier switch. Rule: a tier changes only presentation, never simulation (already in ARCHITECTURE §3).

## H. Roadmap

Each phase ends with something playable and measured. A step starts only when the steps it depends on are done.

**Phase 0: measure and stop the bleeding** (1–2 weeks, no new features)
| Step | Depends on | Done when |
|---|---|---|
| 0.1 Reference phones chosen and on the desk | owner | phones in hand |
| 0.2 GPU timers per pass (H8); bench reports correct MSAA (M8) | — | stages add up to no more than the frame |
| 0.3 Bench scene list and JSON baseline in the repo; CI comparison (§I) | 0.2 | a slow change fails CI (built as `bench-gate`, run by hand until a GPU CI runner exists) |
| 0.4 Phone bench runs (cold and after 10 minutes) for Low/Medium | 0.1, 0.2 | numbers in GRAPHICS §7 |
| 0.5 Tier from device profile + start-up bench (C1) | 0.4 | phones start on Low |
| 0.6 Render scale without rebuilds (C3) | — | governor change costs < 0.1 ms, keeps TAA history |
| 0.7 Frame errors no longer quit (C4) | — | test: a spawn burst past capacity keeps running |

**Phase 1: critical architecture fixes** (3–5 weeks)
| Step | Depends on | Done when |
|---|---|---|
| 1.1 Retained render world + change tracking in extract (H4, H5) | 0.3 | static scene uploads 0 bytes per frame; bench CPU time falls |
| 1.2 Compact instance and vertex formats; position-only shadow stream (H2, H3) | 1.1 | bandwidth per frame halves in the bench |
| 1.3 Per-cascade shadow culling and far-cascade cache (H1, M2) | 1.1 | shadow time flat as the grid grows |
| 1.4 Front-to-back bins (H7); `Rg11b10Ufloat` HDR where supported (M4) | 1.1 | measured on a Mali or Adreno phone |
| 1.5 Retained UI layout (H11) | — | zero allocations per frame in the HUD |
| 1.6 CPU profiler scopes in `engine-devtools` + debug drawing | — | overlay shows per-system time |

**Phase 2: core engine** (6–10 weeks)
`engine-assets` + `tools/cook` (H12) → textured PBR materials and compressed textures (GX2) → meshopt LODs and visibility ranges → Vulkan pipeline cache on disk → `engine-spatial` → `engine-audio` → `engine-animation` with GPU skinning → gamepad → save data → thermal and memory-pressure hooks.

**Phase 3: rendering performance** (4–6 weeks)
Job-pool culling → threaded render mode (D26) → physics `parallel` → GPU-driven path behind the device profile (GX9) → HLOD stand-ins → texture mip streaming (GX11). Each step is kept only if the bench shows a gain on at least one reference device and no loss on the others.

**Phase 4: advanced graphics** (ongoing, ordered by GRAPHICS §9)
GPU particles and decals (GX3) → shadows and clusters (GX4) → sky and fog (GX5) → reflections (GX6) → GI spike, then GI (GX7) → post (GX8). Every feature ships with a Low-tier setting (often "off") and a cost in the bench.

**Phase 5: developer experience**
Entity inspector, destruction view, replay scrubber (`engine-devtools`); UI widgets (scroll, clip, sliders, text input, images, nine-slice), parley text, localisation, accessibility settings; content validation in CI; build-time tracking. Scene editing stays in Blender (D15); an in-engine editor is not planned and should not be until Blender is shown to be the bottleneck.

**Phase 6: later**
Meshlets and hardware ray tracing for Ultra (GX10), WebGPU build, consoles, optional systems from §B when a game asks.

## I. Benchmarks and catching slowdowns

1. **Bench scenes**, run headless at fixed sizes and seeds: the current block grid at 40 and 80; `destruction_yard` (a collapse with debris); `stress_city` (many lights, effects and chunks); a HUD-only scene; a simulation-only run of 3,600 ticks with bots. The test scenes in GRAPHICS §8 join as they are built.
2. **What each run reports** (JSON, one line): wall frame p50, p99 and max; CPU time per named system; GPU time per pass; draws; triangles; instances; bytes uploaded; allocations per frame (counting allocator); GPU and CPU memory.
3. **Baseline in the repo**: `bench/baseline/<device>.json`. CI runs the desktop scenes on every change and fails if p50 grows by more than 5 % or p99 by more than 10 % on three runs in a row (to ignore noise). Updating the baseline is a deliberate commit with the reason in its message. **Built (2026-11-12):** `cargo run --release -p bench-gate -- --device apple-m5` runs the three block-grid cases three times each, keeps each metric's best run, and exits 1 on a slowdown; `--record "why"` rewrites the baseline. Compared: GPU frame and wall p50 (5 %), wall p99 (10 %); per-pass times are not compared because they overlap on tiled GPUs. Checked by undoing the H10 TAA change: the gate reported +20 to +27 % on every TAA case. Not yet run by CI (no GPU runner); run it by hand before merging render work.
4. **CPU micro-benches** with criterion (already a dependency) for extract, culling, structural solve, snapshot building and UI layout.
5. **Allocation tests** (already in use, `stats_alloc`) for every per-tick and per-frame path, including extract, culling and the HUD.
6. **Phones**: a script installs the APK, runs the bench scenes cold and after a 10-minute loop, and pulls the JSON back. Run before every merge that touches render, app or physics, until a device farm makes it automatic.
7. **Visual checks**: approved screenshots per tier and per path (QUALITY §7), so speed-ups never lower image quality silently.
8. **Determinism**: the existing state-hash tests, run with 0, 1 and all workers, guard every threading change.

## J. Decisions to look at again now

| # | Decision | Concern | Recommendation |
|---|---|---|---|
| J1 | D17: "Unreal-5-level image quality" as the headline goal | It pulls work toward desktop features while the phone path is unmeasured. The prompt for this audit puts low-end performance first. | Keep the quality bar for High/Ultra, but make the first goal **"Low tier, steady 60 fps (30 on the weakest phone) after 10 minutes on the reference Android phone"**. No new desktop-only feature until that holds. |
| J2 | Immediate-mode `Gfx::draw` API | Public immediate draws and visible-record staging are gone. The single render path culls to slot indices and indexes retained GPU records; the cull allocator test and render gate pass. | Keep the retained path as the only renderer submission path. |
| J3 | D19 tier benchmark "decided" but not built | Phones run Medium today (C1). | Build it in phase 0. |
| J4 | D6/D7: bit-identical physics on every platform (`enhanced-determinism`) | It excludes Rapier's optional `simd8` path; the netcode already corrects clients from server snapshots, so clients never need to match the server bit for bit. Same-binary determinism (replays, tests) does not need it either. The corrected benchmark requires each character to name its body, filtering that body's colliders from movement queries. | **Recommendation:** keep cross-platform determinism on by default pending the owner's decision. Under heavy system load, mean p50 was about 17% lower with SIMD, but tail and mean timings varied widely; remeasure on an idle machine before using this as a performance decision. |
| J5 | D18: probe GI with voxel ray tracing on every tier | Compute ray tracing every frame is not affordable on low-end phones. | Desktop: as planned, after a spike. Phones: probes updated only near destruction at a low rate, or a flat ambient plus GTAO-free contact darkening. Write the phone mode into D18 before the spike. |
| J6 | GRAPHICS §3 target: TAA on every tier from Medium up | On tiled GPUs MSAA resolves on the chip and is nearly free; TAA needs a prepass, motion vectors and a history target, all of which cost memory traffic. | MSAA on phone tiers unless the phone bench says otherwise (GRAPHICS §7 already allows this for Low). |
| J7 | D28: GPU-driven path on "capable recent" phones | Mobile drivers are the most common place for compute and indirect-draw bugs (Bevy's Adreno 6xx case). | Direct path on every phone until a phone bench shows a gain and a soak test shows no crashes; then allow it per GPU family. |
| J8 | Dynamic resolution rebuilds targets | C3. | Viewport rendering (phase 0.6). |
| J9 | UI rebuilt each frame (D46 slice) | H11; also against AGENTS rule 6. | Kept tree (phase 1.5). |
| J10 | WebGPU as a first-wave platform (D33) | A fourth platform before the second (phones) is measured spreads the work. | Keep the code paths possible (single-threaded mode exists) but move the web build to phase 6. |
| J11 | Procedural materials in the fragment shader | Noise-based patterns are heavy per pixel; fine for greybox, costly on phones. | Textured materials replace them on Low; the procedural ones stay for greybox and tests. |

**J4 measurement** (`engine/physics/benches/determinism.rs`, Apple M5, release bench profile): 24 scripted character controllers, 300 dynamic boxes, 200 static wall boxes, and a static floor, stepped 3,600 times at 60 Hz. Every character move requires its body's ID and excludes that body's colliders. Each run reports mean, p50, and p99 per-step time; total is the sum of timed step durations. Checkpoint hashes are at steps 600, 1,800, and 3,600. Modes: A = default `cross-platform-determinism`; C = `--no-default-features --features simd` (`simd8`). Each mode was run twice with `--report-once`. The 1/5/15-minute system load averages below were sampled at command launch; the 10-logical-CPU machine was heavily loaded, so these timings are not an idle-machine comparison.

| Mode / run | Mean step (ms) | p50 (ms) | p99 (ms) | Total timed steps (ms) | Load average (1 / 5 / 15 min) | Hashes at 600 / 1,800 / 3,600 |
|---|---:|---:|---:|---:|---|---|
| A / 1 | 0.324708 | 0.257959 | 0.930584 | 1168.948 | `42.01 / 29.00 / 21.31` | `0c6fe010509e52da` / `8ed721f8cce5131b` / `f6861ddce81febbf` |
| A / 2 | 0.508647 | 0.262416 | 6.199083 | 1831.130 | `43.74 / 35.94 / 25.30` | `0c6fe010509e52da` / `8ed721f8cce5131b` / `f6861ddce81febbf` |
| C / 1 | 0.259984 | 0.212375 | 0.691792 | 935.942 | `42.16 / 35.74 / 25.29` | `7e015edd5871f36e` / `b2390664d52e2dc9` / `c1b10dc36854224b` |
| C / 2 | 0.401703 | 0.220250 | 3.650666 | 1446.129 | `36.46 / 35.66 / 26.06` | `7e015edd5871f36e` / `b2390664d52e2dc9` / `c1b10dc36854224b` |

Both repeated runs in each mode produced identical hashes. A and C hashes differ because the deterministic and SIMD Rapier modes produce different floating-point results; the hashes changed from the previous table because the old benchmark's movement queries hit each character's own capsule. The loaded-machine timing samples varied substantially, particularly p99. They are recorded for completeness and do not establish a reliable speedup or support a cross-device comparison. The default remains A until the owner decides whether to change D6.

## K. How other engines handle the low-end case

[2026-11-11-engine-comparison.md](2026-11-11-engine-comparison.md) already compares threads, frame pipeline, visibility, draw order, transparency and textures with Bevy, Godot, Unity and Unreal. What matters most for this audit, and is not yet in that file:

- **Godot 4 ships three renderers**: Forward+ (desktop), Mobile (fewer passes, built for tiled GPUs), and Compatibility (OpenGL ES 3 for old devices). The lesson: a phone needs a different pass list, not only lower numbers. Our tiers in §D do this inside one renderer.
- **Unity URP** keeps per-object data on the GPU between frames (the SRP Batcher), so unchanged objects cost almost nothing to draw. This is the idea behind J2.
- **Unreal's mobile renderer** defaults to forward shading with MSAA on phones, and treats deferred as optional. This supports J6.
- **Bevy** moved to a retained render world and binned render phases, and has a GPU-driven path with a fallback; our D28–D29 already follow it, and J2 completes the match.
- **Google's Filament** (a renderer built for Android first) uses clustered forward lighting, a compact `R11G11B10` HDR target, and careful use of tile memory. It is the closest model for our phone tiers (M4, J6).
- **bgfx** sorts every draw by a single 64-bit key and records draws from many threads into separate encoders; our bins and sort keys (D29) are the same idea.
- **The Forge and Wicked Engine** use GPU-driven culling and visibility buffers on desktop; both keep a simpler path for weaker hardware. This supports keeping the Direct path as the default on phones (J7).

Before any of these points becomes a decision, its row in COMPARISON.md should be checked against that engine's current documentation, as that file's rules require.
