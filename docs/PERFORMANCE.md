# Performance: budgets, measurement, results and optimisations

The single source of truth for what "fast" means in numbers, how it is measured, what has been measured, and every optimisation the engine makes or plans. The designs these numbers serve are in [RENDERING.md](RENDERING.md) and [ARCHITECTURE.md](ARCHITECTURE.md). Rule ([PRINCIPLES.md](PRINCIPLES.md) §3, rule 7): every performance claim has a number from a bench or a device; an estimate is labelled as one.

## 1. Budgets

Frame times are wall clock from present to present; "1% low" is the 99th-percentile frame time. Phone numbers are judged warm, after 30 minutes, with the phone's thermal status recorded. These are targets until the test phones are measured; once measured, tighten them where the hardware allows, never loosen them silently.

| Measure | Low (weak Android) | Medium (the test phones) | High (strong phones) | Desktop (M5) |
|---|---|---|---|---|
| Frame rate | 60 fps (30 on the weakest) | 60 fps; 120 mode | 60 fps, all effects | 60 fps at 1440p; uncapped above |
| GPU frame | ~11 ms | ~11 ms (~6 ms in 120 mode) | ~13 ms | ≤ 12 ms |
| 99th-percentile frame | ≤ 1.25 × target | ≤ 1.25 × target | ≤ 1.25 × target | ≤ 1.2 × target |
| After 30 minutes | ≥ 90% of cold fps | ≥ 90% | ≥ 90% | — |
| Main-thread CPU per frame | ≤ 8 ms | ≤ 6 ms | ≤ 6 ms | ≤ 4 ms |
| Simulation tick (60 Hz) | ≤ 4 ms | ≤ 3 ms | ≤ 3 ms | ≤ 2 ms |
| Extract | ≤ 1 ms | ≤ 1 ms | ≤ 1 ms | ≤ 0.5 ms |
| Draw calls | ≤ 150 | ≤ 300 | ≤ 300 | ≤ 1,000 |
| Instance upload per frame | ≤ 64 KB (static is free) | ≤ 128 KB | ≤ 128 KB | ≤ 512 KB |
| Asset upload per frame | 2 MiB | 2 MiB | 2 MiB | 8 MiB |
| GPU memory | ≤ 600 MB | ≤ 1 GB | ≤ 1 GB | ≤ 2.5 GB |
| Process memory | ≤ 700 MB | ≤ 1 GB | ≤ 1 GB | ≤ 2 GB |
| Cold start to menu | ≤ 6 s | ≤ 4 s | ≤ 4 s | ≤ 3 s |
| Match load | ≤ 10 s | ≤ 6 s | ≤ 6 s | ≤ 4 s |
| Allocations per tick and per frame | 0 after warm-up | 0 | 0 | 0 |
| Shader compiles in play | 0 | 0 | 0 | 0 |
| Network downstream per client | ≤ 40,000 B/s | same | same | same |
| App size | ≤ 150 MB base | same | same | — |

The per-pass GPU estimate for each tier is the frame anatomy in [RENDERING.md](RENDERING.md) §4. The desktop High budget per pass (16.6 ms) is: prepass 1.0, shadows 2.0, GI 1.0, AO 0.6, main lighting 4.5, culling 0.3, particles and transparency 1.5, fog 1.0, reflections 0.8, TAA and post 1.3, UI 0.3, headroom 2.3.

**First performance goal (decided by the owner):** the Low tier holds a steady 60 fps (30 on the weakest phone) warm on the test phones. It comes ahead of desktop image quality: no new desktop-only render feature lands while it is unmet, once the phones are measured.

## 2. How we measure

- **Tiler timing rule.** On tile-based GPUs (Apple, Mali, Adreno) independently timestamped passes overlap: on the M5, bloom passes each read about 3–5 ms inside a 6.3–6.6 ms frame, and composite reads 0.000 ms. **Only the frame time (first pass start to last pass end) is valid for budgets.** Per-pass readings are relative only. A pass's real cost is the GPU frame with it minus without it (A/B). Pass-level effects on phones use Arm Streamline (RMX5085) and Snapdragon Profiler (RMX3853) counters.
- **What every bench case reports** (one JSON line): wall frame p50, p95, p99 and max; GPU frame time; CPU time per named system; draws; triangles; instances; bytes uploaded; allocations per frame; GPU and CPU memory; active MSAA and GPU sample count; cold and after 30 minutes; thermal status.
- **Energy:** ODPM power rails exist only on Pixel 6 and later, so on the realme phones energy uses clock and bandwidth proxies.
- **Quiet machine:** a desktop A/B runs back to back on an idle machine; a run at high load average is recorded as such and does not settle a decision.

