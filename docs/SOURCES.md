# Sources

Every external reference behind the design, grouped by topic, with the doc section that uses it. The two rendering research reports and the earlier audits in [research/](research/) carry their own full citations with evidence labels (measured, claimed, inferred); the most important rendering sources are repeated here.

Rules for a source: prefer the vendor's or author's own page over a summary; say when a figure came through a search summary or a secondary site; a source about another engine's behaviour is checked against that engine's current documentation before it changes a decision.

## Research reports (dated evidence)

| Report | Date | Covers |
|---|---|---|
| [research/2026-11-17-render-mobile.md](research/2026-11-17-render-mobile.md) | 2026-11-17 | Nanite and Lumen on phones, culling, upscaling, textures, mesh compression, opacity classification, what Godot and Unity leave on the table |
| [research/2026-11-17-render-deep-dive.md](research/2026-11-17-render-deep-dive.md) | 2026-11-17 | shading, shadows, anti-aliasing and post, lights and reflections, sky and effects, terrain and vegetation, GPU micro-architecture, emerging techniques, frame anatomy |
| [research/2026-11-12-ui-engines.md](research/2026-11-12-ui-engines.md) | 2026-11-12 | Unreal, Unity, Bevy, Godot and game UI middleware against our UI |
| [research/2026-11-11-engine-audit.md](research/2026-11-11-engine-audit.md) | 2026-11-11 | audit of the engine at commit 6579bc7: findings, targets, methods per system, roadmap |
| [research/2026-11-11-engine-comparison.md](research/2026-11-11-engine-comparison.md) | 2026-11-11 | threads, frame pipeline, spatial queries, visibility, draw order, transparency, textures, LOD and platforms against Bevy, Godot, Unity and Unreal |

## Rendering on phones (key evidence)

Used in: RENDERING, PERFORMANCE §6.

