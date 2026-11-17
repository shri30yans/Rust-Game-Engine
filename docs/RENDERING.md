# Rendering: the GPU layer, the frame and every pass

The single source of truth for everything the player sees in the 3D world: the GPU layer, the render graph, the frame on each tier, and the design of each pass with its reasons. Code: `engine/gpu` (`engine-gpu`) and `engine/render` (`engine-render`). Principles behind it: [PRINCIPLES.md](PRINCIPLES.md) §2–3. Budgets and measurements: [PERFORMANCE.md](PERFORMANCE.md). The evidence for the targets marked "planned" is in [research/2026-11-17-render-mobile.md](research/2026-11-17-render-mobile.md) and [research/2026-11-17-render-deep-dive.md](research/2026-11-17-render-deep-dive.md).

Status words: **built** (in the code, tested), **planned** (designed here, a brief exists or will), **proposed** (needs the owner's yes), **gated** (built only after a measurement or hardware says so).

## 1. Goal and honest scope

**Unreal-5-level image quality, at 60 fps on mid-range phones and 120 fps where the tier allows.** Most of the "Unreal look" comes from lighting, anti-aliasing, materials and post done correctly in physical units. We do not port Unreal's headline systems; we take what works on a tiler.

| Unreal 5 | What it gives | Ours | Why |
|---|---|---|---|
| Lumen | bounced light that changes with the world | **light cache** (§13): precomputed visibility, dynamic light, solved on a CPU job thread | Lumen is not supported on Unreal's mobile renderer; precomputed visibility is the only GI design with a phone track record (Enlighten, Frostbite) |
| Nanite | dense geometry, no visible LODs | **cluster culling and cluster LOD** (§8) with hardware raster in the main pass | on a phone Nanite's win is culling and LOD; its visibility buffer and compute shading fight tile memory |
| Virtual shadow maps | sharp shadows everywhere | **two fitted, cached cascades** in one atlas, plus horizon maps and capsules (§11) | Unreal mobile caps at 2 cascades; fitting and caching shipped on phones |
| TSR | stable edges, upscaling | **MSAA on tile** on phone tiers; **fragment TAAU** on High, behind one upscaler contract (§16) | MSAA resolves on chip almost free; TAAU fits in about 1 ms |
| Niagara | rich effects | GPU-simulated particles, six-way lit smoke, effect files (§14) | authored textures sell effects at sprite cost; overdraw is the limit |

**Art direction belongs to each game.** The renderer has no look of its own. Each game sets its style, light temperatures, colour grade (a lookup table it hands in), destruction look and readability rules in its own docs (IMAGINE: [LOOK.md](../games/imagine/docs/LOOK.md)). Every game gets the same tools: physical units, auto exposure, a grade LUT per game or season, and effect files.

## 2. The GPU layer: `engine-gpu` on Vulkan

**Decision.** The renderer talks to the GPU through our own Vulkan layer, `engine-gpu` (layer 0), built on `ash`. wgpu and naga are removed. (Decided by the owner, 2026-11-17. Planned: briefs [vk-0](briefs/vk-0-phone-baseline.md) to [vk-3](briefs/vk-3-port.md).)

- **The only `unsafe` crate.** `engine-gpu` is the only crate that names `ash` and the only one allowed `unsafe` (beyond the Android entry and logging exceptions in [ARCHITECTURE.md](ARCHITECTURE.md) §4). `unsafe` lives only under `src/vk/`, every block has a `// SAFETY:` comment, and every public item is safe to call. Handles are generation-checked ids into the device's slot tables (`engine-core::Slots<T>`; `engine-gpu` → `engine-core` is an allowed same-layer pair), so a handle cannot outlive the device, and destruction is deferred until the GPU finishes the frames that used it.
- **Vulkan-shaped, Vulkan-free API.** Typed handles for buffers, textures, samplers and pipelines; render passes with subpasses; command lists; frames in flight. No Vulkan type appears in the public API, so the iOS Metal backend later replaces the internals, chosen per target at compile time (no run-time backend trait).
- **Platforms.** Android: Vulkan 1.1 plus a fixed required-feature list (Android Baseline 2022 covers 86.6% of active devices), checked once at device creation; a device missing one gets a clear `Unsupported` error naming the features, never a degraded path. Windows and Linux: native Vulkan. macOS (the dev Mac): **KosmicKrisp**, LunarG's conformant Vulkan-on-Metal driver; MoltenVK only if a KosmicKrisp bug blocks work. iOS: a native Metal backend inside `engine-gpu`, after Android is measured. Web: dropped until a game needs it. Consoles: further backends.
- **Tile memory is first class.** Passes that read only the pixel they write merge into one Vulkan render pass with subpasses and input attachments. Attachments nobody reads afterwards are transient, lazily allocated where the device offers it, stored `DONT_CARE`. This is core Vulkan 1.0, so it is one path on every phone and on KosmicKrisp.
- **Descriptors** in sets by update rate (frame, pass, material, draw) plus push constants of 16 bytes or less. No bindless at first: unreliable on Vulkan 1.1-class phones.
- **Pipelines.** One plain `VkPipeline` per cooked permutation, every one built at load, warmed into a `VkPipelineCache` saved through `engine-platform`'s config store. Zero shader compiles during play. No pipeline libraries or shader objects (no documented vendor support on phones).
- **Shaders in Slang,** compiled by `tools/cook` to SPIR-V with reflection (`ShaderModuleData { name, stage, spirv, reflection }`) into the game's pack under `engine/shaders/`. Tiers are specialisation constants. WGSL is removed.
- **Memory:** `gpu-allocator` (pure Rust) sub-allocates large blocks; nothing allocates GPU memory in steady play.
- **Synchronisation:** two frames in flight, fences and binary semaphores (timeline semaphores are not guaranteed on 1.1). Barriers use narrow stage masks so Mali overlaps the next pass's vertex work with the previous pass's fragment work.
- **Measurement:** a timestamp pair and a debug label per pass, so AGI, Arm Performance Studio and Xcode show named passes.
- **Safety net:** Vulkan validation and synchronisation validation run in debug builds and in every GPU test; any message fails the test. Driver workarounds live in one GPU-family table in the device profile.
- **Migration with one renderer at a time.** vk-1 to vk-3 merge into a `vulkan` branch; it merges into `main` only when every golden matches and the full gate passes, and wgpu is deleted in that merge.

**Why.** wgpu hides exactly the controls that matter on tilers: subpasses and input attachments, transient attachments, explicit barriers, memory placement and aliasing, specialisation constants, the pipeline cache's lifetime. Unreal, Godot and Filament each own their GPU layer for this reason. The switch is bounded: wgpu appears in `engine-render` (about 1,000 call sites in 29 files) and two files each of `engine-platform` and `engine-app`.

**Rejected.** Keeping wgpu (its portability costs those controls). A Metal backend now (doubles backend work before Android is measured). VMA (a C++ build for what `gpu-allocator` does in Rust). `VK_KHR_dynamic_rendering_local_read` as the main path (core only in 1.4, 28% of Android devices; subpasses work everywhere). A run-time backend trait.

**Later, each only with a measurement:** command recording on the job pool, async compute, local read as a capability tier, bindless, GPU-driven culling.

## 3. The render graph

**Decision.** `engine-render` builds one immutable frame plan whenever settings change; it is the front end of a render graph compiled once into render passes, subpasses, transient attachments, memory aliasing and barriers. Per frame it only records. (Built: the checked `FramePlan`. Planned: the compiler, [vk-2](briefs/vk-2-graph.md).)

- **Passes declare reads and writes.** Construction rejects a pass that reads what no pass writes, before any GPU resource exists (Unreal's Render Dependency Graph validation).
- **Features imply their inputs.** A setting that needs a producer brings it: `Prepass::On` owns its GTAO and TAA consumers, so asking for either produces the depth, normal and velocity targets it reads. Independent booleans that allow a consumer without a producer are not representable.
- **Depth ownership comes from the plan.** `OpaqueDepth::Own` (opaque pass writes depth, `Less`) or `OpaqueDepth::FromPrepass` (opaque pass uses `LessEqual`, no write). The prepass and the opaque pass share one invariant clip-position function, one quantised-position decoder and the same instance bindings, with invariant position outputs, so equal-depth fragments pass. The first-person layer always owns its depth. `ViewMode::OpaqueCoverage` is the regression check, run at every tier with the prepass on and off.
- **Compile rules, in order:** keep the planned pass order; merge consecutive raster passes into one Vulkan render pass when they share size and samples and every read between them is an input attachment or depth; a texture whose last reader is inside the same render pass is transient; textures with non-overlapping lifetimes share memory; barriers between render passes come from the reads and writes.
- **Errors, not warnings:** an on-tile attachment read other than as an input attachment or depth inside its render pass; a subpass whose colour plus depth exceeds 128 bits per pixel, counting every MSAA sample (Mali's tile shrinks above it); render targets created with flags that disable framebuffer compression (STORAGE, TRANSFER_DST, MUTABLE_FORMAT, ALIAS on colour targets that could otherwise be AFBC or UBWC compressed).
- **Order inside a pass by class:** compatible opaque draws first, then everything that would disable Mali's fragment pre-pass or Adreno's LRZ (masked late-Z, tile reads, helper-lane shaders).
- **Compute sits before the first render pass of the frame**, never between fragment passes. Per-pixel post stays in fragment subpasses.
- **Report:** a text dump of the compiled graph (render passes, subpasses, load and store ops, transient and aliased targets, barriers) for the dev overlay and tests. A test asserts that Low and Medium run opaque → particles → sky → composite → UI as one render pass.

**Why.** Load and store ops derived from usage cut Godot's external memory writes about 300×; merged subpasses use about 55% fewer tiles on Mali-G76. Unity URP and Godot treat these as settings a project can silently defeat; here the compiler makes them structural.

**Rejected:** a general graph scheduler with pass culling before it is needed; dynamic rendering as the base.

## 4. Frame anatomy per tier

The target frame. Every millisecond is an **estimate** extrapolated from published desktop, console and older-phone figures; none is measured on the test phones yet. The bench measures each row before any tier's feature set is frozen ([PERFORMANCE.md](PERFORMANCE.md) §3).

| Tier | Example hardware | Display and AA | Target | GPU budget |
|---|---|---|---|---|
| Low | Adreno 6xx, Mali-G5x/G7x | about 720p internal, 4× MSAA | 60 fps | ~11 ms |
| Medium | the two test phones | 0.6–0.7 scale of 2800×1280 or 2400×1080, 4× MSAA | 60 fps | ~11 ms |
| High | Adreno 8xx, Immortalis-G925, Apple A17+ | about 0.6 scale with TAAU | 60 fps, all effects | ~13 ms |
| Desktop High/Ultra | the M5 | native or TAAU | 60+ fps | ≤ 12 ms at 1440p |

A **120 fps mode** on Medium or High uses the Medium anatomy at a lower render scale, drops raymarched clouds and refreshes the far shadow every other frame, for about 6 ms of GPU. CPU work (culling, light binning, light-cache solve, vegetation setup) runs on the job pool.

| Pass | Low · est. ms | Medium · est. ms | High · est. ms |
|---|---|---|---|
| Early compute (before any render pass) | sky-view LUT, amortised transmittance · 0.1 | + aerial-perspective and fog froxel volume · 0.4 | + local fog and dust · 0.5 |
| Vegetation placement, particle sim | CPU lists · 0 | GPU placement per dirty tile + sim · 0.3 | denser · 0.4 |
| Sun shadow (one D16 atlas) | 1 cascade 1024², static cached + dynamic overlay · 0.8 | near 2048² each frame + one dirty far tile · 1.3 | + 2–4 far tiles · 1.6 |
| Local-light shadows (same atlas) | 0–2 cached spots · 0.1 | ≤ 2 updates · 0.3 | ≤ 4 updates · 0.5 |
| Depth / motion prepass | masked-only, with alpha-to-coverage · 0.4 | masked-only · 0.6 | full depth + motion, stored · 0.9 |
| Half-resolution AO | — | — | visibility-bitmask GTAO · 1.0 |
| **Main render pass:** opaque forward (tile + z-bin lights), masked EQUAL, terrain, impostors | Filament BRDF, specular AA, SH + 1 probe, 2×2 PCF, height fog · 5.5 | + clearcoat/sheen permutations, 2 probes, Castaño PCF, froxel fog · 5.8 | + rotated PCF, local exposure inputs · 5.2 at lower res |
| ↳ sky subpass | sky-view fetch + 2.5D clouds · 0.2 | · 0.3 | · 0.3 |
| ↳ translucent and effects subpass | sorted premultiplied, six-way smoke, wire-safe ribbons · 0.8 | + soft particles via depth input · 1.0 | · 1.0 |
| ↳ MSAA resolve | on tile, tonemap-weighted · ~0.1 | · ~0.2 | — |
| Raymarched clouds | — | quarter-res band · 0.7 | + shadow map · 1.0 |
| TAAU resolve (fragment) | — | — | YCoCg clip, 32-bit history · 1.0 |
| Bloom | dual filter, 4 mips, R11G11B10F · 0.5 | 5 mips · 0.6 | 6 mips + lens ghosts · 0.7 |
| Exposure | histogram from a bloom mip · <0.05 | · <0.05 | + local exposure · 0.1 |
| Composite + UI | one LUT (tonemap + grade), dither, vignette · 0.4 | + haze · 0.5 | + sharpen, motion blur gather · 0.8 |
| **Estimated total** | **~8.9** | **~12.0** (over budget: drops clouds or scale first) | **~15.0** (over budget: drops local exposure, AO or scale first) |

The totals over budget are the honest result of stacking estimates; thermal-driven dynamic resolution (§15) is part of the frame design, not an extra.

## 5. The retained scene and GPU data

**Decision (built).** Following Unreal's `FScene` and primitive proxies, a game registers each drawable once (`RenderScene::add` with a `PrimitiveDesc`) and later updates only what changed (transform, look, visibility) through a generation-checked `PrimitiveId`. `Layer` selects world, first-person or glow. There is no immediate-mode draw call.

- **Records.** The instance record is 64 bytes: three affine `vec4` rows (48), RGBA8 tint (4), f16 emissive (2 plus padding), eight packed material and flag bytes. Previous transforms are a separate 48-byte stream, bound only when motion vectors are on. Procedural vertices are a 12-byte position stream and a 12-byte attribute stream (octahedral `snorm16x2` normal, RGBA8 colour, RGBA8 material). Cooked meshes use Float32 or Quantized16 positions with bounds records. Shadow and depth passes bind positions only; the prepass rebuilds its normal from position derivatives.
- **Uploads.** CPU records are cached per slot; only coalesced dirty ranges upload. A static scene uploads 0 bytes after the first frame.
- **Culling output.** The renderer culls the dense struct-of-arrays scene each frame, writes visible slot ids into reused per-mesh ranges of one grow-only `u32` buffer, and shaders read `instances[visible_indices[instance_index]]`.
- **Resources.** Meshes, textures and materials live in one generation-checked slot table type (`Slots<T>`, distinct typed ids per kind). A resource cannot be removed while a primitive or material refers to it; a texture's use count makes the check constant time. `ClientApp` owns the asset store, the uploader and the job pool. Texture transcoding (`prepare_texture`) is a pure job on the pool; uploads run at the frame boundary, first in first out, under a byte budget (8 MiB desktop, 2 MiB phone). A pack is ready only when its whole GPU representation exists; a reload builds a full replacement, re-points primitives, and frees the old resources after the swap; a count mismatch keeps the last good pack.
- **Descriptor groups** (today's wgpu layout, carried into the Vulkan sets): view resources, material, probes, free.

**Why.** Cost follows what changed, not world size. Smaller records cut memory bandwidth, the main power cost on phones. **Rejected:** immediate mode with smarter batching (re-sends every static object every frame).

## 6. Visibility and culling

**Decision.** Visibility is computed every frame from the current world, never precomputed. **CPU culling over the struct-of-arrays scene is the one path** (built), made excellent and feeding indirect draws; GPU culling, if it ever comes, writes the same indirect buffer.

- **Order of rejection:** grid cells against the frustum ([ARCHITECTURE.md](ARCHITECTURE.md) §5), then instance bounding spheres, then distance and projected size (visibility ranges, §8) with a minimum size for gameplay-critical objects. (Planned: NEON plane tests on four spheres at a time, split across the job pool.)
- **Shadows** (built: per-cascade lists): each cascade culls casters against its own box. Planned: fit each cascade to the visible receivers and cull casters against that box swept toward the light.
- **Submission** (planned): one `vkCmdDrawIndexedIndirect` per pipeline bin. Single indirect draws with `firstInstance` reach 98% of Android devices; multi-draw-indirect only 70% and draw-count 68%, so neither is required.
- **Occlusion (gated)** only if city benches justify it, and **never by splitting the main pass**: either a reversed-Z minimum pyramid built from the finished depth and tested next frame with motion-inflated bounds (falling back to "visible" on camera cuts and new objects), or a low-resolution occluder-box pass before the main pass.

**Why.** Desktop two-pass occlusion stores depth mid-frame and reloads it, which costs over 0.5 ms on Adreno for the split alone and discards LRZ; GPU-driven submission without culling cost 18% on Adreno 650; the best tiler occlusion gained only 5% outdoors. Bevy disabled GPU preprocessing on Adreno 6xx.

**Rejected:** two-pass HZB on tilers; hardware occlusion queries (late or stalling); conditional rendering (30% coverage); per-triangle compute culling (duplicates the binner); mesh shaders (6% of Android devices); precomputed visibility, BSP, portals and baked occluders (wrong as soon as a wall falls).

## 7. Surface classes and draw order

The engine never asks at run time whether something is transparent. **Each cooked material section has a class, and the class chooses the pass.** (Planned: a cook stage and pass families; revises the old "opaque, alpha-tested included" pass.)

```rust
enum SurfaceClass { Opaque, Masked { cutoff: UnitAlpha }, Translucent { blend: TranslucentBlend } }
enum TranslucentBlend { Premultiplied, Modulate }   // premultiplied covers "over" and additive
```

- **The cook decides from the pixels,** with the glTF `alphaMode` only as a hint. It computes effective alpha (base factor × texture × vertex alpha) over the texels the mesh's UVs touch. All opaque → `Opaque`, whatever the file says, and the alpha channel is stripped (a smaller texture format follows). Bimodal → `Masked`. Continuous → `Translucent` only if declared BLEND, otherwise `Masked` with a cook warning. Every promotion and demotion is logged. An override file may force a class; forcing `Opaque` on a texture with holes is a cook error unless acknowledged.
- **Masked triangles are split.** For each masked triangle the cook rasterises its UV footprint over the alpha texture: fully solid triangles move to an `Opaque` sub-section, fully empty ones are deleted, only mixed ones keep `discard`. Alpha mips preserve coverage (Castaño's rescale), possible because the cutoff is a cooked constant. Normal-variance roughness mips are made in the same stage.
- **Pass families are fixed.** Opaque shaders are compiled with no alpha input. Masked geometry gets a depth-only prepass with discard (alpha-to-coverage on MSAA tiers), then a colour pass with `EQUAL`, no depth write and no discard. Then sky, then translucent. A draw in the wrong family cannot be expressed.
- **Opaque, prepass and shadow work is binned** (built): bins keyed by pipeline, material, mesh; only bin keys are sorted; each bin is one draw. Front-to-back order of bins (by the nearest visible instance) is a **per-device policy** chosen at bind: on for Mali-G720-class and Adreno (LRZ), off where Mali's fragment pre-pass makes order irrelevant.
- **Translucent work is sorted** back to front by a 64-bit key: material sort priority, then distance, then instance id (no flicker), with a radix sort into pre-sized buffers.
- **Fading LODs** are binned into the masked family only while they fade (a dither is a discard).

**Why.** On Mali, `discard` forces late Z and can disable hidden-surface removal; on Adreno a discarding draw cannot feed LRZ and alpha test costs as much as blending. A wrong class costs overdraw elimination for that draw and everything behind it. Godot infers transparency from shader code and ships a long tail of sorting bugs; Unreal and Unity make the class a material property. We make it a checked cook output.

**Pass order:** shadows → depth/motion prepass (masked-only on MSAA tiers, full on TAAU tiers) → main render pass (opaque → masked EQUAL → terrain and impostors → sky → translucent and effects → MSAA resolve) → TAAU (High) → bloom → composite with UI. First-person geometry draws with its own FOV and depth range so it never clips into walls.

## 8. Geometry: instances, LOD, clusters and stand-ins

- **Instances, not merged meshes** (built). Every movable or breakable piece is its own instance; breaking one changes a few records. Geometry that can never break is merged per grid cell by the cook.
- **Visibility ranges** (planned) are one generic mechanism for LODs, small-prop cut-off and far stand-ins: every renderable may have start and end distances with a fade margin and a parent stand-in, scaled per device and tier (how phones get a shorter draw distance). This follows Godot's visibility ranges and Bevy's `VisibilityRange`.
- **Mesh LOD** (built): meshopt-generated LODs at 50% and 25% of source triangles, chosen per primitive with hysteresis; shadows use one LOD coarser.
- **HLOD:** one simplified stand-in per intact building is the parent of its pieces; the first damage turns it off for good (no re-merging, no hitch).
- **Clusters in three stages, one pipeline** (planned): **V1** cooks meshoptimizer clusters (64 vertices, up to 124 triangles, sphere and cone bounds), per-cluster quantised positions and meshopt disk codecs; the CPU culls whole clusters into the same index ranges. **V2** adds the cluster-LOD DAG (partition, simplify with locked borders, re-cluster, as in meshoptimizer's `clusterlod.h`) with fixed-size streamed pages. **V3** (gated on 64-bit image atomics and a measurement showing sub-pixel triangles dominate) is a visibility buffer with a compute rasteriser for small clusters. Cluster content avoids world-position offset and alpha masks, or turns them off with distance.
- **Vertex formats** (built, extended by V1): positions as 3×16-bit integers relative to bounds in their own stream (Mali shades position first and runs the rest only for visible vertices), normal and tangent sign octahedral, UV0 as 2×16-bit: about 12–14 bytes per vertex against 32–48 for floats.

**Rejected:** re-merging meshes after damage (hitches); Frostbite-style destruction masks on merged meshes (we use pre-broken chunks); GPU strip decoding (needs a compute pass without mesh shaders).

## 9. Textures

- **One compressed source, transcoded per device** (built). Textures cook once to KTX2 with Basis Universal; at load they become BC7/BC5 on desktop, ASTC on phones and Apple GPUs, ETC2 as fallback, chosen from the device's reported formats.
- **Block size by role** (planned; revises "ASTC 4×4 everywhere"): Basis v2 ASTC LDR or XUASTC at 6×6 for albedo, 6×6 two-channel normals, 8×8 or larger for packed occlusion-roughness-metal and masks. Every ASTC block is 128 bits, so 4×4 is 8 bits per pixel, 6×6 is 3.56 and 8×8 is 2: an inferred 2.2–4× cut in texture memory and bandwidth from a cook setting. Desktop BC7 quality after XUASTC transcode needs a test first.
- **Sampling** (planned): `VK_EXT_astc_decode_mode` set to unorm8; bilinear with 2× anisotropy rather than trilinear (trilinear costs twice bilinear on Mali); 16-bit samplers.
- **Mip streaming** (planned): each device has a hard texture budget from its profile. The cook stores texel density per material; the renderer computes each texture's wanted mip from the screen size of its visible instances; a pool keeps wanted mips within budget, dropping the farthest first. The smallest mips are always resident; uploads have a per-frame byte budget. The cook reports texture memory per map and device class, and CI fails a map whose always-resident part does not fit the smallest device.
- **Few unique texels:** low-resolution unique masks driving 2–4 tiling layers from a texture array, trim sheets, detail maps and wear masks. Fully vertex-coloured models are fine for stylised games.

**Rejected:** neural texture compression at sample time (0.5–0.7 ms even on an RTX 5070, no phone figure); virtual texturing and sparse residency (not guaranteed on the baseline, no mobile costs found); separate per-platform texture packages; atlases for world materials (they break mips and wrap; atlases are for UI and decals).

## 10. Shading

(Planned unless marked; the forward HDR renderer with metal/roughness PBR is built.)

- **One BRDF:** Filament's standard model (GGX, Smith height-correlated visibility, Lambert diffuse), fp16-safe, with multiple-scattering energy compensation from the DFG LUT. Extra lobes (clearcoat, sheen, transmission for glass) are **cook-chosen permutations** as specialisation constants, never runtime branches.
- **Specular anti-aliasing** at the cause: normal-variance roughness in the mips at cook, plus geometric specular AA in the shader.
- **Image-based light:** SH ambient plus split-sum IBL from the light cache's probes, with specular and horizon occlusion; pre-exposure keeps values in fp16 range.
- **Precision and occupancy:** fp16 is the default in fragment shaders; varyings go out as fp16 where 16-bit storage is reported. A **shader gate** in CI runs the Mali Offline Compiler on every shader: no register spills, at most 32 registers on hot shaders, and no cycle regressions against a recorded baseline. No helper-lane or subgroup operations in opaque fragment shaders.
- **Materials:** textured PBR sets from Blender (colour, normal, packed roughness/metal/occlusion), trim sheets, detail maps, wear masks (vertex colour). Broken surfaces have no UVs, so they use triplanar mapping of the material's interior. Damage is a material parameter (a crack mask that grows with health). Procedural patterns stay for greybox and tests only.

## 11. Shadows

**Decision (planned; revises "4 cascades with PCSS").** Two fitted cascades in one D16 depth atlas, the far part cached and scrolled in tiles, filtered with Castaño's 9-tap PCF.

- **Fit to what is visible:** each cascade fits the previous frame's visible receivers (sample-distribution style), texel-snapped so it does not shimmer.
- **Cache:** static casters in the far cascade are cached in tiles and scrolled; only dirty tiles redraw (destruction marks them). Built today: per-cascade culling and a reused far layer while its light matrix and caster bytes are unchanged.
- **Filter:** Castaño PCF on Medium, rotated PCF with a blocker-scaled radius on High, 2×2 PCF on Low.
- **Depth-only pipelines** with empty fragment shaders; opaque and masked casters in separate bins.
- **Beyond the cascades:** horizon maps for terrain self-shadowing, capsule proxies for character soft shadows, a cloud-shadow texture.
- **Local lights:** a few shadowed spots and octahedral omni maps in the same atlas, chosen by screen size and importance; cached when static.
- **Gated:** ray-query near-field sun shadows replacing the near cascade on phones that report ray query.

**Why.** Unreal's mobile renderer caps at 2 cascades; fitted, cached cascades shipped on phones (Roblox); PCSS and EVSM have no phone numbers. Shadows are the largest bandwidth cost after the main pass.

## 12. Lights and reflections

- **Lights** (planned; replaces GPU 3D clusters and lifts the built 32-light tile cap): a CPU-built **tile plus z-bin** list (Drobot): lights sorted by depth into z-bins, each bin holding a min/max light index; screen tiles hold N×32-bit light masks. Built on the job pool, so no compute barrier. Decals and reflection probes share the same grid. Lights use photometric units with a radius derived from intensity, and a light LOD for dense cities. Stochastic lighting is gated on a measured clumping problem.
- **Reflections:** at most two box-projected reflection probes per pixel, **normalised by the light cache's irradiance** so they follow dynamic light; placed by the cook or by markers in Blender, relit a few per frame. Planar reflections only for High-tier water. **No SSR on phone tiers**: it needs a resolved depth and colour copy, ending tile residency, and Unity and Epic exclude it on mobile.
- **Ambient occlusion:** visibility-bitmask GTAO at half resolution on High only (built: GTAO at half resolution, 0.58 ms on the M5).

## 13. Indirect light: the light cache

**Decision (planned: light-cache-1 to 3).** Bounced light is a **cache with precomputed visibility and dynamic light**, after Enlighten and Frostbite's shipped precomputed GI.

| Layer | What | Where | Changes at run time |
|---|---|---|---|
| Direct | sun, sky, point and spot lights in physical units | GPU, forward | everything |
| Light cache (indirect diffuse) | surfels on static geometry, generated at cook; precomputed visibility links surfel→surfel and surfel→probe (top K per receiver); a probe grid (SH L1) with validity | cook: `tools/bake`; run time: `engine/light` on a job thread | sun, local lights, emissives, sky, arena layouts (one link set per layout). Bounces accumulate one iteration per update. |
| Reflections | box probes relit from the surfels (§12) | GPU, a few per frame | yes |
| Dynamic objects | sample the probe grid | GPU | yes |

- **Lighting just works.** The cook places surfels, probes and reflection probes and invalidates probes buried in walls. Artists may add probe volumes or reflection markers but never have to.
- **Destruction** (the cache's one weakness): when a piece breaks, zero the links and probes inside its bounds and mark its surfels non-emitting (local invalidation, pulled early); debris and dynamic objects are receivers only. A traced surface cache is a later tier for ray-tracing phones only.
- **Size risk:** link data is surfel count × K per layout; light-cache-1 measures it before content scales, along with the CPU solve time on a big and a little core.

**Why.** Lumen is not supported on Unreal's mobile renderer and has no phone milliseconds; every fully dynamic design traces rays into a surface cache and none has a phone number (EA's surfels started at 6–7 ms on PS5-class GPUs; Unity abandoned its probe-propagation design). Precomputed visibility gives dynamic light at a fraction of the cost, needs no lightmap UVs, runs on an idle CPU core, and handles time of day and moving lights for free.

**Rejected:** fully baked lightmaps (freeze all light, need lightmap UVs); Lumen-style screen and distance-field tracing (too heavy); DDGI (needs ray tracing or a distance field); the earlier plan of probes updated by tracing a voxel copy of the map every frame (unaffordable on phones).

## 14. Sky, atmosphere and effects

- **One sky path:** Hillaire's sky model (transmittance, multiple-scattering and sky-view LUTs, amortised), with aerial perspective.
- **One froxel volume** for aerial perspective, height and volumetric fog, light shafts and dust: injected, lit by the sun with shadows and by local lights. Destruction injects dust that settles over seconds. Low tier uses height fog only.
- **Clouds per tier:** a 2.5D layer on every tier; a quarter-resolution raymarched band with a shadow map on Medium and High (flight breaks the usual temporal amortisation, so this is designed per tier).
- **Effects:** particles simulated on the GPU (gravity, drag, wind, curl noise, depth collisions); emitters sorted on the CPU, particles inside each emitter sorted. Lit sprites, **six-way lit smoke** flipbooks, motion-vector flipbooks, ribbons (tracers, trails) with wire-safe anti-aliasing, mesh particles. Flipbook cutout hulls are cooked to cut overdraw. **Overdraw budgets are validated data** per effect.
- **Effect files:** each effect is a data file listing layers (particles, a short light, decals, camera shake, a sound cue), hot-reloaded. Camera shake uses a decaying "trauma" value scaled by distance; reduce motion turns it off.
- **Decals:** one atlas, a hard cap (oldest fade first), in the light grid.
- **Destruction visuals:** large chunks are physics bodies drawn as instances; collapse animation is baked rigid-body vertex animation (VAT) for cosmetic pieces; pooled debris; dust goes into the froxel volume.
- **Translucency tiers** (gated by capability, chosen once): soft particles read depth as an input attachment in the same render pass; weighted blended OIT for smoke volumes where measured (its accumulation targets are transient); a 2–4 layer blend in tile memory (MLAB) where raster-order attachment access or local read is reported, imageblocks on Metal later. Glass refraction reads a copy of the opaque image, which ends a render pass, so it exists only where a tier's budget allows.

**Rejected:** per-pixel linked lists and depth peeling (memory and tile cost); weighted blended OIT for glass (wrong for fairly opaque surfaces).

## 15. Devices, tiers, dynamic resolution and pacing

- **Device class and tier are separate choices.** `engine-platform` builds a device profile at start-up (class, performance-core count, memory budget, GPU capabilities, known-bad list match). The engine picks the starting tier from a table by device (phones start on Low or Medium, never desktop defaults); a short benchmark on first launch moves it up or down; a saved choice overrides both. Every change goes through one `set_settings`. (Planned: device-tier brief. Today phones start from desktop defaults, a known defect.)
- **A tier changes only presentation,** never simulation. Tiers choose numbers and specialisation constants inside the one path.
- **Dynamic resolution** (built): a pure `Governor` takes wall frame times (no clock, no GPU reads). Every 30 frames it averages; a window over target × 1.10 lowers the scale one 0.05 step (vsync reports about 16.7 ms on a perfect 60 Hz display, so only a clear miss counts). Recovery is by probing: after four met windows it tries one step up; a missed probe steps back and doubles the wait (cap 64 windows). Targets are allocated once at the largest scale and passes render into a viewport, so a change rebuilds nothing and keeps TAA history. Phone builds use a 70% floor.
- **Pacing and thermals** (planned): rotate in the final pass using the surface's current transform (no compositor rotation); pace with `VK_EXT_present_timing` (Android 17+) falling back to `VK_GOOGLE_display_timing` or Swappy; open an ADPF performance-hint session for the game and render threads; let thermal headroom drive the resolution scale before the OS throttles. The display rate is declared once per surface session (`PresentationBinding`, built).

## 16. Anti-aliasing, upscaling and post

- **MSAA 4× resolved on tile** is the base on phone tiers (built on Low and Medium). Alpha-to-coverage is used only in the masked depth prepass. Sub-pixel content (wires, tracers, fences) is fixed as geometry: wire-safe ribbon widths and alpha fade below a pixel.
- **TAA / TAAU on High** (built: TAA with jitter, motion vectors from the prepass, Catmull-Rom 5-tap history, cross-shaped closest-depth search; it needs the prepass, so turning it on without one is a typed error, and it renders at 1× sample). Planned: extend it into a **fragment-shader TAAU resolve** writing a render target (fragment variants run about half the cost of compute on Adreno and keep framebuffer compression), YCoCg variance clipping, 32-bit history, mip bias, one blue-noise source shared by every stochastic effect.
- **The upscaler contract** (planned): every frame produces render-resolution HDR colour and depth; dejittered, max-dilated motion vectors (prepass, skinned meshes, particles, ribbons with previous control points); exposure; an R8 reactive mask written with MAX blend by the translucent pass; a transparency mask; jitter, render size and maximum render size. With it, MetalFX, Arm NSS and Adreno Neural Fusion become optional backends behind one upscale node chosen at device bind. HUD and thin additive tracers draw after the upscale at display resolution.
- **Post chain:** bloom at half resolution and below (dual filter on Low, Call of Duty-style on Medium and High, R11G11B10F); histogram exposure from a bloom mip; **one composite pass** with a single baked LUT holding the tonemap (AgX or Tony McMapface) and the game's grade, blue-noise dither, vignette, sharpening on High. UI draws last in sRGB-space blending (§17). Depth of field only when scoped or in menus; motion blur off by default.
- **Scene colour format** is a measured tier choice: pre-exposed R11G11B10F fits 4× MSAA inside Mali's 128 bpp tile budget, where RGBA16F does not.

**Rejected:** frame generation for an aiming game (holds back a real frame; 50–80 ms of extra lag measured); vendoring ASR or GSR2 as a second path (they are references for history rejection); per-pixel ACES (too heavy); compute post passes (they bypass framebuffer compression on Mali and Adreno). Variable-rate shading is a later measured option.

## 17. UI drawing

The UI pass draws into a non-sRGB view of the swapchain after tonemapping, so alpha means the same as in design tools (with linear blending a 75% scrim over white gives 3.18:1 contrast instead of 8.59:1). Where the platform lacks the second view format, scrims use alpha 0.84. Rounded and notched rectangles, strokes, linear gradients (up to four stops, CSS angle convention) and skew are drawn by one shader from a draw list (built). Images, icons and nine-slice return with a real runtime atlas. Layout and documents: [ARCHITECTURE.md](ARCHITECTURE.md) §10.

## 18. Terrain, vegetation and large worlds

(Planned, against the engine test scenes.)

- **64-bit world positions** with camera-relative rendering.
- **Terrain** is a heightfield concern, separate from the cluster DAG: CDLOD with camera-relative coordinates, horizon maps for self-shadowing.
- **Vegetation** is placed on the GPU deterministically per dirty tile into indirect lists; opaque leaf clusters where possible (masked only where needed); hemi-octahedral impostors at 1024²; a canopy heightfield for distance.
- **Cities:** HLOD stand-ins per cell; streaming by grid cell.
- **Animation** is mostly rigid; destruction is baked.

Every triangle costs binning work before any pixel is shaded, so geometry density is budgeted per tile, not only per frame.

## 19. Built today, and the order of work

**Built** (on wgpu, moving to `engine-gpu`): forward HDR with metal/roughness PBR and cooked textured materials; 2D tiled point lights (32 per tile); two sun cascades with per-cascade culling and a cached far layer; GTAO at half resolution; bloom; auto exposure; ACES; TAA on High and Ultra, MSAA 4× elsewhere; dynamic resolution without rebuilds; the retained scene; cooked mesh LODs with hysteresis; per-pass GPU timers; the checked frame plan; PNG goldens with content expectations ([ARCHITECTURE.md](ARCHITECTURE.md) §12).

**Order of work** (evidence strength per unit of effort; each step one canonical path with tiers chosen at bind):

| # | Step | Brief |
|---|---|---|
| 1 | Capability and baseline dump on both phones (features, driver versions) and the shader gate (Mali Offline Compiler in CI) | [vk-0](briefs/vk-0-phone-baseline.md) |
| 2 | `engine-gpu`, then the tile-correct render graph with the compiler rules of §3, per-device sort policy, fp16 varyings, R11G11B10F candidate | [vk-1](briefs/vk-1-gpu.md), [vk-2](briefs/vk-2-graph.md), [vk-3](briefs/vk-3-port.md) |
| 3 | `SurfaceClass` at cook, alpha-to-coverage in the masked prepass, roughness mips, flipbook hulls | to write |
| 4 | Shading core (§10) | to write |
| 5 | Texture formats by role (§9) | to write |
| 6 | Pacing and thermals (§15) | to write |
| 7 | Post chain: one composite, baked LUT, bloom, exposure, HDR swapchain choice | to write |
| 8 | TAAU and the upscaler contract; wire-safe ribbons | to write |
| 9 | Light cache with local invalidation; normalised probes | light-cache-1 to 3 |
| 10 | Lights: tile + z-bin | to write |
| 11 | CPU culling and the shadow system (§6, §11) | to write |
| 12 | Sky and media (§14) | to write |
| 13 | Effects: six-way smoke, flipbooks, overdraw budgets, VAT destruction | to write |
| 14 | Terrain and vegetation (§18) | to write |
| 15 | Cluster V1 | to write |
| 16 | Translucency tiers | to write |
| 17 | Occlusion, only if city benches justify it | gated |
| 18 | Gated tiers: GTAO bitmask (High), stochastic texture filtering (High, after 8), ray-query near shadows, cluster V2/V3, neural upscalers, Gaussian splats (watch) | gated |

**Not used:** work graphs, Android mesh shaders, texture-space shading, neural radiance caching (no mobile implementation).
