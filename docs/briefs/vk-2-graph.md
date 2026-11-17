# vk-2-graph: the render graph on `engine-gpu`, and the core passes running on the Mac and both phones (D82)

Goal: the renderer runs on Vulkan with a render graph that keeps data in tile memory. Done means:
1. `FramePlan` is the front end of a render graph that compiles once per settings change into render passes, subpasses, transient attachments, memory aliasing and barriers.
2. The core passes run on `engine-gpu`: both shadow cascades, the depth prepass, light tiles, the opaque forward+ scene, particles, sky, exposure, composite (tonemap) and UI.
3. `engine-platform` creates the surface and device through `engine-gpu`; `engine-app` drives frames through it.
4. The sandbox and IMAGINE run on the M5 (KosmicKrisp) and on both test phones, with validation clean.
5. Goldens for every tier pass, except the passes listed under "Left for vk-3", whose goldens are skipped by name with that reason.
6. `scripts/gate.sh` passes on the `vulkan` branch.

Starts after vk-1 merges into `vulkan`, and after `device-tier` and `stylized-lighting` merge into `main` (both edit `engine/render`); merge `main` into `vulkan` first. It lands on `vulkan`, not `main`.

Read first:
- `AGENTS.md`; `docs/README.md` §7 D26, D28, D33, D54, D68, D71, D77, D82;
- `docs/RENDERING.md` §3–§7;
- `engine/gpu/src` (all of it);
- `engine/render/src/{frame_plan.rs, targets.rs, tier.rs, shaders.rs, gfx/, scene/}` and `engine/render/shaders/*.wgsl`;
- `engine/platform/src/{gpu.rs, window.rs, presentation.rs, profile.rs}`; `engine/app/src/{client.rs, startup.rs}`.

## Design

- **Layering.** `engine-render` (layer 2) depends on `engine-gpu` instead of wgpu. `engine-platform` uses `engine-gpu` for the surface and builds `GpuProfile` from `GpuCaps`. No other crate names `engine-gpu` types except through `engine-render` and `engine-platform`.
- **The graph** (`engine/render/src/graph/`, `mod.rs` only declares and documents):
  - `desc.rs`: `PassId`, `GraphTexture` (virtual: size relative to the render size or fixed, format, samples), `GraphBuffer`, and `PassDesc { name, kind: Raster | Compute, reads: [Read { resource, how: Sampled | InputAttachment | Depth | Storage }], writes: [Write { resource, load }] }`.
  - `plan.rs`: today's `FramePlan` builds the pass list from `Settings` and keeps its checks (a pass may not read what no pass wrote; D68 depth ownership). It now emits `PassDesc`s.
  - `compile.rs`: `compile(&[PassDesc], &GpuCaps) -> Result<CompiledGraph, RenderError>`. Rules, in order: (1) order passes as planned; (2) merge consecutive raster passes into one Vulkan render pass when they share size and samples and every read between them is an input attachment or depth (same-pixel reads); (3) a texture whose last reader is inside the same render pass is transient (`Store::DontCare`, lazily allocated); (4) textures with non-overlapping lifetimes share memory; (5) barriers between render passes from the reads and writes. Compiled once per `set_settings`; never per frame.
  - `execute.rs`: per frame, walks the compiled graph and calls each pass's record function with a `CommandList`. Zero CPU allocation after warm-up.
  - `report.rs`: a text dump of the compiled graph (render passes, subpasses, transient and aliased targets, barriers) for the dev overlay and tests.
- **What merges on phones (Low and Medium).** Opaque scene → particles → sky → composite → UI become one render pass with subpasses: HDR colour and MSAA depth never leave the tile; MSAA resolves in the render pass. Shadows and light tiles are their own passes. Write this as a test (below), not only a hope.
- **Shaders.** Port every WGSL file used by these passes to Slang under `engine/render/shaders/`, with tiers as specialization constants. The cook (vk-1) writes them into each game's pack under `engine/shaders/`; `engine-render` loads them through `engine-assets` at start-up, builds every pipeline before the first frame, and saves the pipeline cache through `engine-platform`'s config store. The `hot-reload` dev feature reloads changed shader records from the pack watch (D72), not from files.
- **Instances and meshes.** Keep the retained scene exactly: 64-byte instance slots, the split position and attribute streams, dirty-range uploads, per-mesh slot index ranges. Only the upload and bind calls change.
- **Platform and app.** `engine/platform/src/gpu.rs` and `window.rs` create the `engine-gpu` instance, device and surface; the D45 frame-rate binding stays after swapchain creation. Surface loss and resume follow D73: a lost surface is a state; resume recreates the swapchain and keeps every other resource.
- **Left for vk-3** (removed from `FramePlan` on this branch, and their goldens skipped by name): GTAO and its denoise, TAA, bloom, the governor's dynamic resolution, and the buffer view modes that need them.
- **What gets deleted.** wgpu from `engine-platform`; the ported WGSL files; `engine-render`'s wgpu setup and pipeline code for the ported passes. wgpu stays only in the files vk-3 ports.

Tests (GPU tests skip with a printed reason when no Vulkan device exists):
- Graph compile, table-driven: for each tier, the expected render passes, subpasses, transient targets and aliasing; the Low/Medium merge above is asserted.
- Compile rejects: a pass reading an unwritten target; a merge that would sample a neighbouring pixel through an input attachment.
- `execute` allocates nothing per frame after warm-up (counting allocator, 1,000 frames of a moving scene; extends `scene_cull_no_alloc`).
- Existing headless tests keep passing on Vulkan: retained scene, depth coverage at every tier with prepass on and off (D71), UI gradient, debug views that remain.
- Goldens (D77) for the sandbox and IMAGINE at every tier, compared with the existing references; re-record only with a written reason per image in the report.
- Validation log empty in every GPU test.
- Device run: `scripts/android-test.sh engine-render` passes on both phones; the sandbox APK starts, plays, suspends and resumes on both.

Do not touch: simulation crates, `engine/ui` logic, kits, game logic. Game crates change only where they named wgpu types (they should not).

When a detail is not specified: pick the simplest correct design that keeps one code path, state it in the report, and continue. Stop only if the goal itself cannot be met.

Docs in the same change: this brief's status; RENDERING §6 gains the graph rules and the compiled pass table per tier; ARCHITECTURE §1 rows for `engine-render` and `engine-platform`.

## Gates
scripts/gate.sh
