# Make every phone pixel earn its milliseconds

> Dated evidence, not the source of truth. The current design is in the topic docs listed in [../README.md](../README.md) §1; old doc names and decision numbers cited below refer to docs merged on 2026-11-17 (see [../README.md](../README.md#7-former-decision-numbers) §7).

Below the pass structure, the strongest phone evidence says one thing. **Unreal-like quality at 60–120 fps comes from cheap, cook-time and analytic algorithms that keep work on tile. Desktop screen-space and stochastic techniques do not fit.** The engine should adopt seven things:

1. **One fp16-safe BRDF** (Filament's standard model, which Epic also converged on), with extra lobes chosen per material at cook time.
2. **Specular anti-aliasing at two levels:** roughness baked from normal variance at cook, plus a roughly 10-ALU geometric term at run time.
3. **Two sun cascades that are fitted, cached and scrolled,** with horizon maps and capsule proxies beyond them, in one D16 atlas shared with local lights.
4. **MSAA resolved on tile,** with sub-pixel content fixed in geometry: alpha-to-coverage foliage and one-pixel-clamped wires and trails.
5. **A CPU-built tile plus z-bin light list** in place of GPU clusters.
6. **One participating-media froxel volume** serving aerial perspective, fog and light shafts, fed by Hillaire's sky LUTs.
7. **Content-side wins:** CDLOD terrain, deterministic GPU vegetation placement, small octahedral impostors, rigid-body vertex-animation-texture (VAT) destruction, and six-way-lit flipbook smoke.

The best measured numbers back this up:

- 4× MSAA kept in tile memory costs about **1–2%** in Arm's sample ([Arm](https://arm-software.github.io/vulkan-sdk/multisampling.html)).
- 4×MSAA+CMAA2 matches 8×MSAA in PSNR ([Intel](https://www.intel.com/content/www/us/en/developer/articles/technical/conservative-morphological-anti-aliasing-20.html)).
- Z-binning cut PS4 opaque time **15–20%** ([Activision](https://www.advances.realtimerendering.com/s2017/2017_Sig_Improved_Culling_final.pdf)).
- The only measured phone many-light system costs **+0.66 ms on a $99 Adreno 610** ([HypeHype](https://www.advances.realtimerendering.com/s2025/content/s2025_stb_lighting_v1.1_notes.pdf)).

The "cutting-edge" items mostly fail on phones today:

- Ray-traced sun shadows at full resolution pushed a G715/G720-class frame to **17.3 ms** ([Arm](https://dev.vulkan.org/user/pages/09.events/vulkanised-2024/vulkanised-2024-Iago-calvo-lista-arm-2.pdf)).
- The best Gaussian-splat renderer reaches 116–127 fps only when it renders one scene and nothing else ([Mobile-GS](https://www.alphaxiv.org/abs/2603.11531)).
- Neural materials have no in-shader path on Mali or Adreno.

Several current targets in GRAPHICS.md should change: four cascades with PCSS, GPU-built 3D clusters, screen-space reflections, and per-pixel ACES. The test phones also shape policy. The RMX5085's Mali-G720 has Deferred Vertex Shading but not the G925's sort-free fragment pre-pass, so front-to-back sorting still matters on it. Nearly every millisecond below is an estimate until the vk-0 bench runs on RMX5085 and RMX3853.

## Evidence labels, test hardware, and how this report builds on the last one

This report uses the same labels as the earlier round:

- **Measured:** a named party timed it on named hardware and published the number.
- **Claimed:** a vendor or author states it without a reproducible setup, or it came through a search summary.
- **Inferred:** this report's own reasoning or arithmetic.

Several primary sources (Karis's mobile shading post, the Tokuyoshi 2021 PDF, the Mali Offline Compiler guide and Arm's Valhall datasheet) could not be read in full. Formulas cited from them are marked as needing a constant check before anyone codes them.

The two test phones:

- **RMX5085:** realme, Mali-G720, 2800×1280 ([engine brief](../briefs/touch-from-layout.md)).
- **RMX3853:** realme GT 6T, Snapdragon 7+ Gen 3, Adreno 732 ([engine brief](../briefs/android-lifecycle.md)).

Neither phone appears in any source. Two architecture facts matter for them:

- Arm's 5th generation, which includes the G720, introduced Deferred Vertex Shading. It shades only position during tiling, and re-runs the vertex shader in the fragment phase for small triangles ([Arm](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/immortalis-g925-the-fragment-prepass)).
- The hardware fragment pre-pass, which makes draw order irrelevant, arrives only with Immortalis-G925 and Mali-G725/G625 (same source). So the RMX5085 still benefits from front-to-back opaque sorting (inferred).

The Mali-G720 is not an Immortalis part, so it very likely has no ray-tracing unit. Whether the Adreno 732 exposes ray query is unverified. Both facts belong in the step-1 capability dump.

The earlier report settled the pass structure: tile-correct graph, `SurfaceClass` at cook, texture formats by role, TAAU contract, light cache, CPU culling, cluster LOD. It also produced a 12-step plan. This report goes one level down, into what runs inside those passes. Where the two disagree, a table near the end resolves each case.

## One fp16-safe BRDF, extra lobes chosen at cook, and specular aliasing killed twice

### The base model is settled

Filament's standard model is the right shading core:

- **D:** GGX.
- **V:** height-correlated Smith, using the Hammon form `0.5 / lerp(2·NoL·NoV, NoL+NoV, α)`. It removes two square roots, and Filament calls it "good enough" for mobile.
- **F:** Schlick.
- **Diffuse:** Lambert.

Its precision fixes are what make it phone-ready. It computes `1−(n·h)²` as `|n×h|²` to avoid fp16 cancellation, clamps to 65504, and floors roughness at **0.089** so `1/α⁴` fits in half floats ([Filament](https://google.github.io/filament/Filament.md.html)). **Multiple-scattering energy compensation costs about one multiply-add** when it reads `dfg.y` from the same RG16F DFG lookup table that image-based lighting already samples: `Fr *= 1 + f0·(1/dfg.y − 1)` (same source). Filament notes that Karis's analytic environment-BRDF fit cannot feed this term, so the LUT, not the fit, is the canonical path.

GRAPHICS.md §5.2 already names GGX, multiscatter, Lambert and clearcoat, so this confirms the direction rather than changing it.

Epic's experience supports a single BRDF. Up to UE 5.5, mobile ran a separate GGX path that computed Fresnel from the environment BRDF instead of F0. **From 5.6 Epic deleted it**, because artists were authoring two materials to make mobile and desktop look alike. Epic accepted "a slight performance hit" and a rise in texture fetches but not in ALU ([Epic forum](https://forums.unrealengine.com/t/incorrect-pbr-brdf-for-mobile-platform/2723952)). Epic also gave up the half-precision path. This engine should keep one BRDF, written to be fp16-safe, not a mobile fork and not an fp32-only path (inferred). That is tenet 3 applied to shading.

### Richer materials are permutations, not runtime branches

Unreal's Substrate shows the ceiling on mobile. Its Blendable GBuffer keeps **one closure per pixel**, with no F90, haze, glints or diffusion SSS. When a material has more than one special feature, it keeps one by priority: Fuzz, then SSS, then Haze. Lower-end platforms "such as mobile" lose diffusion SSS entirely ([Epic](https://dev.epicgames.com/documentation/en-us/unreal-engine/overview-of-substrate-materials-in-unreal-engine)). The phone-tier material is therefore one slab plus one optional lobe, chosen at cook time:

- **Clearcoat** for aircraft paint and canopies: Kelemen visibility, fixed F0 0.04, base layer attenuated by `(1−Fc)`.
- **Charlie sheen** for cloth.
- **Pre-integrated skin** (Penner): a 2D LUT over N·L and curvature.
- **Anisotropic GGX** for brushed metal, which needs a tangent frame.
- **Foliage** (no extra lobe): wrap diffuse plus a view-dependent transmission term, with no extra pass.

Formulas: [Filament](https://google.github.io/filament/Filament.md.html); [Penner](https://advances.realtimerendering.com/s2011/Penner%20-%20Pre-Integrated%20Skin%20Rendering%20(Siggraph%202011%20Advances%20in%20Real-Time%20Rendering%20Course).pptx).

Each lobe is a specialisation-constant permutation, picked by the cooked material's shading-model id. The notes' cost estimates are **inferred and unmeasured**: clearcoat adds roughly 40–60% to specular ALU, and anisotropy roughly doubles D and V. Screen-space separable SSS is excluded, because it needs a resolve and two blur passes that break the tile-local forward frame. Hair is Kajiya-Kay with two shifted lobes (Scheuermann), and it is low priority for an aircraft game.

Area lights follow the same logic:

- **Default:** Karis's representative point for sphere and tube lights, about 20 ALU, with energy renormalised by `(α/α′)²` ([Karis 2013](https://blog.selfshadow.com/publications/s2013-shading-course/karis/s2013_pbs_epic_notes_v2.pdf)). The sun is a disk handled this way, using its roughly 0.27° angular radius, which also clamps sun fireflies on smooth paint.
- **LTC** (Heitz 2016): about 100+ ALU and two 64×64 LUTs ([Heitz](https://eheitzresearch.wordpress.com/415-2/)). Reserve it for a handful of cockpit screens or hangar panels in view.

### Specular anti-aliasing belongs in the cook and the shader, not in TAA

Aircraft panel edges, rivets and canopy rims seen edge-on are where specular shimmer is worst. Two cheap techniques together remove most of it:

1. **At cook:** bake normal-map variance into the roughness mip chain, using Toksvig or the vMF fit from The Order: 1886 ([Neubelt & Pettineo](https://blog.selfshadow.com/publications/s2013-shading-course/rad/s2013_pbs_rad_notes.pdf)). It costs nothing at run time. It is exactly the kind of check-once-at-the-boundary step the engine wants, and it sits next to the coverage-preserving alpha mips from the earlier round.
2. **At run time:** add the screen-space derivative of the normal to α² as a variance term, clamped by a threshold: Kaplanyan and Hill's 2016 filtering, which Filament ships as `specularAntiAliasingVariance/Threshold` ([Filament materials](https://google.github.io/filament/Materials.html)). Tokuyoshi and Kaplanyan's 2021 projected-space variant fixes the error that grows at grazing angles ([JCGT](https://www.jcgt.org/published/0010/02/02/paper-lowres.pdf)). The notes could not extract its constants, so treat them as unverified.

Both are on in every tier. They matter most on the MSAA tiers, where nothing temporal hides sparkle. On the TAA tier they reduce the history-clamp flicker the resolve would otherwise fight (inferred).

### Image-based lighting, occlusion and pre-exposure

Indirect light uses:

- the split-sum prefiltered cube (`lod = maxLod·perceptualRoughness`) with the RG16F DFG LUT;
- two-band SH irradiance (a 4×4 matrix-vector multiply, read from uniforms with no texture fetch);
- Lagarde's specular occlusion from AO, plus Russell's horizon fade (`horizon²`) for normal-mapped reflections that dip below the surface ([Filament](https://google.github.io/filament/Filament.md.html));
- Jimenez's cubic multi-bounce AO fit ([Activision](https://www.activision.com/cdn/research/Practical_Real_Time_Strategies_for_Accurate_Indirect_Occlusion_NEW%20VERSION_COLOR.pdf)).

**Pre-exposure is what lets all of this run in half floats.** Lights are multiplied by exposure before shading, on the CPU for the sun and IBL (Filament). Pre-exposure also opens a bandwidth option that the earlier report did not consider; see the micro-architecture section.

### Precision and occupancy rules

These rules are where shader cost is decided:

- **Register budget:** Valhall gives up to 64 32-bit work registers per thread, and crossing 32 roughly halves resident threads ([Arm](https://documentation-service.arm.com/static/6659a973876c8d213b7870b5); the table extraction was ambiguous).
- **fp16 rate:** fp16 packs two values per register and runs at twice the rate only when both 16-bit lanes are filled ([Harris/Arm](https://solidpixel.github.io/floats_in_shaders)).
- **Must stay fp32:** world positions, depth reconstruction, long light vectors, time, and the GGX denominator before its clamp.
- **fp16-safe:** colour, normals, the fixed BRDF, and pre-exposed radiance.
- **Device bind:** gate real `float16_t` arithmetic on `shaderFloat16`, checked once at bind; everything else uses `RelaxedPrecision`.
- **CI gate:** compile every cooked permutation through the Mali Offline Compiler. It reports work registers, a spill flag, and cycles per pipe (FMA, CVT, SFU, LS, V, T) with the bound pipe ([malioc article](https://legacy.codeproject.com/Articles/5307207/Shader-Optimization-with-the-Mali-Offline-Compiler)). Fail CI on any spill or on crossing 32 registers. **Flutter's Impeller already checks in a `malioc.json` baseline** ([Flutter](https://dart.googlesource.com/external/github.com/flutter/engine/+/c8fc3508317a556c6ce80f2ff796db6b77b7cbff/impeller/tools/malioc.json)), so there is production precedent.

This gives tenet 8 a device-free number for every shader.

### Tonemapping, colour and HDR output

Per-pixel ACES should go. ACES 2.0 (spring 2025) is a colour-appearance-model output transform, too heavy for per-pixel ALU on phones ([ACES](https://docs.acescentral.com/system-components/output-transforms/)). The replacement is one 32³–48³ 3D LUT that bakes the tonemapper and the per-game grade together, sampled once in the final composite. **AgX** (Blender's default since 4.0) or **Tony McMapface** (a 48³ LUT, used by Bevy) are the right curves for the filmic look ([tony-mc-mapface](https://github.com/h3r2tic/tony-mc-mapface)). Khronos PBR Neutral is for true-colour previews, not scenes.

GRAPHICS.md already targets "AgX + grading LUT". This makes it one fetch, and removes the separate ACES step from the current pipeline.

Output format is a device-bind decision inside the existing `PresentationBinding`. Use `VK_EXT_swapchain_colorspace` in this order of preference:

1. HDR10 PQ, if the GPU and display controller both expose it;
2. Display P3 on A2R10G10B10;
3. sRGB on RGBA8.

The choice is passed to the LUT stage as a display-target descriptor ([Android](https://developer.android.com/training/wide-color-gamut); [Arm forum](https://community.arm.com/support-forums/f/graphics-gaming-and-vr-forum/54708/vulkan-android-mali-gpu-drivers-do-not-expose-other-color-spaces-on-android-devices-with-hdr10-hdr10-display/181934)). Always dither by about 1 LSB with triangular-PDF blue noise before quantising, including on 10-bit targets ([Gjøl](https://loopit.dk/banding_in_games.pdf)). Skies and fog band visibly without it.

## Two fitted, cached cascades beat four filtered ones

Unreal's mobile renderer still allows **at most two cascades** for a movable sun ([Epic](https://dev.epicgames.com/documentation/en-us/unreal-engine/movable-light-mobility-in-unreal-engine)). Virtual shadow maps are listed as unsupported on mobile, and Epic says they are "likely not a good fit" without Nanite ([Epic VSM](https://dev.epicgames.com/documentation/en-us/unreal-engine/virtual-shadow-maps-in-unreal-engine)). Quality per millisecond therefore comes from fitting and caching, not from more cascades. GRAPHICS.md §4.2's target of "4 cascades … PCSS" should be replaced.

### Fit the cascades to what is visible

**SDSM is practical on phones.** Arm ran it on a Mali-T880 in 2016:

- It built a min/max depth pyramid from the previous frame to avoid stalls.
- It computed splits in compute.
- It reported that it "can lower resolution to get similar result" ([Arm SIGGRAPH 2016](https://community.arm.com:443/cfs-file/__key/communityserver-blogs-components-weblogfiles/00-00-00-20-66/3_2D00_mmg_2D00_siggraph2016_2D00_shadows_2D00_marius.pdf)).

Its numbers are in chart images only, so the gain is **claimed**. SDSM pulls against texel snapping, which only holds if partitions never change ([MJP](https://therealmjp.github.io/posts/shadow-maps)).

The inferred compromise:

- **Near cascade:** texel-snapped, with its radius quantised to a few sizes with hysteresis.
- **Split and depth range:** taken from the previous frame's min/max depth, reprojected and padded.
- **Depth range:** pancaked with `depthClampEnable`, so D16's 65,536 steps cover only the receivers, as Roblox does.

For an aircraft at altitude, this fitting shrinks the near cascade to the airframe and hands the ground to the second cascade.

### Cache and scroll the far cascade in tiles

Insomniac's 2012 CSM scrolling kept static geometry in a cached map and rendered only the newly exposed edge. It claimed to skip about **70%** of static casters ([Acton](https://advances.realtimerendering.com/s2012/insomniac/Acton-CSM_Scrolling(Siggraph2012).pdf)).

**Roblox shipped the phone version.** Its far cascades are split into tiles so updates can be throttled. Dirty tiles are re-rendered in one pass with per-tile CPU frustum culling, and every shadow map lives in one atlas. Its admitted open problem is that a sun direction change invalidates the cache ([Kapoulkine/Imagination, GDC 2020](https://zeux.io/data/gdc2020_pvr.pdf)).

The engine answer (inferred) is to quantise time-of-day sun motion into steps and rebuild the far cascade tile by tile over several frames. Destruction invalidates only the tiles under the broken element's bounds, which reuses the same local-invalidation idea the earlier report applied to the light cache.

### Filter with nine taps, and fake the rest

Castaño's optimised PCF covers a 5×5 Gaussian-like footprint in **9 bilinear compare taps**. It measured 10–20% faster than Gather4 on 2013 desktop GPUs, and Unity, Bevy and Knockout City use it ([Castaño](https://www.ludicon.com/castano/blog/?p=901)).

Pre-filterable maps cost more:

- EVSM and MSM need 2–4× the bytes per texel plus blur passes.
- EVSM4 cost **11.5 ms more** than 7×7 PCF at 2048² on a desktop GPU ([MJP](https://therealmjp.github.io/posts/shadow-maps)).
- Roblox made EVSM work on phones only with RG16F moments, 4×MSAA resolved on chip, and cached per-tile blurs.

PCF is the baseline. On the TAA tier, rotate a small tap pattern per frame with blue noise, and scale the kernel by a coarse blocker estimate. This gives contact-hardening without a PCSS blocker search (inferred, unmeasured on phones).

Beyond the cascades, use analytic proxies:

- **Horizon maps** for terrain. Cost does not depend on geometry ([Snyder & Nowrouzezahrai](https://diglib.eg.org/handle/10.2312/CGF.v27i4pp1275-1283)), and HPG 2025 shows a Fourier-compressed variant built for LOD terrain ([DLR](https://diglib.eg.org/items/5c49e826-6f07-448c-be0e-b98c36f46202/full)). Bake per-tile horizon angles at cook, and re-sweep a tile on the job pool only when a crater deforms it.
- **Capsule or ellipsoid soft shadows** for aircraft and characters beyond cascade range. Unreal measured **0.29 ms for the first character and 0.05 ms for each additional one** on a 2012 desktop GPU, at half resolution with tile culling ([Epic](https://dev.epicgames.com/documentation/en-us/unreal-engine/capsule-shadows-overview-in-unreal-engine)).
- **Cloud shadows:** one fetch of the projected cloud-coverage texture.

Bend's Days Gone screen-space sun shadows are real and their code is public ([Bend](https://www.bendstudio.com/blog/inside-bend-screen-space-shadows/)). But no phone cost exists, and Arm's hybrid screen-space plus ray-query experiment was no cheaper than pure ray query. Screen-space shadows are a High-tier option only where depth is already stored.

### One atlas, one depth-only pipeline

The shadow pass on a tiler is a binning-bound depth pass. It should use:

- a position-only stream ([Arm](https://learn.arm.com/learning-paths/mobile-graphics-and-gaming/optimizing-vertex-efficiency/vme-learning-path/));
- D16, with an empty fragment shader for opaque casters;
- a separate masked caster bin that keeps a fragment shader, mirroring the earlier report's `SurfaceClass` split.

Arm found multiview "great for shadow rendering" because each draw is submitted once for all cascades. `VK_KHR_multiview` is core in Vulkan 1.1. With a cached far cascade, though, both cascades re-render only occasionally, so multiview is a measured option, not a default (inferred).

**Local lights share the same D16 atlas**, following HypeHype's mobile design ([HypeHype](https://www.advances.realtimerendering.com/s2025/content/s2025_stb_lighting_v1.1_notes.pdf)):

- spot maps use normal projection;
- omni lights render a cube that is then stored octahedrally;
- static-geometry maps are cached;
- dynamic updates go through a priority queue;
- filtering is 1-gather PCF.

Muzzle flashes and explosions stay unshadowed.

### Ray-traced shadows are an opt-in replacement for the near cascade

Ray query works on Immortalis, but Arm's numbers on G715/G720-class phones are sobering. Ray-query shadows took the frame to **17.3 ms at full resolution, 12.2 ms with VRS and 10.1 ms at half resolution** ([Arm 2024](https://dev.vulkan.org/user/pages/09.events/vulkanised-2024/vulkanised-2024-Iago-calvo-lista-arm-2.pdf)). Frame times look vsync-capped, so cycles are the better indicator.

On Mali-G1's RTUv2:

- `TerminateOnFirstHit` gave **42.6%** on Bistro shadows;
- G925 to G1 improved shadows plus reflections by 36–68% ([Arm 2026](https://vulkan.org/user/pages/09.events/vulkanised-2026/Mobile-Ray-Tracing-Demystified-Iago-CalvoLista-Arm.pptx.pdf)).

Opacity micromaps on Mali G2 lifted a MegaLights foliage scene by **5.9–31%**. That scene still ran at 22–33 fps ([Arm](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/reducing-ray-traced-shadow-costs-for-foliage-with-omm-moku-results-on-arm-mali-g2)).

The fit is a High-RT tier, chosen at device bind, built as follows:

- **Shader:** the shader library's sun-visibility function gets a ray-query variant selected by specialisation constant, so the same forward shader calls it and no new pass exists.
- **Rays:** traced at half rate with opaque flags and first-hit termination.
- **Scope:** only within near-cascade range, against a TLAS built from the near caster set.
- **Vegetation:** kept as opaque proxies or left in the shadow map.

Neither test phone is likely to qualify.

### Shadow tier summary (budgets inferred)

| | Low | Medium | High | High-RT (opt-in) |
|---|---|---|---|---|
| Sun cascades | 1 near (1024² D16) + horizon maps | 2 (near 1536–2048², far 1024–2048²) | 2 × 2048² | far 2048²; near replaced by half-rate ray query |
| Fit | stable sphere, receiver-fit Z, pancaking | + previous-frame min/max split and range | same | same |
| Far update | n/a | cached, scrolled, 1 dirty tile per frame | 2–4 tiles per frame | same |
| Filter | 2×2 hardware PCF | Castaño 9-tap | Castaño + rotated taps resolved by TAA, blocker-scaled radius | first-hit ray + TAA edge |
| Beyond cascades | horizon maps, capsule blobs, cloud texture | same | + inset map for 1–2 hero aircraft | same |
| Local lights | 0–2 cached spots | atlas, ≤2 dynamic updates per frame | ≤4 dynamic updates per frame | same |
| Memory (computed) | ~2 MiB | ~12–16 MiB | ~16 MiB + inset | ~8 MiB + BVH |

## Clean pixels come from MSAA on tile and from fixing sub-pixel geometry at the source

### MSAA stays the base on tilers

Arm's Vulkan sample measures **4× MSAA at about 1–2%** when the multisampled images are transient (store DONT_CARE, lazily allocated) and resolved through `pResolveAttachments` ([Arm](https://arm-software.github.io/vulkan-sdk/multisampling.html)). A separate resolve pass added about **5 GB/s** of bandwidth in the Khronos sample ([Khronos](https://docs.vulkan.org/samples/latest/samples/performance/msaa/README.html)). One developer reports 20–30% more GPU-active time on Mali-G77. That is a single forum report, not an Arm measurement ([Arm forum](https://community.arm.com/support-forums/f/graphics-gaming-and-vr-forum/56051/some-questions-about-msaa-on-mali-g77/183974)). The RMX5085 bench must settle which figure applies to the G720.

HDR edges can re-alias under a box resolve. Pettineo's fix is to tonemap-weight each sample by `1/(1+luma)` before resolving and invert afterwards ([MJP](https://therealmjp.github.io/posts/msaa-resolve-filters/)). It can run as a sample-rate input-attachment read in the final subpass, if the driver keeps that on tile, which has to be measured.

Post-AA is an optional extra, not a base:

| AA (Intel, Bistro scene) | GTX 1080, ms | Iris Pro 580, ms | PSNR vs supersampled |
|---|---|---|---|
| FXAA | 0.11 | 0.94 | 36.75 |
| CMAA2 | 0.15 | 0.73 | 36.86 |
| SMAA 1x | 0.30 | 2.20 | 36.67 |
| 4×MSAA | — | — | 39.23 |
| 4×MSAA + CMAA2 | — | — | **39.81** (8×MSAA: 39.75) |

Source: [Intel](https://www.intel.com/content/www/us/en/developer/articles/technical/conservative-morphological-anti-aliasing-20.html) (measured, desktop and integrated GPUs).

CMAA2 is one third of SMAA's cost at equal or better PSNR. But it is a compute shader, and the earlier report found compute writes disable AFBC and UBWC. It is therefore a measured option for the MSAA tiers if shader aliasing survives specular AA, ideally ported to a fragment pass. No phone figure exists, and the notes' 0.5–1.5 ms at 1080p is an extrapolation.

### Sub-pixel content is a geometry problem

Neither MSAA nor TAA can fix a contrail, a missile trail or a power line that is narrower than a pixel. Persson's **phone-wire AA** can: clamp the ribbon's width to at least one pixel in the vertex shader, and scale alpha by true width over clamped width ([Humus](http://www.humus.name/index.php?page=3D&ID=89)). It works identically on MSAA and TAA tiers. It also beats Unreal's TSR thin-geometry detection, which Epic calls "not free" ([Epic](https://dev.epicgames.com/documentation/unreal-engine/thin-geometry-detection-with-temporal-super-resolution)). This should be the trail and ribbon vertex path, not an option.

Foliage on the MSAA tiers uses **alpha-to-coverage with fwidth-sharpened alpha** ([Golus](https://bgolus.medium.com/anti-aliased-alpha-test-the-esoteric-alpha-to-coverage-8b177335ae4f)). The earlier report's masked prepass rule reconciles this with HSR: A2C runs only in the masked depth prepass, which writes per-sample depth. The colour pass then shades with `EQUAL` and no discard, so edges are anti-aliased per sample and the colour pass keeps early-Z (inferred). The TAA tier keeps plain discard in the prepass, with coverage-preserving mips.

### TAA done right, and cheap on bandwidth

The state of the art is still the 2014–2016 recipe:

- velocity dilated from the closest depth;
- Catmull-Rom history (already in place);
- YCoCg variance clipping toward the mean, with γ≈1 ([Salvi](https://www.gdcvault.com/play/1023521/From-the-Lab-Bench-Real); [Playdead](https://github.com/playdeadgames/temporal));
- Karis's `1/(1+luma)` anti-flicker weight;
- a depth or velocity disocclusion test;
- blend α≈0.04–0.1;
- RCAS folded into the composite.

**k-DOP clipping** (SIGGRAPH Asia 2024) reduces ghosting consistently for about **0.2 ms** on desktop ([Ikkala et al.](https://webpages.tuni.fi/vga/publications/k_DOP_Clipping.html)). Use it if ghosting behind fast aircraft dominates.

The phone-specific issue is bandwidth, and the arithmetic is the report's own. An RGBA16F history at 1080p is about 16.6 MB, so one read plus one write is about **4 GB/s at 120 fps**. HypeHype quotes about 15 GB/s for a $99 phone ([HypeHype](https://www.advances.realtimerendering.com/s2025/content/s2025_stb_lighting_v1.1_notes.pdf)). Store history in 32 bits per pixel, as R11G11B10F or RGB10A2 in a perceptual space after the luma weighting.

Two more settings follow:

- Texture LOD bias: −0.5 to −1 when jitter is on, 0 on MSAA tiers.
- Missile and trail velocity: route it through the reactive mask from the earlier report's upscaler contract.

### One noise source and one dither

Use **one engine noise source**:

- spatiotemporal blue-noise masks (64×64×16 scalar and vec2) for GTAO directions, PCF rotation, volumetrics and stochastic filtering ([NVIDIA STBN](https://research.nvidia.com/publication/2022-07_spatiotemporal-blue-noise-masks));
- interleaved gradient noise as the fetch-free fallback ([Jimenez](http://www.iryoku.com/next-generation-post-processing-in-call-of-duty-advanced-warfare));
- R2 or Halton(2,3) for jitter ([Roberts](http://extremelearning.com.au/unreasonable-effectiveness-of-quasirandom-sequences/)).

### Post chain: gather passes at low resolution, everything per-pixel in one composite

**Bloom:**

- Use Jimenez's COD:AW chain: a 13-tap downsample with a Karis average on the first mip to kill fireflies, then 3×3 tent upsampling (same Jimenez source). Arm's dual filter is the alternative.
- Run from half resolution down in R11G11B10F. Arm's earlier-round AFRC fixed-rate compression fits these low-frequency mips.
- Arm's Spellsouls case put the original bloom at about **3 ms** on an unnamed Mali. It also noted that every blur level is a DRAM round trip, because blurs cross tile boundaries ([Arm](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/post-processing-effects-on-mobile-optimization-and-alternatives)).

**Built from the bloom chain:**

- Lens ghosts read an existing bloom mip.
- The exposure histogram reads the quarter-resolution bloom mip.
- Local exposure on High reads a 1/32 log-luma mip.

**Separate low-resolution passes:** only effects that gather neighbours need them. That means bloom, cinematic depth of field, and McGuire tile-max motion blur ([McGuire](https://casual-effects.com/research/McGuire2012Blur/index.html)), which suits fast aircraft.

**One composite pass:** everything else merges into it. That covers exposure, the tonemap and grade LUT, vignette, grain, dither, heat-haze offsets, chromatic aberration and RCAS. RCAS, chromatic aberration and haze read neighbours, so the composite must sample the resolved scene as a texture. When it is a subpass input, only the strictly per-pixel parts can merge.

### Ambient occlusion: visibility bitmask, half resolution, High only

The visibility-bitmask variant of GTAO replaces two horizon angles with a 32-sector bitfield and a thickness assumption. It removes dark halos around thin objects such as fins, wires and foliage. It costs **0.49 versus 0.51 ms at 8 samples** against plain GTAO on an RTX 2080 at 1080p ([Therrien et al.](https://ar5iv.labs.arxiv.org/html/2301.11376)). XeGTAO costs about 0.56 ms at 1080p on an RTX 2060 and 2.4 ms on an i7-1195G7 iGPU ([XeGTAO](https://github.com/GameTechDev/XeGTAO)).

Screen-space AO needs depth stored out of the tile. So it belongs only on the tier that already stores depth for TAA, at half resolution with 1–2 slices and about 8 steps, temporally accumulated. The phone estimate is roughly 1–2 ms (extrapolated). The MSAA tiers rely on baked and cooked AO plus specular occlusion. GRAPHICS.md's "GTAO (have)" should therefore become a High-tier feature, not a baseline one.

## A CPU-built tile and z-bin list replaces GPU clusters for lights

### Drobot's z-binning

The best-documented design that ships on both console and mobile is Drobot's (CoD: Infinite Warfare, 2017). It uses 2D screen tiles holding a light bitmask, plus a 1D table of min/max light index per depth bin, built after sorting lights by view depth.

At 1080p with 256 lights, the cost compares as follows:

| Structure | Memory | Cull operations per light |
|---|---|---|
| Tiles + z-bins | 1,036 KB + 32 KB | 32,400 + 8,096 |
| 3D clusters | 1,106 KB | 34,560 |

Measured on PS4:

- Opaque time fell from **9.00 to 7.65 ms** in one scene.
- Scalarised plus z-binned shading reached **80% of base** in another ([Drobot](https://www.advances.realtimerendering.com/s2017/2017_Sig_Improved_Culling_final.pdf)).

### Who ships it

Unity URP Forward+, the most-used mobile pipeline, uses the same tile plus z-bin layout **built on the CPU every frame** ([Meta](https://developers.meta.com/horizon/documentation/unity/unity-forward-plus-rendering/)). Blender EEVEE-Next adopted it for its depth range and low memory ([commit](https://miikahweb.com/en/blender/git-logs/commit/5697f96608ce96f25e02b3888325d0d23a613175)). On Quest, Forward+ beat per-object Forward from about 5 lights: **8.5 versus 12 ms at 30 lights**, measured.

### Caps cause flicker

Engines that cap lights per cell get flickering tiles. Unreal's Android light grid holds 8 lights per cell, and Arm's MegaLights port had to switch to a linked-list grid ([Arm](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/lighting-at-scale-bringing-hundreds-of-dynamic-lights-to-mobile-with-unreal-megalights)).

### Recommendation

This contradicts GRAPHICS.md §4.1, whose target is a "3D cluster grid … built on the GPU each frame". On a tiler that adds a compute dispatch and a compute-to-fragment barrier before the main pass. The canonical path should be the current 2D tiles with three changes:

1. Widen the mask to N×32 bits, sized to a validated max-lights setting, with 256 as the default.
2. Add a z-bin table of 2×u16 entries.
3. Build both on the job pool and upload them through a ring-buffered SSBO, with no barrier inside the render pass.

The shader masks tile words to the z-bin range and loops over set bits. It scalarises with subgroup ballot where that is reported; Vulkan 1.1 guarantees only basic subgroup operations. The limit becomes a buffer size, so there is no flicker.

The same design fixes the light data itself:

- **Photometric units:** lumens, lux and nits ([Lagarde](https://seblagarde.files.wordpress.com/2015/07/course_notes_moving_frostbite_to_pbr_v32.pdf)).
- **Falloff:** Karis's windowed inverse-square, `saturate(1−(d/r)⁴)²/(d²+1)`.
- **Radius:** derived from intensity and a luminance cutoff rather than set by artists. This keeps bright, tiny muzzle flashes tightly binned, by construction (inferred).
- **Same grid for decals and reflection probes,** as Godot and Doom 2016 do ([Courrèges](https://adriancourreges.com/blog/2016/09/09/doom-2016-graphics-study)).

### Light level of detail

City lights should be tiered by data, not by shader branch:

1. **Far:** emissive plus bloom only.
2. **Mid:** baked into the light cache.
3. **Near the player:** a few dozen real culled lights.

### Stochastic lighting is the measured upgrade path

**HypeHype's stochastic tile-based lighting** is the only many-light system measured on phones, with a fixed cost under light clumping:

- **Big tiles:** 128×128 px, each with 16 reservoir samples.
- **Small tiles:** 256 px, each resampling 1–4 lights.
- **Shadows:** a deferred shadow term evaluated per 2×2 quad.
- **Lighting:** done in pixel shaders, to keep framebuffer compression.

It costs **+0.66 ms on an Adreno 610 and about +0.35 ms on an Adreno 830** against sun-only lighting, and **+1.5 ms and +0.5 ms with 50 lights**. Sampling 500 lights costs 0.02 ms ([HypeHype](https://www.advances.realtimerendering.com/s2025/content/s2025_stb_lighting_v1.1_notes.pdf)). Its denoising is still open: TAA's clamp produces "flashing lighting splotch" artefacts, and it needs depth, normal and roughness before lighting. It is a later, TAA-tier option for when measured light clumping breaks the deterministic path.

MegaLights ran about 195 shadowed lights at **41 versus 8 fps** on a Mali-G1. That needs hardware ray query and an engine fork, so it is research-only. Per-pixel ReSTIR and light BVHs have no phone evidence.

### Reflections: two normalised probes, no SSR

Unity calls SSR "not recommended to be used on mobile" ([Unity](https://docs.unity3d.com/2017.3/Documentation/Manual/PostProcessing-ScreenSpaceReflection.html)), and Epic has no mobile SSR on its roadmap ([forum](https://forums.unrealengine.com/t/mobile-ssr-screen-space-reflections-not-working/387076/5)). Meta forks URP to blend only the **two nearest probes** on Quest, because per-pixel atlas blending is too expensive. GRAPHICS.md §4.5's "SSR on top" should leave the phone tiers.

The canonical path:

1. **Probes:** box-projected and prefiltered, culled through the light grid, at most two per pixel.
2. **Normalisation:** each probe's radiance is divided offline by its own average irradiance, and multiplied at run time by the receiver's irradiance from the light cache ([Lazarov, Black Ops II](https://blog.selfshadow.com/publications/s2013-shading-course/lazarov/s2013_pbs_black_ops_2_notes.pdf)). Explosions that feed the light cache therefore tint reflections without recapturing probes. This is a cheap complement to the earlier report's relit probes.
3. **Specular occlusion** on every probe sample.
4. **Curved glass:** canopies get a per-vehicle sky cube refreshed one 64–128² face per frame, plus the clearcoat lobe (inferred).
5. **Water:** only flat water on High gets Pixel Projected Reflections, the one-pass analytic-plane method ([Cichocki](https://www.advances.realtimerendering.com/s2017/PixelProjectedReflectionsAC_v_1.92.pdf)).

## Sky, fog, light shafts and aerial perspective share one froxel volume

### Hillaire's sky model is the single sky path

Hillaire's 2020 model, used by Unreal's SkyAtmosphere, was designed to scale to mobile. Epic ships it in Fortnite on mobile ES3.1, and its cost is "decoupled from screen resolution". It uses four small LUTs:

- **Transmittance.**
- **Multiple scattering,** indexed by altitude and sun angle.
- **Sky-view,** in lat-long with horizon-weighted latitude.
- **Aerial-perspective froxel volume.**

The LUTs cost **1.03 ms on the mobile reference**, which the paper summary places on an iPhone 6s, against 0.31 ms in total on a GTX 1080 ([Hillaire](https://blog.selfshadow.com/publications/s2020-shading-course/hillaire/s2020_pbs_hillaire_slides.pdf)). That is old hardware, so treat it as claimed.

Transmittance and multiple scattering depend only on atmosphere parameters. They should be recomputed only when time of day or weather changes, and amortised over frames. The steady per-frame work is then the sky-view LUT and the froxel volume. The inferred cost on a 2024 phone is well under 0.5 ms. Sky pixels cost one fetch, and opaque and translucent surfaces each add one 3D fetch in the forward shader, so there is no extra full-screen pass.

Flight needs two adjustments:

- The froxel depth range must span kilometres, using non-linear slices.
- Unreal's Fast Sky-View LUT shimmers at large distances ([forum](https://forums.unrealengine.com/t/skyatmosphere-horizon-fog-is-shifting-and-flickering-at-large-distances/243130)), so its sample counts must be validated at altitude.

Analytic Preetham or Hosek skies give no altitude change for a climbing jet, so they would be a second path for no benefit.

### Fog and light shafts extend the same volume

The aerial-perspective volume is already a froxel grid. Extending it into a participating-media volume is Frostbite's unified design. Inject into it:

- height fog;
- local fog volumes;
- destruction dust (GRAPHICS.md §4.3);
- sun shadow-map visibility;
- cloud Beer-shadow visibility.

Then integrate front to back. This gives one canonical path for fog, aerial perspective and volumetric shafts.

Arm's 2017 Mali prototype shows the phone shape, but published no milliseconds ([Arm](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/clustered-volumetric-fog)):

- 1/16 screen resolution;
- 32 slices, with 1–4 samples per slice;
- blue-noise jitter;
- a 1/16 history blend.

Low tier keeps analytic exponential height fog in the forward shader. The injected, shadowed volume is Medium and up, which matches GRAPHICS.md's current gating. The volume's compute must run before the first render pass, never between fragment passes.

### Clouds need a per-tier design, because flight breaks the AAA amortisation

**Horizon Zero Dawn** raymarched noise volumes in about 20 ms on PS4. It then updated **1 pixel in each 4×4 block per frame** and reprojected the rest, which brought clouds to about **2 ms** ([Schneider 2015](https://www.advances.realtimerendering.com/s2015/The%20Real-time%20Volumetric%20Cloudscapes%20of%20Horizon%20-%20Zero%20Dawn%20-%20ARTR.pdf)). In 2020 Hillaire said cloud rendering was "not reasonably achievable on mobile. For now."

The one phone data point comes from Adreno 540, a 2017 SoC ([80.lv](https://80.lv/articles/creating-volumetric-clouds-for-mobile-in-unreal-engine-5)):

- An unoptimised 64-step raymarch took **about 300 ms**.
- Rendering into a **320×180** target with 64 density steps, 2 light steps, early-out and a 4-tap blur, then upsampling to 1080p, reached a reported **solid 60 fps**.
- Dithered TAA "performed poorly", and the result is visibly low resolution.

A yawing jet camera has little reprojection history, so HZD's 16-frame latency will show (inferred). The per-tier design:

| Tier | Clouds |
|---|---|
| Low | 2.5D cloud layer shaded from a coverage texture, plus impostor cumulus cards |
| Medium | Low's layer + a quarter-resolution raymarched cumulus band, with TAA-style accumulation (RDR2's "instant but blurrier") and HZD cheap/expensive stepping |
| High | Medium's band + more steps + a Beer shadow map on terrain + a fly-through deck accelerated by a coarse empty-space mip (the Nubis³ idea) ([Guerrilla](https://www.guerrilla-games.com/read/nubis-cubed)) |

### VFX quality comes from authored textures; overdraw is the constraint

**Smoke lighting.** Unity's **six-way lighting** packs six directional lightmaps into two RGBA textures, at a cost "comparable to a traditional lit sprite". It evaluates probes in the vertex shader, and it shipped for URP in April 2025 ([Unity](https://unity.com/blog/engine-platform/realistic-smoke-with-6-way-lighting-in-vfx-graph)). Combined with **motion-vector flipbooks**, which allow fewer frames and smooth slow-motion ([Unreal community](https://forums.unrealengine.com/t/community-tutorial-level-up-your-flipbooks-with-motion-vectors-and-subuv-blending-unreal-5-tutorial/2694470)), it gives convincing missile smoke at sprite cost.

**Overdraw** is the binding constraint. Imagination puts the ideal overdraw counter at about 1.0, and says sustained values above **2.0** start to hurt ([Imagination](https://blog.imaginationtech.com/improving-particle-systems-on-powervr/)). Tight sprite hulls "can drastically reduce the overdraw" ([RealtimeVFX](https://realtimevfx.com/t/niagara-and-vfx-optimization-how/25656)).

This fits the engine's cook philosophy. Bake 4–8-vertex cutout hulls from flipbook alpha at cook, the same pass that classifies `SurfaceClass`. Then enforce per-effect and screen-coverage budgets as validated data.

**The rest of the VFX stack (inferred design):**

- **Explosions** are layers: a motion-vector fireball flipbook, six-way smoke, additive velocity-stretched sparks, a distortion ring, and a short unshadowed light in the z-bin list.
- **Trails** are GPU ribbons from per-emitter ring buffers, with phone-wire AA.
- **Decals** are binned in the light grid and sampled in the forward shader.
- **Heat haze** writes a small offset buffer that the composite applies.
- **Half-resolution particles** need their own render pass on a tiler, so they are a per-effect flag for screen-filling smoke, never a global switch.

## Terrain, vegetation and destruction are geometry the binner can afford

### Every triangle costs binning work before any pixel is shaded

On a tiler, each triangle costs position shading plus per-tile list writes before a pixel is shaded. Mali bins into 16×16 tiles ([Arm](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/the-mali-gpu-an-abstract-machine-part-2---tile-based-rendering)). On Adreno, "even a draw contributing to one tile will incur a small overhead on each tile" ([Qualcomm](https://developer.qualcomm.com/sites/default/files/docs/adreno-gpu/snapdragon-game-toolkit/gdg/gpu/gpu.html)).

Arm credits its 5th generation, including DVS, with **26–41% less bandwidth** in Fortnite, Genshin Impact and Elven Ruins. That figure is claimed, and it covers the architecture and drivers as a whole ([Arm](https://developer.arm.com/community/arm-community-blogs/b/announcements/posts/arm-gpus-built-on-new-fifth-gen-architecture)).

The engine rule (inferred) is an **LOD floor of several pixels per triangle**, chosen by screen-space error:

- 2×2 quad shading wastes most fragment work below about 10 pixels.
- Concurrent binary trees target about **49 pixels per triangle** ([Benyoub & Dupuy](https://arxiv.org/html/2407.02215v1)).

Fat triangles also touch fewer tiles than thin ones ([Meta](https://developers.meta.com/horizon/documentation/native/android/gpu-impaired-algorithms/)).

### Terrain is a heightfield concern, separate from the cluster DAG

**Use CDLOD:**

- a CPU quadtree selects fixed N×N grid patches (33² or 65²);
- one shared grid mesh is drawn per patch;
- the vertex shader samples the heightmap;
- a distance-based morph removes cracks and skirts ([Strugar](https://github.com/fstrugar/CDLOD)).

Its LOD uses true 3D distance, so altitude coarsens the whole terrain naturally. Clipmaps centre their rings in xy, which suits ground views better than flight.

**Concurrent binary trees (HPG 2024)** are the long-term planet-scale option. They update in **0.1 ms** and render in 1.75 ms for an Earth-sized planet on a 6800 XT, using about 7 MiB ([arXiv](https://arxiv.org/html/2407.02215v1)). They need 9 compute kernels per update and have no phone measurement.

This does not conflict with the earlier report's cluster DAG. Terrain is a heightfield and buildings are meshes, so they are separate concerns, each with one path.

Precision, from the CBT authors' warning that f32 breaks at planet scale:

- Keep world positions as an integer cell plus an f32 offset, or as f64, on the CPU.
- Upload camera-relative f32 positions.

**Terrain texturing:**

- Near the camera: a runtime page cache of pre-blended terrain material.
- At distance: a cooked low-resolution colour map, which dominates at altitude.
- Reference for the desktop ceiling: Far Cry 4's adaptive virtual texturing ([GDC](https://gdcvault.com/play/1021761/Adaptive-Virtual-Texture-Rendering-in)).

### Vegetation is placed on the GPU, deterministically

Horizon Zero Dawn places **all** of its vegetation procedurally on the GPU around the player ([Guerrilla](https://www.guerrilla-games.com/media/News/Files/GDC2017_VanMuijden_GPUBasedProceduralPlacementInHorizonZeroDawn.pdf)). The engine form (inferred):

- a compute pass per terrain tile;
- density maps × a hash of the tile id, so there is no RNG state;
- output into pre-sized per-LOD instance lists with indirect arguments;
- one `vkCmdDrawIndexedIndirect` per LOD class, which matches the earlier report's submission path.

**Grass** follows Ghost of Tsushima's pattern: blades get fewer and wider with distance and widen when edge-on ([GDC](https://gdcvault.com/play/1027214/contactUs); details via a [reimplementation](https://github.com/2Retr0/GodotGrass)). Grass stops beyond tens of metres, and from altitude it does not exist.

**Trees** prefer opaque leaf-cluster geometry over masked cards near the camera, for the HSR and LRZ reasons the earlier report documented. **Wind** is a global wind field plus hierarchical vertex-colour bend, which costs a few vertex ALU and no memory traffic.

### Impostors are smaller than the desktop default

UE 5.7's experimental Nanite Foliage collapses far foliage into near-pixel-sized voxels ([Epic](https://dev.epicgames.com/documentation/unreal-engine/nanite-foliage)), but it requires Nanite.

The phone equivalent is the **octahedral impostor**. Fortnite uses a 12×12 frame grid at 2048² and blends three frames ([Epic](https://dev.epicgames.com/documentation/unreal-engine/impostor-baker-plugin-in-unreal-engine)). The arithmetic is the report's own: a 2048² atlas in ASTC 6×6 with colour and normal-depth maps is about 5 MB per species with mips, so ten species is about 50 MB, which is too much for a phone. Use:

- 1024² hemi-octahedral atlases (about 1.2 MB each), which suit top-down flight;
- one or two blended frames instead of three;
- for far forests seen from altitude, a cooked "canopy heightfield" drawn as part of the terrain (inferred);
- for city blocks, a per-block merged HLOD proxy.

### Animation is mostly rigid, and destruction is baked

Aircraft and missiles need hierarchical rigid transforms per part, not skinning.

**Skinning**, where it exists, defaults to the vertex shader. Compute pre-skinning only pays when one mesh is drawn in several passes. On Mali, compute shares the vertex queue, and long compute jobs starve fragment work ([Arm](https://community.arm.com/developer/tools-software/graphics/b/blog/posts/using-asynchronous-compute-on-arm-mali-gpus)).

**ACL** is the animation-compression reference. On Fortnite data it reached **5.21:1 against UE4's 1.69:1**, and decompressed **2.3× faster on a Samsung S8** ([Frechette](https://nfrechette.github.io/2019/01/25/compressing_fortnite_animations)).

**Shipped constrained-hardware destruction is pre-fractured and baked:**

- Simulate rigid bodies offline and play them back as vertex animation textures on instanced pieces ([SideFX](https://www.sidefx.com/docs/houdini/nodes/out/labs--vertex_animation_textures-3.0.html)).
- Store per-piece position and quaternion rather than per-vertex data. That is about **92 KB per variant** for 64 pieces × 90 frames (computed).
- Play variants by impact direction.
- Pool small debris and integrate it ballistically against the terrain height. The simulation stays deterministic, because debris is presentation-only.

The destruction event that starts a VAT also invalidates the light-cache region and the shadow tiles under it, so one event drives all three (inferred).

## Micro-architecture rules the graph compiler and CI should enforce

### Architecture facts

**Mali Valhall:**

- 16-wide warps and up to 64 registers ([Arm](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/introducing-arm-mali-g77-with-new-valhall-architecture)).
- Three arithmetic pipes (FMA, CVT, SFU) alongside load/store, varying and texture units. Texturing runs in parallel with arithmetic, so the longest pipe sets the cost.

**Immortalis-G925 fragment pre-pass,** measured at fixed frequency ([Arm](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/immortalis-g925-the-fragment-prepass)):

- It saved **0.6–6.5%** of GPU cycles in shipping games.
- It cut FP arithmetic by up to 16.1%, and fragment read bandwidth by up to 39% in Roblox.
- Arm documents what disables it for every later draw in a tile:
  - non-opaque draws that write depth;
  - draws that do not write every render target;
  - draws that read the tile buffer;
  - centroid, coverage-mask or helper-lane-dependent shaders;
  - subgroup operations touching helper lanes;
  - computed-LOD texture lookups.
- `discard` and depth writes are compatible with it, but late-Z.
- Tile size drops from 64×64 to **64×32 above 128 bits per pixel** and to 32×32 above 256.
- Arm's "ideal draw": early-Z, opaque, writes all targets, ≤128 bpp, cheap position shader.

**Adreno:**

- Wave64, or wave128 for 16-bit fragment work ([AnandTech](https://at-web1.www.anandtech.com/show/21445/qualcomm-snapdragon-x-architecture-deep-dive/3)).
- GMEM of 1–4 MB depending on the chip.
- FlexRender switches between binned and direct rendering at the driver's discretion ([Qualcomm](https://developer.qualcomm.com/sites/default/files/docs/adreno-gpu/snapdragon-game-toolkit/gdg/gpu/overview.html)).

**Apple family 9** (A17 Pro, M3) decouples occupancy from peak register use ([Apple](https://developer.apple.com/videos/play/wwdc2023/10127/)). That matters for the later Metal backend.

### Rules for the graph compiler

These become rules in the vk-2 compiler, extending the earlier report's three errors:

1. **Pass order by class.** Within a pass, order draws by class: compatible opaque draws first, then everything that would disable the pre-pass (masked late-Z, tile reads, helper-lane shaders).
2. **Sort policy per device.** Front-to-back sorting is decided at device bind: on for G720-class Mali and for Adreno LRZ, off on pre-pass hardware.
3. **No helper-lane operations in opaque fragment shaders.** Quad and subgroup operations stay in compute.
4. **Count all samples in the bpp budget.** The 128 bpp budget should count every sample (inferred, verify with counters). RGBA16F colour at 4× MSAA is 256 bpp of colour, while pre-exposed **R11G11B10F at 4× fits in 128**. Godot's mobile renderer already trades range for bandwidth with an RGB10A2 colour buffer ([Godot](https://docs.godotengine.org/en/4.5/engine_details/architecture/internal_rendering_architecture.html)). The scene-colour format becomes a measured tier choice, against GRAPHICS.md's fixed `Rgba16Float`.
5. **Varyings and streams:**
   - fp16 outputs through `VK_KHR_16bit_storage` where reported ([Khronos](https://docs.vulkan.org/samples/latest/samples/performance/16bit_storage_input_output/README.html));
   - position in its own minimal stream, because G925 may shade position three times per draw.
6. **Compute:**
   - "Bandwidth and watts first, arithmetic second" ([Vulkan tutorial](https://docs.vulkan.org/tutorial/latest/Advanced_Vulkan_Compute/12_Mobile_and_Embedded_Compute/03_mobile_optimization.html)).
   - Workgroup of 64, with the exact size declared.
   - Per-pixel post stays in fragment subpasses.

### API baseline

Android's floor is Vulkan 1.1 plus Baseline 2022, at **86.61%** of active devices. The Vulkan 1.3 VPA15 profile was at **3.69%** in early 2024 ([LunarG](https://www.lunarg.com/wp-content/uploads/2024/04/The-Vulkan-Profiles-Tools-LunarG-Christophe-Riccio-04-11-2024.pdf)). Graphics pipeline libraries and shader objects have no documented vendor support, so there is one path: a plain `VkPipeline` per cooked permutation, warmed into a persisted `VkPipelineCache`.

### Measurement tools

| Platform | Tool | Use |
|---|---|---|
| Mali | Mali Offline Compiler | CI gate per shader, device-free |
| Mali | libGPUCounters / Streamline | counters on device |
| Adreno | Snapdragon Profiler | counters on device |
| Apple | Xcode | spilled bytes and tile-memory counters |

For energy, Kuro Games cut Wuthering Waves' power by **9.68%** using Android Studio's Power Profiler on ODPM rails. ODPM exists only on Pixel 6 and later ([Android](https://developer.android.com/stories/games/kuro-powerprofiler)), so on the realme phones the energy metric falls back to clock and bandwidth proxies.

## Emerging techniques: adopt one, gate three, watch the rest

**Adopt after TAA: stochastic texture filtering.** One stochastic texel lookup, filtered after shading and denoised by TAA, matched reference quality at **1.60× (trilinear) and 2.77× (tricubic)** filtering speed-ups on desktop ([Pharr et al.](https://research.nvidia.com/labs/rtr/publication/pharr2024stochtex)). It suits the TAA tier only, because the MSAA tiers have nothing to resolve the noise. It is a shader-library change plus the shared blue-noise texture.

Under magnification it can alias, and the fix uses wave intrinsics ([NVIDIA 2025](https://research.nvidia.com/labs/rtr/publication/akeninemoller2025collaborative/collaborative_texfilt.pdf)). Per the micro-architecture rules, that fix must stay off pre-pass-sensitive opaque shaders. No phone measurement exists. It also interacts with the earlier report's Mali finding that trilinear costs twice bilinear, which is a reason to measure it early.

**Gated: ray query.** Arm says Mali devices expose only ray query and that the full RT pipeline is slow on first-generation units. Arm prefers ray query in fragment shaders for framebuffer compression ([Arm](https://learn.arm.com/learning-paths/smartphones-and-mobile/ray_tracing/rt03_ray_traversal)). Shipped Android titles each use one effect: Diablo Immortal does reflections from a visibility buffer ([Android](https://developer.android.com/stories/games/diablo-raytracing)), and War Thunder Mobile does shadows (secondary source). The engine's first and only RT effect should be near-field sun visibility, as described in the shadow section.

**Gated: the visibility buffer.** The notes call it "usable now" because Diablo Immortal ships one. But it feeds that game's reflections, not its main shading. The earlier report's evidence still governs: SSBO reads are uncached on Adreno 6xx, and compute shading disables AFBC. It stays a gated V3 tier.

**Gated: Gaussian splatting.** Mobile-GS (ICLR 2026) reaches **116–127 fps** on a Snapdragon 8 GPU for one scene and **74 fps throttled**, while earlier methods managed **8–24 fps** on the same device ([Mobile-GS](https://www.alphaxiv.org/abs/2603.11531)). A full-screen splat scene therefore costs about 8–13 ms alone, against an inferred 2–3 ms budget for a background layer. `KHR_gaussian_splatting` is reported ratified, but only by a trade site, and compression (SPZ) is still a draft ([RadianceFields](https://radiancefields.com/platforms/the-khronos-group)). No shipped game renders environments with splats. Splats are gated until a distant-scenery use is measured.

The notes suggest "designing for" splats and neural nodes now. Under tenet 3 that must not become stub passes or unused content types. The only legitimate preparation is what TAAU already requires: motion vectors, jitter, normals and roughness as graph resources.

**Watch: neural rendering.** Neural radiance caching has no mobile implementation. The only mobile radiance-cache research needed custom hardware ([Lumina](https://arxiv.org/pdf/2506.05682)). In-shader ML exists only as Apple TensorOps on the A19 and M5 neural accelerators ([WWDC26](https://developer.apple.com/videos/play/wwdc2026/330/)), and on Arm's `VK_ARM_tensors` / `VK_ARM_data_graph`, which run on an emulation layer pending hardware ([Arm](https://learn.arm.com/learning-paths/mobile-graphics-and-gaming/vulkan-ml-sample/1-introduction/)). NVIDIA's neural materials are a desktop, tensor-core claim. The first practical neural pieces will be once-per-frame post networks (denoise, upscale) behind the earlier report's upscaler node.

**Ignore:** work graphs (AMD-only experimental), Android mesh shaders, and texture-space shading.

## Conflicts with the earlier report and current docs, resolved

| Topic | Earlier report or current doc | This round's evidence | Resolution |
|---|---|---|---|
| Sun shadows | GRAPHICS §4.2: 4 stable cascades + PCSS | Unreal mobile caps at 2; SDSM + caching shipped on phones (Roblox); EVSM and PCSS have no phone numbers | 2 fitted cascades, cached and scrolled far tiles, Castaño PCF, blocker-scaled radius on High; horizon maps and capsules beyond |
| Light culling | GRAPHICS §4.1: 3D clusters built on the GPU each frame | Tile + z-bin is equal in memory, cheaper to cull, and built on the CPU by URP | CPU tile + z-bin, N×32-bit masks; decals and probes share it |
| Reflections | GRAPHICS §4.5: SSR on top of probes | Unity and Epic exclude mobile SSR; Meta limits to 2 probes | ≤2 normalised probes; PPR for High water; no phone SSR |
| AO | GRAPHICS: GTAO "have" on all tiers | Needs stored depth; desktop-only numbers | Visibility-bitmask GTAO, half resolution, High only |
| Tonemap | Current ACES; target AgX + LUT | ACES 2.0 too heavy per pixel | One baked AgX or Tony LUT including the grade |
| Foliage AA vs HSR | Earlier: masked prepass + EQUAL, discard costs HSR | A2C is the cheapest foliage AA on MSAA tiers | A2C only in the masked depth prepass; colour pass EQUAL with no discard |
| Post-AA | Earlier: prefer fragment over compute (AFBC/UBWC) | CMAA2 is compute | Optional, measured; port to fragment if adopted |
| Draw sorting | Implicit front-to-back | G925 pre-pass makes order irrelevant; G720 lacks it | Sort policy chosen per device at bind |
| Scene colour | GRAPHICS: `Rgba16Float` | 128 bpp tile budget × MSAA samples | Measure R11G11B10F pre-exposed on MSAA tiers |
| Visibility buffer | Earlier: gated V3 | Notes: "usable now" (Diablo Immortal) | Earlier stands; Diablo's use is for RT reflections |
| Terrain vs cluster DAG | Earlier: cluster LOD for meshes | CDLOD for heightfields | Separate concerns, one path each |

## Frame anatomy per tier

Every millisecond below is an **estimate**. The estimates are extrapolated from the cited desktop, console and old-phone figures, and none is measured on the test phones.

Tier definitions (inferred):

| Tier | Example hardware | Display and AA | Target | GPU budget |
|---|---|---|---|---|
| Low | Adreno 6xx, Mali-G5x/G7x | about 720p internal, 4×MSAA | 60 fps | ~11 ms |
| Medium | RMX3853, RMX5085 | 0.6–0.7 render scale of 2800×1280 or 2400×1080, 4×MSAA | 60 fps | ~11 ms |
| High | Adreno 8xx, G925, A17+ | about 0.6 render scale with TAAU | 60 fps with all effects | ~13 ms |

The GPU budgets leave about a third of the 16.7 ms frame as thermal headroom (inferred). A **120 fps mode** on Medium or High uses the Medium anatomy at a lower render scale, drops clouds to the 2.5D layer and the far-shadow refresh to every other frame, and targets about **6 ms** of GPU time. CPU work (culling, light binning, light-cache solve, vegetation placement setup) runs on the job pool and costs no GPU time.

| Pass (render pass) | Low: algorithm · est. ms | Medium: algorithm · est. ms | High: algorithm · est. ms |
|---|---|---|---|
| Early compute (before any render pass) | sky-view LUT, amortised transmittance/MS · 0.1 | + AP/fog froxel volume, injected and shadowed · 0.4 | + local fog and dust, larger volume · 0.5 |
| Vegetation placement and VFX sim (compute) | CPU instance lists · 0 | GPU placement per dirty tile + particle sim · 0.3 | same, denser · 0.4 |
| Sun shadow (D16 atlas) | 1 cascade 1024², static cached + dynamic overlay · 0.8 | near 2048² every frame + far 1 dirty tile · 1.3 | near 2048² + 2–4 far tiles · 1.6 |
| Local-light shadows (same atlas) | 0–2 cached spots · 0.1 | ≤2 updates · 0.3 | ≤4 updates · 0.5 |
| Depth / motion prepass | masked-only prepass with A2C · 0.4 | masked-only prepass with A2C · 0.6 | full depth + motion vectors, stored · 0.9 |
| Half-res AO | — | — | visibility-bitmask GTAO, 1–2 slices, 8 steps · 1.0 |
| **Main render pass:** opaque forward+ (tile + z-bin), masked EQUAL, terrain CDLOD, impostors | Filament BRDF, geometric spec AA, SH2 + 1 probe, 2×2 PCF, height fog · 5.5 | + clearcoat/sheen permutations, 2 probes, Castaño PCF, froxel fetch · 5.8 | + rotated PCF, local exposure inputs · 5.2 at lower res |
| ↳ sky subpass | sky-view fetch + 2.5D clouds · 0.2 | same · 0.3 | same · 0.3 |
| ↳ translucent and VFX subpass | sorted premultiplied, 6-way smoke, phone-wire ribbons · 0.8 | same + soft particles via depth input · 1.0 | same · 1.0 |
| ↳ MSAA resolve | on tile, tonemap-weighted · ~0.1 | same · ~0.2 | — |
| Raymarched clouds (separate) | — | quarter-res cumulus band · 0.7 | quarter-res + Beer shadow map · 1.0 |
| TAAU resolve (fragment) | — | — | YCoCg clip (k-DOP if needed), 32-bit history · 1.0 |
| Bloom chain | dual filter, 4 mips, R11G11B10F · 0.5 | COD:AW, 5 mips · 0.6 | COD:AW, 6 mips + lens ghosts · 0.7 |
| Exposure | histogram from bloom mip · <0.05 | same · <0.05 | + local exposure from 1/32 mip · 0.1 |
| Composite + UI | LUT tonemap/grade, dither, vignette · 0.4 | + haze offsets · 0.5 | + RCAS, motion blur gather if on · 0.8 |
| **Estimated GPU total** | **~8.9 ms** | **~12.0 ms** (over its budget; Medium drops clouds or render scale first) | **~15.0 ms** (over its budget; High drops local exposure, AO or render scale first) |

The Medium and High totals exceed their own budgets on paper. That is the honest result of stacking estimates. It is why the bench must measure each row before any tier's feature set is frozen, and why dynamic resolution driven by thermal headroom (earlier step 5) is part of the frame design, not an extra.

## Ordered implementation plan, merged with the earlier twelve steps

The order follows evidence strength per unit of effort, and keeps the earlier plan's order where it already holds. "E" marks an earlier step. "N" marks a new one. Every step is one canonical path, with tiers chosen once at device bind.

| # | Step | Origin | Why here |
|---|---|---|---|
| 1 | **Capability and baseline dump**, extended with `shaderFloat16`, `16bit_storage`, subgroup arithmetic/ballot, multiview, depthClamp, swapchain colour spaces, ray query and OMM. Add a **malioc CI gate** (spills, >32 registers, cycle regressions; Flutter-style baseline) | E1 + N | every tier decision needs real bits; the gate makes tenet 8 automatic for shaders |
| 2 | **Tile-correct graph**, plus: per-device sort policy, class-ordered draws (pre-pass-incompatible last), sample-counted 128 bpp check, R11G11B10F scene-colour candidate, fp16 varyings, position-only stream | E2 + N | the largest measured wins; the micro-architecture rules belong in the same compiler |
| 3 | **`SurfaceClass` at cook**, plus A2C in the masked prepass on MSAA tiers, normal-variance roughness mips, and flipbook cutout hulls in the same cook stage | E3 + N | one cook boundary fixes opacity, specular aliasing and particle overdraw together |
| 4 | **Shading core:** Filament standard BRDF, fp16 rules, multiscatter from the DFG LUT, geometric specular AA, SH2 + split-sum IBL with specular and horizon occlusion, lobes as specialisation-constant permutations, representative-point area lights | N | every later pass calls this code; it is cheaper to fix before content scales |
| 5 | **Texture formats by role** | E4 | unchanged; precedes bulk content |
| 6 | **Pacing and thermals** (present timing, ADPF, thermal-driven resolution) | E5 | the frame anatomy is over budget without it |
| 7 | **Post chain:** one composite pass, baked AgX/Tony + grade LUT, blue-noise dither, COD:AW bloom at half resolution and below, histogram exposure from the bloom mip, HDR swapchain choice in `PresentationBinding` | N; revises GRAPHICS §5.5 | removes per-pixel ACES and extra full-screen passes on every tier |
| 8 | **TAAU and the upscaler contract**, plus Salvi/Pedersen clip, k-DOP if ghosting, 32-bit history, mip bias, STBN noise source, phone-wire AA for trails and wires | E6 + N | High tier's image quality; ribbons fix sub-pixel content on all tiers |
| 9 | **Light cache with destruction**, plus probe normalisation by light-cache irradiance and ≤2 probes per pixel | E7 + N; revises GRAPHICS §4.5 | reflections inherit the cache's dynamic lighting for free |
| 10 | **Lights:** CPU tile + z-bin, N×32-bit masks, photometric units, radius from intensity, decals and probes in the same grid, light LOD for cities | N; revises GRAPHICS §4.1 | removes the 32-light cap and flicker without a compute barrier |
| 11 | **CPU culling and the shadow system:** one D16 atlas, previous-frame fit, cached and scrolled far tiles, Castaño PCF, opaque and masked caster bins, octahedral omni maps, horizon maps, capsule proxies, cloud-shadow texture | E8 + N; revises GRAPHICS §4.2 | shadows are the main bandwidth sink after the main pass |
| 12 | **Sky and media:** Hillaire LUTs, AP/fog froxel volume with injection, tiered clouds | N | one path for sky, fog, shafts and dust |
| 13 | **VFX:** six-way smoke, motion-vector flipbooks, overdraw budgets as validated data, layered explosions, rigid-body VAT destruction, pooled debris | N | sells explosions at sprite cost; destruction events drive light-cache and shadow invalidation |
| 14 | **Terrain and vegetation:** CDLOD with camera-relative coordinates, deterministic GPU placement into indirect lists, opaque leaf clusters, 1024² hemi-octahedral impostors, canopy heightfield, city HLOD | N | the game's outdoor scale depends on it; independent of the cluster DAG |
| 15 | **Cluster V1** | E9 | unchanged |
| 16 | **Translucency tiers** | E10 | unchanged |
| 17 | **Occlusion, only if city benches justify it** | E11 | unchanged |
| 18 | **Gated tiers:** visibility-bitmask GTAO (High), stochastic texture filtering (High, after 8), ray-query near-field sun shadows (High-RT), STB stochastic lighting if clumping is measured, Medium raymarched clouds, splats and neural backends (watch), plus the earlier E12 items | E12 + N | each needs a measurement or hardware the baseline lacks |

## Measurements on RMX5085 and RMX3853

All of these run through the vk-0 bench, cold and after 30 minutes, with thermal state recorded. GPU time comes from frame timestamps. Pass-level effects use Streamline on the RMX5085 and Snapdragon Profiler on the RMX3853. Energy uses clock and bandwidth proxies, since ODPM is Pixel-only.

| Area | Measurement | Decides |
|---|---|---|
| Capabilities | the step-1 list, including ray query, OMM, float16 and colour spaces, per phone and driver | which tiers exist; confirms the G720 has no RT |
| Shader cost | malioc registers, spills and bound pipe for every permutation (G720 target); fragment cycles with and without multiscatter, clearcoat, sheen and geometric specular AA | permutation budget per tier |
| Tile budget | tile count and external bandwidth for RGBA16F vs R11G11B10F scene colour at 4×MSAA | scene-colour format per tier |
| MSAA | GPU time at 1×/4× MSAA, in-subpass vs tonemap-weighted resolve; does the custom resolve stay on tile | whether Arm's 1–2% or the G77 report's 20–30% applies |
| Draw order | front-to-back vs state-sorted opaque on both phones | sort policy at bind |
| Foliage | discard prepass vs A2C prepass + EQUAL on a forest scene | masked-family default on MSAA tiers |
| Post-AA | CMAA2 (compute and fragment port) ms at Medium resolution | whether CMAA2 ships at all |
| TAA | resolve ms and bandwidth with RGBA16F vs 32-bit history; ghosting capture on a yawing jet | history format; k-DOP need |
| Shadows | ms per cascade cold vs cached; caster count with previous-frame fit; D16 vs D32; multiview vs separate; 4-tap vs Castaño 9-tap in the forward pass; capsule and horizon-map cost | shadow tier table |
| Lights | CPU tile + z-bin build ms at 64/256 lights on a medium core; fragment cost at 8/32/64 lights per tile | max-lights default; STB trigger |
| Reflections | 1 vs 2 probe blend ms; normalisation cost | probe count per tier |
| Sky and fog | sky-view and froxel volume ms per update; transmittance/MS LUT ms when amortised | media tier gating |
| Clouds | quarter-res raymarch ms at 32/64 steps; disocclusion quality under fast yaw | Medium cloud tier |
| VFX | overdraw counter and ms for an explosion with and without cutout hulls; six-way vs unlit sprite | particle budgets |
| AO | visibility-bitmask GTAO half-res ms on each phone | whether AO reaches any phone tier |
| Terrain and vegetation | CDLOD patch ms at altitude and ground level; triangles per pixel histogram; impostor 1- vs 2-frame blend | LOD floor and impostor settings |
| Composite | full composite ms with LUT tonemap and dither vs current ACES chain | step 7 payoff |
| Texture filtering | trilinear vs bilinear vs stochastic (High device only, if available) | stochastic filtering tier |
| Sustain | p95/p99 frame time at 60 and 120 fps modes over 30 minutes, with thermal-driven resolution | final tier feature sets |

## Conclusion

The deeper layer gives the same verdict as the pass layer. On phones, the decisive algorithms are the ones that turn per-frame work into cooked data or a closed-form expression:

- normal variance baked into roughness;
- horizon angles baked into terrain tiles;
- capsules in place of shadow-map texels;
- light radius derived from intensity;
- cutout hulls from flipbook alpha;
- VAT rows in place of runtime fracture;
- sky LUTs refreshed only when the sun moves.

Each removes a whole class of runtime cost, and often a class of bug too, which matches the engine's "decide once" design. The desktop techniques the owner might expect to adopt (more cascades, PCSS, SSR, GPU clusters, screen-space SSS, per-pixel ACES, per-pixel ReSTIR) are exactly the ones that need stored depth, extra passes or divergent sampling. On a tiler those cost bandwidth before they add quality.

The newer finding is that the hardware is now splitting the policy. Mali's fragment pre-pass makes draw order irrelevant on the G925 but not on the G720 in the engine's own test phone. Tile size shrinks past 128 bpp. Ray query, OMM and neural accelerators arrive one vendor generation at a time. An engine that hard-codes any of these choices will be wrong on half its devices. The way to stay correct by construction is to make sort policy, scene-colour format, shadow visibility source and upscaler backend into device-bind decisions behind one shader library and one graph, and to let the two phones' measurements set the defaults. The frame anatomy is over budget on paper at Medium and High, so the first benches matter more than any further reading.
