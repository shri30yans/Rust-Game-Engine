# How other engines do it

> Dated evidence, not the source of truth. The current design is in the topic docs listed in [../README.md](../README.md) §1; old doc names and decision numbers cited below refer to docs merged on 2026-11-17 (see [../README.md](../README.md#7-former-decision-numbers) §7).

For each engine design choice, this file shows what **Bevy**, **Godot**, **Unity** and **Unreal** do, what we do, and why. The decisions themselves are in [decision index](../README.md); this file is the evidence behind them. Checked 2026-11-11 against each engine's current docs and source. Re-check a row before changing the decision it supports.

Short verdict: our architecture holds up. On most topics it matches what the newest engine versions moved to (Bevy 0.16, Unity 6, Godot 4.8). It differs in three places on purpose, each because buildings break: nothing about visibility is baked, determinism comes from how work is split, and HLOD uses parent stand-ins that damage can switch off.

## 1. Threads and jobs (D25)

| Engine | What it does |
|---|---|
| Bevy | three pools: compute, async compute and I/O; ECS systems run in parallel on the compute pool. Single-threaded on the web. |
| Godot | one `WorkerThreadPool` with high and low priority; physics and rendering can each run on their own thread. |
| Unity | a job system with worker threads (count set by `-job-worker-count`); Burst compiles jobs. Cross-platform identical float results only with Burst's deterministic float mode, which is 64-bit only. On Android, Unity sorts cores into big and little and advises against pinning threads. |
| Unreal | a Tasks system with high, normal and background priorities and separate foreground and background workers. |
| **Us** | one rayon job pool plus a lower-priority background pool. Worker count comes from the device profile: desktop cores − 2, phones performance cores − 1, web 0. Threads are not pinned. Determinism comes from fixed chunks and ordered combining, and is tested with 0, 1 and all workers. |

**Why ours:** the shape is the same as all four: a pool for frame work, plus background work that never blocks the frame. What we add is determinism that does not depend on a compiler mode or on the thread count. Our netcode needs that (D6), and it lets the single-threaded web build simulate exactly like the server.

## 2. Render thread and frame pipeline (D26)

| Engine | What it does |
|---|---|
| Bevy | pipelined rendering: the render world is extracted, then drawn on another thread while the next frame simulates. Off on the web, because wgpu objects cannot move between threads there. |
| Godot | single-threaded rendering by default; the multi-threaded render model is optional and has known bugs. |
| Unity | a render thread, plus "graphics jobs" that record commands on workers. |
| Unreal | game, render and RHI threads, one frame apart by default, with a setting to sync tighter and cut latency. |
| **Us** | a main thread and a render thread with an extract step (Bevy's model), a late camera so aiming has no extra delay, and a single-threaded mode for the web, few-core devices and tests. |

**Why ours:** a separate render thread is standard. The web fallback is forced by the browser. The late camera answers Unreal's latency concern without a third thread.

## 3. Spatial queries (D27)

| Engine | What it does |
|---|---|
| Bevy | no shared spatial index: culling is per entity (CPU, or GPU where supported); physics plugins keep their own broadphase. |
| Godot | a dynamic AABB tree (BVH) for both render culling and the physics broadphase. |
| Unity | physics (PhysX) has its own broadphase; rendering culls per renderer; no public shared index. |
| Unreal | physics (Chaos) has its own acceleration structure; network relevance uses the Replication Graph, whose main node is a 2D grid. |
| **Us** | Rapier answers collision questions. A sparse hashed cubic grid answers "what is near" (network relevance, coarse culling, sounds, AI, effects). Objects that cover many cells go to a "large objects" list. |

**Why ours:** collision and "near" are different questions. Every engine keeps the physics structure separate, and Unreal's Fortnite-scale relevance uses a grid. A grid is O(1) to update when objects move, and our worlds have many moving pieces after destruction. Making it sparse removes the only weakness of a fixed grid: worlds of unknown size.

## 4. Visibility and occlusion (D28)

| Engine | Frustum | Occlusion | Phones and web |
|---|---|---|---|
| Bevy 0.16 | GPU where supported, else CPU | two-pass depth pyramid, experimental opt-in | GPU path turned off on WebGL2 and on Adreno 6xx after driver crashes |
| Godot 4 | CPU, through its BVH | the CPU rasterises baked static occluders (Embree) | occlusion culling off on the web |
| Unity 6 | CPU, or GPU with the GPU Resident Drawer | GPU occlusion culling (needs compute and indirect draws) | works on capable phones; older ones fall back |
| Unreal 5 | CPU, or GPU with Nanite | hardware queries, depth pyramid; Nanite's own two-pass culling | software occlusion and precomputed visibility volumes; queries arrive two frames late on phones |
| **Us** | GPU on the GPU-driven path, CPU per grid cell on the Direct path | two-pass depth pyramid on the GPU-driven path; none at first on Direct | Direct path on the web, older phones and GPUs on the known-bad list |

**Why ours:** our GPU-driven path is the design Bevy and Unity moved to most recently, and Nanite uses the same two-pass idea. We reject every baked method Godot and Unreal use on phones, because a falling wall makes them wrong. The Direct path is the honest fallback where compute is weak or buggy.

## 5. Draw order (D29)

| Engine | Opaque | See-through |
|---|---|---|
| Bevy | "binned" phases: bins keyed by pipeline and material; only bins are sorted | "sorted" phases, back to front |
| Godot | sorted by material and state, with a depth prepass on Forward+ | sorted per object; `render_priority` |
| Unity | SRP Batcher groups by shader; render queues | sorted per object; render queue and sorting priority |
| Unreal | mesh draw commands cached and merged by state | sorted per object by distance; `TranslucencySortPriority` |
| **Us** | binned by pipeline, material, mesh | sorted by a 64-bit key: material priority, distance, instance id |

**Why ours:** binning is Bevy's measured improvement over sorting everything (fewer items to sort), and the sort priority is what artists in every other engine rely on.

## 6. Transparency (D30)

| Engine | What it does |
|---|---|
| Bevy | sorted per object; opt-in per-pixel-list OIT (memory heavy) |
| Godot | sorted per object; no OIT |
| Unity | sorted per object; no built-in OIT |
| Unreal | sorted per object; OIT is experimental and costly |
| **Us** | sorted per object (glass), unsorted additive effects, particles sorted per emitter, thick collapse dust as volumetric fog (or large lit particles on phones) |

**Why ours:** no shipping engine relies on OIT by default. Moving thick dust into the fog solves the one case where sorting fails for us.

## 7. Textures in memory (D31)

| Engine | Streaming | Phone formats |
|---|---|---|
| Bevy | none built in | KTX2/Basis support; ASTC if the GPU has it |
| Godot | mip streaming arrived in 4.8 | ETC2 and ASTC |
| Unity | mipmap streaming: mesh UV density, camera and a memory budget | ASTC (ETC2 fallback) |
| Unreal | wanted mips from texel density within a pool; virtual textures optional | ASTC |
| **Us** | mip streaming from cooked texel density and the visible instances, within the device profile's budget; smallest mips always loaded | one KTX2/Basis source transcoded to BC, ASTC or ETC2 at load |

**Why ours:** this is Unity's and Unreal's method. Phones make it necessary from the start, because the OS kills an app that runs out of memory. One source file with transcoding avoids keeping texture sets per platform.

## 8. Instances, LOD and HLOD (D32)

| Engine | What it does |
|---|---|
| Bevy | automatic instancing; `VisibilityRange` with margins, used for LOD and an HLOD example |
| Godot | visibility ranges with begin, end, margins, a parent and fade modes; used for LOD and HLOD |
| Unity | LOD groups; the GPU Resident Drawer instances automatically; an HLOD package builds merged clusters |
| Unreal | Nanite, or LODs plus HLOD from merged clusters; a special component tracks damaged actors in HLOD |
| **Us** | every piece is an instance; unbreakable geometry merged per cell; one visibility-range mechanism for LOD, cut-off and HLOD parents; damage turns a building's parent off |

**Why ours:** Godot's and Bevy's visibility ranges are the simplest general mechanism. Damage then only flips one parent, where Unreal needs a special component for destructible HLODs.

## 9. Platforms and render paths (D33)

| Engine | Renderers | Phones | Web | Consoles |
|---|---|---|---|---|
| Bevy | one renderer; features turned on or off by GPU capability | iOS, Android | WebGPU and WebGL2 | none official |
| Godot | three: Forward+ (desktop), Mobile (single-pass forward for tile GPUs), Compatibility (GLES/WebGL2) | Mobile renderer | Compatibility only | through third-party porters |
| Unity | two pipelines: URP (all devices) and HDRP (high-end) | URP | WebGL2 and WebGPU | yes |
| Unreal | one renderer with a separate mobile shading path (forward by default, deferred optional) | mobile path | none current | yes |
| **Us** | one renderer with two capability paths (GPU-driven and Direct) and separate quality tiers | Metal and Vulkan | WebGPU only | later, as a backend behind `engine-render` |

**Why ours:** Godot's three renderers and Unity's two pipelines duplicate features and drift apart. Bevy's single renderer with capability switches matches "one canonical path per concern" (AGENTS rule 1). We drop WebGL2, which has no compute shaders: Godot needs a whole third renderer to support it. Consoles need a proprietary graphics backend under NDA, so every GPU call lives in one crate, `engine-gpu` (Vulkan today, D82).

## Sources

- Bevy: [0.16 release notes](https://bevy.org/news/bevy-0-16/), [two-pass occlusion culling (PR 17413)](https://github.com/bevyengine/bevy/pull/17413), [GPU preprocessing](https://doc.qu1x.dev/bevy_trackball/bevy_render/batching/gpu_preprocessing/index.html), [pipelined rendering](https://doc.qu1x.dev/bevy_trackball/bevy_render/pipelined_rendering/struct.PipelinedRenderingPlugin.html), [wasm threads (PR 12205)](https://www.github.com/bevyengine/bevy/pull/12205), [bevy_tasks](https://docs.rs/crate/bevy_tasks/latest), [VisibilityRange](https://docs.rs/bevy/latest/bevy/render/view/struct.VisibilityRange.html), [0.15 OIT](https://bevy.org/news/bevy-0-15/).
- Godot: [renderers](https://docs.godotengine.org/en/4.5/tutorials/rendering/renderers.html), [internal rendering architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html), [occlusion culling](https://docs.godotengine.org/en/stable/tutorials/3d/occlusion_culling.html), [visibility ranges](https://docs.godotengine.org/en/stable/tutorials/3d/visibility_ranges.html), [WorkerThreadPool](https://docs.godotengine.org/en/latest/classes/class_workerthreadpool.html), [thread-safe APIs](https://docs.godotengine.org/en/4.4/tutorials/performance/thread_safe_apis.html), [4.8 dev 5 texture streaming](https://godotengine.org/article/dev-snapshot-godot-4-8-dev-5/).
- Unity: [GPU occlusion culling](https://docs.unity3d.com/6000.3/Documentation/Manual/urp/gpu-culling.html), [Android thread configuration](https://docs.unity.com/en-us/engine/6000.3/manual/platform-specific/android/developing/optimization/thread-configuration), [Burst float determinism](https://docs.unity3d.com/Manual/burst/float-precision-determinism.html), [mipmap streaming](https://docs.unity3d.com/Manual/TextureStreaming-API.html), [HLODSystem](https://github.com/Unity-Technologies/HLODSystem).
- Unreal: [parallel rendering](https://dev.epicgames.com/documentation/en-us/unreal-engine/parallel-rendering-overview-for-unreal-engine), [low-latency frame syncing](https://dev.epicgames.com/documentation/en-us/unreal-engine/low-latency-frame-syncing-in-unreal-engine), [Tasks System](https://dev.epicgames.com/documentation/en-us/unreal-engine/tasks-systems-in-unreal-engine), [visibility and occlusion culling](https://dev.epicgames.com/documentation/en-us/unreal-engine/visibility-and-occlusion-culling-in-unreal-engine), [mobile shading modes](https://dev.epicgames.com/documentation/en-us/unreal-engine/mobile-rendering-and-shading-modes-for-unreal-engine), [Replication Graph](https://www.unrealengine.com/en-US/tech-blog/replication-graph-overview-and-proper-replication-methods), [texture streaming metrics](https://dev.epicgames.com/documentation/en-us/unreal-engine/texture-streaming-metrics-in-unreal-engine), [translucency sorting](https://dev.epicgames.com/documentation/en-us/unreal-engine/sorting-translucent-objects-in-unreal-engine), [destructible HLOD component](https://dev.epicgames.com/documentation/unreal-engine/API/Runtime/Engine/UWorldPartitionDestructibleHLODC-).
- Platforms: [wgpu limits](https://wgpu.rs/doc/wgpu/struct.Limits.html), [Android thread scheduling](https://developer.android.com/agi/sys-trace/threads-scheduling?hl=en), [Rust on consoles](https://users.rust-lang.org/t/is-there-a-reliable-way-to-support-mainstream-console-including-std/64451), [Rapier determinism](https://www.rapier.rs/docs/user_guides/rust/determinism).