- [Arm: optimising 3D scenes in Godot on Arm GPUs](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/optimizing-3d-scenes-in-godot-on-arm-gpus) and [part 2](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/optimizing-3d-scenes-in-godot-on-arm-gpus-part-2): derived load/store ops, specialisation constants, fp16
- [Khronos sample: subpasses](https://docs.vulkan.org/samples/latest/samples/performance/subpasses/README.html), [AFBC](https://docs.vulkan.org/samples/latest/samples/performance/afbc/README.html), [image compression control](https://docs.vulkan.org/samples/latest/samples/performance/image_compression_control/README.html), [pipeline barriers](https://docs.vulkan.org/samples/latest/samples/performance/pipeline_barriers/README.html)
- [Android: Godot Vulkan improvements](https://developer.android.com/stories/games/godot-vulkan)
- [Arm: Mori to Nanite, billions of triangles on mobile](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/mori-to-nanite-billions-of-triangles-on-mobile) and [Mali and Unreal's Nanite](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/mali-and-unreal-engine-s-nanite-enabling-the-future-of-mobile-graphics)
- [zeux: billions of triangles in minutes](https://zeux.io/2025/09/30/billions-of-triangles-in-minutes/), [meshoptimizer](https://github.com/zeux/meshoptimizer), [jms55: virtual geometry in Bevy 0.16](https://jms55.github.io/posts/2025-03-27-virtual-geometry-bevy-0-16)
- [Epic: Lumen technical details](https://dev.epicgames.com/documentation/en-us/unreal-engine/lumen-technical-details-in-unreal-engine), [Lumen on mobile](https://dev.epicgames.com/documentation/en-us/unreal-engine/using-lumen-global-illumination-on-mobile-in-unreal-engine), [Lumen performance guide](https://dev.epicgames.com/documentation/en-us/unreal-engine/lumen-performance-guide-for-unreal-engine)
- [Enlighten, GDC 2018](https://www.siliconstudio.co.jp/middleware/enlighten/en/blog/2018/20180406/pdf/gdc2018-enlighten-global-illumination-that-scales.pdf), [Frostbite precomputed GI](https://ea.com/frostbite/news/precomputed-global-illumination-in-frostbite), [EA GIBS, SIGGRAPH 2024](https://advances.realtimerendering.com/s2024/content/EA-GIBS2/Apers_Advances-s2024_Shipping-Dynamic-GI.pdf), [Unity dynamic GI roadmap](https://discussions.unity.com/t/feedback-request-changes-to-unitys-dynamic-gi-roadmap/1660897)
- [Devsh, Vulkanised 2026: GPU-driven rendering on tilers](https://vulkan.org/user/pages/09.events/vulkanised-2026/1415-Lucas-Silva-DevshGraphicsProgramming.pptx.pdf), [gpuinfo: Android core features](https://vulkan.gpuinfo.org/listfeaturescore10.php?platform=android), [gpuinfo: Android extensions](https://vulkan.gpuinfo.org/listextensions.php?platform=android)
- [Arm ASR](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/introducing-arm-accuracy-super-resolution), [Snapdragon GSR 2](https://www.qualcomm.com/developer/blog/2024/10/introducing-snapdragon-game-super-resolution-2), [Arm neural graphics SDK](https://github.com/arm/neural-graphics-sdk-for-game-engines/blob/main/docs/user_guide.md), [AMD FSR manual](https://gpuopen.com/manuals/fidelityfx_sdk/techniques/super-resolution-upscaler/), [WWDC26 MetalFX](https://developer.apple.com/videos/play/wwdc2026/359/)
- [Arm: ASTC textures](https://arm-software.github.io/opengl-es-sdk-for-android/astc_textures.html), [Arm: yet more ASTC compression](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/yet-more-astc-compression), [Android: texture guidance](https://developer.android.com/games/optimize/textures?hl=en), [NVIDIA RTXNTC](https://github.com/NVIDIA-RTX/RTXNTC), [Ubisoft: neural textures in AC Mirage](https://www.ubisoft.com/en-us/studio/laforge/news/415slmB3ZGzv8d2LhO9QT/shipping-neural-texture-compression-in-assassins-creed-mirage)
- [Epic: material blend modes](https://dev.epicgames.com/documentation/en-us/unreal-engine/material-blend-modes-in-unreal-engine), [Godot 3D rendering limitations](https://docs.godotengine.org/en/stable/tutorials/3d/3d_rendering_limitations.html), [Arm: hidden surface removal on Mali](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/hidden-surface-graphics-removal-methods-in-mali-gpus), [Mesa: Adreno LRZ](https://docs.mesa3d.org/drivers/freedreno/hw/lrz.html), [Igalia: Adreno LRZ](https://blogs.igalia.com/dpiliaiev/adreno-lrz/), [Castaño: alpha mipmaps](https://www.ludicon.com/castano/blog/articles/computing-alpha-mipmaps/), [Arm: opacity micromaps](https://learn.arm.com/learning-paths/mobile-graphics-and-gaming/omm-mali-intro/4-build-and-validate-an-omm-adoption-plan/)
- [Arm: Immortalis-G925 fragment pre-pass](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/immortalis-g925-the-fragment-prepass), [Qualcomm mobile best practices](https://docs.qualcomm.com/doc/80-78185-2/topic/mobile_best_practices.html), [Epic: mobile deferred shading](https://dev.epicgames.com/documentation/en-us/unreal-engine/using-the-mobile-deferred-shading-mode-in-unreal-engine)
- [Android: frame pacing extensions](https://developer.android.com/games/develop/vulkan/frame-pacing-extensions), [Android: ADPF performance hints](https://developer.android.com/games/optimize/adpf/performance-hint-api)
- [Pharr et al.: stochastic texture filtering](https://research.nvidia.com/labs/rtr/publication/pharr2024stochtex)

## Engine base and libraries

Used in: ARCHITECTURE §13.

- [Bevy 0.19](https://bevy.org/news/bevy-0-19/)
- [migration 0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/)
- [Bevy 0.17](https://bevy.org/news/bevy-0-17/)
- [Tiny Glade (reported)](https://foro3d.com/en/2026/mayo/rust-y-bevy-en-tiny-glade-construccion-procedural-sin-cuadricula.html)
- [Linebender Q1 2026](https://linebender.org/blog/tmil-25/)
- [Rust GUI survey 2026](https://blog.wybxc.cc/blog/rust-gui-survey-2026/)
- [egui/iced/Slint comparison](https://www.pistack.xyz/posts/2026-08-25-rust-gui-frameworks-egui-iced-slint-comparison/)
- [kira](https://lib.rs/crates/kira)
- [Firewheel](https://lib.rs/crates/firewheel)
- [firewheel-ircam-hrtf](https://lib.rs/crates/firewheel-ircam-hrtf)
- [Are We Game Yet: audio](https://arewegameyet.rs/ecosystem/audio/)
- [lightyear](https://docs.rs/lightyear_netcode)
- [renet2](https://lib.rs/crates/renet2)
- [ozz-animation-rs](https://lib.rs/crates/ozz-animation-rs)
- [skeletal_animation](https://docs.rs/crate/skeletal_animation/0.15.0/source/README.md)
- [Dimforge Q3 2026](https://dimforge.com/blog/2026/10/25/dimforge-Q3-technical-report/)
- [Avian 0.2](https://joonaa.dev/blog/07/avian-0-2)
- [Rapier in Godot comparison](https://godot.rapier.rs/blog/)

## Threads, jobs and the frame pipeline

Used in: ARCHITECTURE §3.

- [Rapier determinism](https://www.rapier.rs/docs/user_guides/rust/determinism)
- [Unreal Tasks System](https://dev.epicgames.com/documentation/en-us/unreal-engine/tasks-systems-in-unreal-engine)
- [Unity Burst float determinism](https://docs.unity3d.com/Manual/burst/float-precision-determinism.html)
- [Unity Android thread configuration](https://docs.unity.com/en-us/engine/6000.3/manual/platform-specific/android/developing/optimization/thread-configuration)
- [Android thread scheduling](https://developer.android.com/agi/sys-trace/threads-scheduling?hl=en)
- [Godot WorkerThreadPool](https://docs.godotengine.org/en/latest/classes/class_workerthreadpool.html)
- [bevy_tasks](https://docs.rs/crate/bevy_tasks/latest)
- [Bevy on wasm with atomics (PR 12205)](https://www.github.com/bevyengine/bevy/pull/12205)
- [deterministic parallel reduction](https://tachyon-beep.github.io/murk/determinism.html)
- [Unreal parallel rendering](https://dev.epicgames.com/documentation/en-us/unreal-engine/parallel-rendering-overview-for-unreal-engine)
- [Unreal low-latency frame syncing](https://dev.epicgames.com/documentation/en-us/unreal-engine/low-latency-frame-syncing-in-unreal-engine)
- [Bevy pipelined rendering](https://doc.qu1x.dev/bevy_trackball/bevy_render/pipelined_rendering/struct.PipelinedRenderingPlugin.html)
- [Bevy issue 5082](https://github.com/bevyengine/bevy/issues/5082)
- [Godot thread-safe APIs](https://docs.godotengine.org/en/4.4/tutorials/performance/thread_safe_apis.html)

## Spatial queries and replication

Used in: ARCHITECTURE §5, §8.

- [Unreal Replication Graph](https://www.unrealengine.com/en-US/tech-blog/replication-graph-overview-and-proper-replication-methods)
- [Grid Spatialization 2D node](https://dev.epicgames.com/documentation/unreal-engine/API/Plugins/ReplicationGraph/UReplicationGraphNode_GridSpatia-)
- [Godot BVH broadphase](https://remotebranch.eu/Stowage/godot/src/commit/3877ed73d01bbbf764d0a2756fe6b310ba9dc6f4/servers/physics_3d/broad_phase_3d_bvh.cpp)
- [Bullet forum: grid vs tree](https://pybullet.org/Bullet/phpBB3/viewtopic.php?p=23672)
- [Real-Time Rendering: collision detection](https://mail.realtimerendering.com/Real-Time_Rendering_4th-Collision_Detection.pdf)

## Platforms, start-up and Android

Used in: ARCHITECTURE §4.

- [Godot renderers](https://docs.godotengine.org/en/4.5/tutorials/rendering/renderers.html)
- [Godot internal rendering architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html)
- [Unreal mobile shading modes](https://dev.epicgames.com/documentation/en-us/unreal-engine/mobile-rendering-and-shading-modes-for-unreal-engine)
- [Bevy 0.16](https://bevy.org/news/bevy-0-16/)
- [Bevy GPU preprocessing](https://doc.qu1x.dev/bevy_trackball/bevy_render/batching/gpu_preprocessing/index.html)
- [wgpu limits](https://wgpu.rs/doc/wgpu/struct.Limits.html)
- [Rust on consoles](https://users.rust-lang.org/t/is-there-a-reliable-way-to-support-mainstream-console-including-std/64451)
- [winit Android NativeActivity](https://docs.rs/winit/0.30/winit/platform/android/index.html)
- [Android logging](https://developer.android.com/ndk/reference/group/logging)

## The GPU layer (Vulkan)

Used in: RENDERING §2.

- [KosmicKrisp](https://www.phoronix.com/news/KosmicKrisp-Vulkan-1.4-2026)
- [Slang](https://shader-slang.org/)
- [KosmicKrisp on iOS](https://www.phoronix.com/news/KosmicKrisp-2026)
- [dynamic rendering local read](https://docs.vulkan.org/tutorial/latest/Building_a_Simple_Engine/Advanced_Topics/Dynamic_Rendering_Local_Read.html)
- [Arm: framebuffer fetch in Vulkan](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/framebuffer-fetch-in-vulkan)
- [Vulkan: streamlining subpasses](https://vulkan.org/blog/streamlining-subpasses)

## Visibility and occlusion (as surveyed in 2026-11-11)

Used in: RENDERING §6.

- [wgpu 28](https://github.com/gfx-rs/wgpu/releases/tag/v28.0.0)
- [wgpu multi-draw on Metal (PR 9640)](https://github.com/gfx-rs/wgpu/pull/9640)
- [wgpu issue 2148](https://github.com/gfx-rs/wgpu/issues/2148)
- [two-pass HZB on tilers (Vulkanised 2026)](https://vulkan.org/user/pages/09.events/vulkanised-2026/1415-Lucas-Silva-DevshGraphicsProgramming.pptx.pdf)
- [Bevy two-pass occlusion culling (PR 17413)](https://github.com/bevyengine/bevy/pull/17413)
- [Unity GPU occlusion culling](https://docs.unity3d.com/6000.3/Documentation/Manual/urp/gpu-culling.html)
- [Unreal visibility and occlusion culling](https://dev.epicgames.com/documentation/en-us/unreal-engine/visibility-and-occlusion-culling-in-unreal-engine)
- [Godot occlusion culling](https://docs.godotengine.org/en/stable/tutorials/3d/occlusion_culling.html)

## Draw order and transparency

Used in: RENDERING §7, §14.

- [MJP on weighted blended OIT](https://therealmjp.github.io/posts/weighted-blended-oit)
- [McGuire and Bavoil 2013](https://www.jcgt.org/published/0002/02/09/paper.pdf)
- [AVBOIT, SIGGRAPH 2025](https://www.advances.realtimerendering.com/s2025/content/AVBOIT_SIG2025_MDROBOT-final.pdf)
- [Unreal translucency sorting](https://dev.epicgames.com/documentation/en-us/unreal-engine/sorting-translucent-objects-in-unreal-engine)
- [Bevy order-independent transparency (0.15)](https://bevy.org/news/bevy-0-15/)

## Textures

Used in: RENDERING §9.

- [Unity mipmap streaming](https://docs.unity3d.com/Manual/TextureStreaming-API.html)
- [Godot 4.8 dev 5: texture streaming](https://godotengine.org/article/dev-snapshot-godot-4-8-dev-5/)
- [Unreal texture streaming metrics](https://dev.epicgames.com/documentation/en-us/unreal-engine/texture-streaming-metrics-in-unreal-engine)
- [KTX2 / Basis Universal](https://github.com/BinomialLLC/basis_universal)
- [DirectX sampler feedback](https://devblogs.microsoft.com/directx/coming-to-directx-12-sampler-feedback-some-useful-once-hidden-data-unlocked/)

## Instances, LOD and HLOD

Used in: RENDERING §8.

- [Godot visibility ranges](https://docs.godotengine.org/en/stable/tutorials/3d/visibility_ranges.html)
- [Bevy VisibilityRange](https://docs.rs/bevy/latest/bevy/render/view/struct.VisibilityRange.html)
- [Unity HLODSystem](https://github.com/Unity-Technologies/HLODSystem)
- [Unreal destructible HLOD component](https://dev.epicgames.com/documentation/unreal-engine/API/Runtime/Engine/UWorldPartitionDestructibleHLODC-)
- [Destruction in Frostbite (SIGGRAPH 2010)](https://www.advances.realtimerendering.com/s2010/Kihl-Destruction%20in%20Frostbite(SIGGRAPH%202010%20Advanced%20RealTime%20Rendering%20Course).pdf)

## Content pipeline

Used in: CONTENT.

- [meshopt](https://docs.rs/crate/meshopt/latest)
- [basisu](https://docs.rs/crate/basisu/0.1.0)
- [eulumdat-gltf-pack (same stack)](https://docs.rs/crate/eulumdat-gltf-pack/0.7.0)
- [glTF Transform](https://gltf-transform.dev/)

## UI

Used in: ARCHITECTURE §10.

- [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/)
- [Emulation domain](https://chromedevtools.github.io/devtools-protocol/tot/Emulation/)
- [wgpu 30 Metal backend compiles with `preserveInvariance` (invariant depth on the dev Mac until the Vulkan move; RENDERING §3)](https://github.com/gfx-rs/wgpu/blob/v30.0.1/wgpu-hal/src/metal/device.rs#L254-L259)