## 3. Benches and test scenes

**Engine test scenes** (in `games/sandbox`, the engine's own demo; each is a bench case): air combat with many missiles and tracers; dense terrain with vegetation, rocks and buildings; high-overdraw combat with explosions and smoke; many networked entities with interpolation; continuous streaming across a large world. Large-world features (64-bit positions, terrain, HLOD, streaming, effects) are built against them.

**Look test scenes** (`games/sandbox/content/test_scenes/`, each with approved goldens per tier):

| Scene | Proves |
|---|---|
| `lighting_room` | sun through a window, bounced light, no leaks; against a Blender Cycles render |
| `material_board` | every base material under sun, shade and a point light |
| `shadow_closeups` | cascade transitions, contact shadows, small objects on the ground |
| `reflections` | wet floor, glass, metal, a closed room with no sky leaking in |
| `atmosphere` | sunrise to noon, fog, light shafts |
| `effects_range` | every effect in light and dark |
| `destruction_yard` | breach, floor drop, collapse: frame time and dust |
| `movement_course` | movement timing, camera smoothness |
| `stress_city` | many lights, effects and chunks at once |

**The phone bench** (planned: [vk-0](briefs/vk-0-phone-baseline.md)): the bench moves into `games/sandbox/game/src/bench/`; each `BenchCase` names its grid, tier, MSAA, TAA, texturing and frame count; results go to `bench/phones/<model>/<date>/`; `scripts/phone-bench.sh` installs, runs cold and after 30 minutes, samples `dumpsys thermalservice` and pulls the JSON back. Run before every merge that touches render, app or physics until a device farm makes it automatic.

## 4. Gates that catch slowdowns

- **`bench-gate` (built):** `cargo run --release -p bench-gate -- --device apple-m5` runs the block-grid cases (grid40 TAA, grid40 textured, grid80 TAA, grid80 no-TAA) three times each, keeps each metric's best run, and fails if GPU frame or wall p50 grows more than 5% or wall p99 more than 10% against `bench/baseline/<device>.json`. `--record "why"` rewrites the baseline as a deliberate commit. Per-pass times are not compared. Verified by undoing a TAA optimisation: the gate reported +20 to +27%. Run by hand before merging render work until a GPU CI runner exists.
- **Shader gate (planned):** the Mali Offline Compiler on every shader in CI: no spills, at most 32 registers on hot shaders, no cycle regressions against a recorded baseline.
- **Allocation tests (built):** counting-allocator tests for every per-tick and per-frame path: extract, retained updates and culling (1,000 moving frames, 0 allocations), the HUD and menus, the governor, telemetry, the combat path, netcode capture and apply.
- **Visual goldens (built):** speed-ups never lower quality silently ([ARCHITECTURE.md](ARCHITECTURE.md) §12).
- **Determinism tests** with 0, 1 and all workers guard every threading change.
- **Content budgets (planned):** the cook reports texture memory per map and device class; CI fails a map whose always-resident part does not fit the smallest device. Effect overdraw budgets are validated data.

## 5. Measurements so far

All on the Apple M5 unless stated. **No phone has been measured yet.**

**Retained render scene** (1920×1080, back to back, CPU about 88% idle):

| Case | Before GPU / wall p50 / p99 (ms) | After GPU / wall p50 / p99 (ms) |
|---|---:|---:|
| grid40 TAA | 2.411 / 3.306 / 3.413 | 2.384 / 2.956 / 3.006 |
| grid80 TAA | 2.885 / 5.024 / 5.243 | 2.789 / 3.469 / 3.529 |
| grid80 no-TAA | 2.155 / 4.213 / 4.378 | 2.061 / 2.727 / 2.803 |

CPU medians for grid80 TAA: 0.117 ms cull, 0.017 ms upload and index flatten, 0.161 ms encode and submit.

**True pass costs by A/B** (1920×1080, grid 80, 480 frames, load about 1):

| Pass | GPU frame with / without | Real cost |
|---|---:|---:|
| TAA resolve (16 history loads, 9-pixel depth search) | 3.584 / 2.166 ms | 1.42 ms |
| TAA resolve (5-tap Catmull-Rom, cross depth search) | 2.887 / 2.167 ms | 0.72 ms |
| GTAO + denoise, half resolution | 2.887 / 2.308 ms | 0.58 ms |

The whole High-tier frame of the 80×80 grid is 2.89 ms GPU and 4.0 ms wall p50. The converged TAA image matches the old resolve (mean difference 0/255).

**Render-perf-1 A/B** (grid 80, TAA on, 240 frames, at load average about 30, so indicative only): main 7.75 / 10.00 ms against branch 6.63 / 9.52 ms (GPU frame / wall p50).

**Extract** (release, 240 samples, p50/p99): 40×40 grid 0.036 / 0.042 ms; 80×80 grid 0.103 / 0.139 ms (from 0.073/0.130 and 0.167/0.206 before dense queries).

**Simulation:** a headless six-bot arena averaged 0.675 ms per tick in `dev` and 0.634 ms in `play` over 10,000 ticks, against a 16.67 ms budget.

**Physics determinism modes** (24 characters, 300 dynamic and 200 static boxes, 3,600 steps; machine heavily loaded, so not a decision): default deterministic mean 0.32–0.51 ms, p50 0.26 ms; SIMD mean 0.26–0.40 ms, p50 0.21–0.22 ms. Hashes were identical within each mode across runs and differ between modes, as expected. Remeasure on an idle machine before any change.

**Physics allocations:** Rapier allocates at most 3 times / 640 bytes per warm step on the test scene; character moves over static geometry allocate nothing in our code; on the arena duel scene the step averages about 3 allocations (1,845 over 600 steps) and `move_character_excluding` about 5 per call over static geometry (3,012 over 600). The arena figures exceed the test-scene bound; either the bound is raised with these numbers or Rapier's character path is forked (owner to decide).

**Netcode soak** (1,800 ticks, 24 players and 200 debris, 150 ms RTT, 5% loss, 20 ms jitter, 30 snapshots/s): 9,312 B/s downstream per client at the 1,150-byte datagram cap (target 40,000), 20.04 player and 0.83 debris updates per entity per second; 90 reliable events arrived exactly once in order; zero allocations in server capture, write and encode and client insert, apply and event paths over 600 warm ticks. A server write takes 0.30 ms per tick for 24 clients × 224 entities all changing (from 4.29 ms before slot-indexed records).

**Build times** (cold `cargo build --workspace`): 116 s `dev`, 61 s `play`, 112 s `release`. A one-line change in a game crate rebuilds in 1.7 s (`dev`), 1.4 s (`play`), 29.6 s (`release`); in `engine-world`, 2.0 s, 8.4 s and 49.8 s.

## 6. The optimisation catalogue

Every optimisation the engine makes or plans, by area. Status: **built**, **planned** (in a brief or the render order of work), **gated** (only after a measurement or hardware). Evidence: M = measured by someone on named hardware, C = vendor claim, I = inferred.

### 6.1 Tile memory and the GPU layer

| Optimisation | Expected gain | Status |
|---|---|---|
| Load and store ops derived from the render graph | ~300× fewer external writes in Godot (M) | planned (vk-2) |
| Merge same-pixel passes into one render pass with subpasses; opaque → particles → sky → composite → UI as one pass on Low and Medium | ~55% fewer tiles on Mali-G76 (M) | planned (vk-2) |
| Transient, lazily allocated attachments for depth and MSAA samples | ~50 MB saved in Godot (M) | planned (vk-1, vk-2) |
| MSAA resolved on tile | near free on tilers (M) | built (wgpu); kept |
| Memory aliasing of targets with disjoint lifetimes | memory | planned |
| AFBC/UBWC-safe image usage flags (no STORAGE on the swapchain) | 33% bandwidth cut in Arm's sample (M) | planned (compiler error) |
| ≤ 128 bpp per subpass counting samples; R11G11B10F scene colour | keeps Mali's 64×64 tile (M) | planned |
| Specialisation constants for tiers and material lobes | ~7 ms on Pixel 8 Pro in Godot (M) | planned |
| fp16 by default in fragment shaders; fp16 varyings | removed spilling in Godot (M) | planned |
| Push constants ≤ 16 B, descriptor sets by update rate, immutable samplers, linear pools | part of a 10–20% GPU cut on Pixel 7 / S23 (M) | planned |
| Pipelines built at load, persisted pipeline cache | no compiles in play; faster start | planned |
| Narrow barrier stage masks | vertex/fragment overlap across passes on Mali | planned |
| Pre-rotation in the final pass | avoids compositor rotation | planned |
| Fixed-rate compression (AFRC) on low-frequency intermediates | 65% memory, 38% write bandwidth (M, lossy) | gated |

### 6.2 Geometry and culling

| Optimisation | Expected gain | Status |
|---|---|---|
| Retained scene: stable 64-byte slots, dirty-range uploads | static scenes upload 0 bytes; wall p50 5.0 → 3.5 ms (M, ours) | built |
| Split position and attribute streams; position-only shadow and depth passes | less vertex bandwidth; suits Mali's position-first shading | built |
| Quantised 16-bit positions | ~2.5–3.5× less vertex fetch (I) | built for cooked meshes |
| Dense SoA CPU culling into reused index ranges | 0.12 ms for grid80 (M, ours) | built |
| Per-cascade shadow culling; cached far cascade | shadow cost follows what each cascade shows | built |
| NEON 4-wide sphere tests on the job pool; projected-size cull | CPU time | planned |
| One indexed indirect draw per pipeline bin | fewer CPU calls; 98% device coverage | planned |
| Per-device front-to-back bin order | early-Z on Mali-G720 and Adreno LRZ | planned |
| Mesh LODs with hysteresis, coarser shadow LODs | triangles | built |
| Visibility ranges and HLOD stand-ins | draw distance per tier | planned |
| meshoptimizer clusters with cone culling (V1); cluster DAG (V2) | flat frame time as density grows (M on Nanite/Mali) | planned / gated |
| meshopt disk codecs | 2–4× smaller on disk (M) | planned |
| Occlusion without splitting the main pass | +5% outdoors at best (M, Quest) | gated |

### 6.3 Materials, textures and opacity

| Optimisation | Expected gain | Status |
|---|---|---|
| Cooked `SurfaceClass`; opaque shaders with no alpha input | keeps HSR/LRZ for every opaque draw | planned |
| Masked triangle split; masked-only depth prepass with EQUAL colour pass | discard only on mixed triangles | planned |
| Alpha channel stripped from all-opaque textures | smaller formats | planned |
| ASTC block size by role (6×6, 8×8+) | 2.2–4× texture memory and bandwidth (I) | planned |
| `astc_decode_mode` unorm8; bilinear + 2× aniso; 16-bit samplers | part of Godot's 10–20% (M) | planned |
| Mip streaming within a hard budget | memory on phones | planned |
| Textures transcoded on the job pool; frame-budgeted uploads | no hitches | built |

### 6.4 Lighting, shadows and post

| Optimisation | Expected gain | Status |
|---|---|---|
| Light cache solved on a CPU job thread | GI off the hot GPU | planned |
| CPU tile + z-bin light lists | no 32-light cap, no compute barrier | planned |
| Two fitted cascades, cached and scrolled far tiles, one D16 atlas | shadow bandwidth | planned |
| No phone SSR; ≤ 2 normalised probes | keeps tile residency | planned |
| GTAO at half resolution, High only | 0.58 ms on M5 (M, ours) | built (half res) |
| TAA 5-tap history and cross depth search | 1.42 → 0.72 ms (M, ours) | built |
| Fragment TAAU at ~0.6 render scale | ~1 ms buys back 30–50% of pixel cost (I) | planned |
| One composite with a baked tonemap + grade LUT | removes per-pixel ACES and extra passes | planned |
| Bloom at half resolution and below in R11G11B10F | half the bandwidth of RGBA16F | planned |
| Dynamic resolution without rebuilding targets | a scale change costs nothing, keeps history | built |

### 6.5 CPU, memory and pacing

| Optimisation | Expected gain | Status |
|---|---|---|
| Zero allocation per tick and frame after warm-up | no allocator spikes | built on measured paths |
| Retained UI layout; paint-only changes skip layout | 0 allocations on clean frames | built |
| Dense `NetId` slot tables in extract and replication | extract 0.17 → 0.10 ms; server write 4.29 → 0.30 ms (M, ours) | built |
| Threaded render mode; culling and physics on the pool | uses the phone's fast cores | planned |
| Present timing or Swappy pacing | even frame delivery | planned |
| ADPF performance hints; thermal-driven resolution | sustained fps before throttling (I) | planned |
| Memory-pressure hooks drop streamed mips and cosmetic debris | no OS kill | planned |
| Build profiles and line-tables-only debug info | ~1 s rebuilds; smaller target folders | built |

## 7. Open measurements on the test phones

All through the vk-0 bench, cold and after 30 minutes, with thermal state.

| Area | Measurement | Decides |
|---|---|---|
| Features | the capability list per phone (64-bit image atomics, multi-draw indirect, draw count, local read, raster-order access, image compression control, shading rate, ASTC decode mode, fp16, 16-bit storage, subgroups, ray query, swapchain colour spaces), with driver versions | which tiers exist |
| Baseline | wgpu renderer on every bench case | the comparison vk-3 must beat |
| Tile graph | external read/write bytes and tile counts, split against merged opaque→UI; AFBC-safe flags on the swapchain | that vk-2 delivers |
| Opacity | opaque vs masked vs masked-with-prepass on foliage and fences; LOD dither vs fade-as-masked | the masked prepass default |
| Geometry | float vs quantised streams; CPU cull per 10k instances (NEON vs scalar, core type); indirect vs direct submission | cluster V1 priority |
| Shadows | casters and shadow ms before and after receiver-aware culling and cached tiles | the shadow budget per tier |
| Occlusion | next-frame pyramid gain in a dense city and an open sky | whether occlusion is built |
| Upscaling | engine TAAU vs Arm ASR vs Snapdragon GSR2 at the phone's output size; ghosting on fast cameras and tracers | the upscaler default and render scale |
| Light cache | CPU solve on a big and a little core; probe upload bytes; link data size per layout; invalidation cost per broken building | GI tier settings |
| Textures | memory and bandwidth at 4×4 vs role-based blocks; BC7 after XUASTC on desktop | texture formats |
| Translucency | sorted forward vs weighted blended OIT on explosions at 60 and 120 fps; half-resolution particle break-even | translucency tiers |
| Sustain | p95/p99 and thermal state over 30 minutes with and without ADPF and thermal-driven resolution | pacing value |
| Post | GTAO, contact shadows (no phone numbers are published anywhere) | whether they belong on phone tiers |

## 8. Known defects against these budgets

| Defect | Effect | Fix |
|---|---|---|
| Phones start from desktop defaults (Medium, MSAA 4×, 2048² shadows, 32 lights, full scale); the GPU part of the device profile is not passed to detection | a weak phone gets laptop settings | tier from the device table plus the start-up bench (device-tier brief) |
| The job pool is created but unused; no threaded render mode; physics not parallel | the frame runs on one core | threaded mode, pooled culling, then physics `parallel`, once phone numbers show the CPU limits |
| No prepass and no front-to-back bins on Low and Medium | full overdraw on Mali and Adreno | per-device sort policy; masked-only prepass |
| Light tiles hold a 32-bit mask and re-project every light per tile | 32-light cap | tile + z-bin lists |
| HDR target is RGBA16F everywhere | double bandwidth | R11G11B10F where renderable |
| Text uses fontdue with one SDF size and no shaping | no localisation, soft text at size | parley + MSDF |
| Particles built on the CPU, capped at 12,000 | effect density | GPU particles |

Not yet planned anywhere and worth adding: crash and error reporting from devices; save data per platform; localisation string tables; debug drawing from any crate (compiled out in release); content validation in CI; build-time tracking.
