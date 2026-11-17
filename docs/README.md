# Engine platform: start here

This repository is **our own Rust game engine, built as a platform**, plus the games made on it. The engine knows nothing about any one game, so new games are built on it without touching the others. Its goal is Unreal-quality 3D on phones at 60–120 fps, highly optimised end to end, on our own Vulkan layer. Read this file first in every new session.

| Game | Folder | Start at |
|---|---|---|
| **IMAGINE** (working title): a multiplayer arena shooter whose arenas change during the match | `games/imagine/` | [../games/imagine/docs/README.md](../games/imagine/docs/README.md) |
| **Sandbox**: the engine-only demo, home of the engine test scenes and benches | `games/sandbox/` | `games/sandbox/docs/README.md` |

## 1. The docs: one topic, one file

Each file below is the single source of truth for its topic, with decisions written inline as the current design with their reasons and rejected alternatives. There is no separate decision log.

| Topic | File | Answers |
|---|---|---|
| Engineering rules | [../AGENTS.md](../AGENTS.md) | the tenets, quality bar for code, definition of done, who builds |
| Why, and how we decide | [PRINCIPLES.md](PRINCIPLES.md) | the goal, first principles from phone hardware, design rules, how decisions are recorded, the player-facing quality bar |
| How the engine is built | [ARCHITECTURE.md](ARCHITECTURE.md) | engine / kits / games, crates and layers, simulation, threads, start-up and platforms, spatial queries, physics, destruction, online play, UI and input, audio, testing, libraries, one path per concern, risks |
| Rendering | [RENDERING.md](RENDERING.md) | the Vulkan layer, the render graph, frame anatomy per tier, retained scene, culling, surface classes, geometry, textures, shading, shadows, lights, light cache, sky and effects, tiers and pacing, AA and post, terrain, order of work |
| Content | [CONTENT.md](CONTENT.md) | data not code, content packs, folders, Blender rules and tags, cooking (meshes, textures, collision, shaders), levels, prefabs, live edits, budgets, licences |
| Performance | [PERFORMANCE.md](PERFORMANCE.md) | budgets, how we measure, benches and test scenes, gates, every measurement so far, the optimisation catalogue, open phone measurements, known defects |
| Sources | [SOURCES.md](SOURCES.md) | every external reference by topic, and the dated research reports |
| How work gets done | [WORKFLOW.md](WORKFLOW.md) | briefs, build, review, merge, gates, goldens, keeping docs true |
| Task briefs | [briefs/README.md](briefs/README.md) | each task's design and status |
| Research (dated evidence) | [research/](research/) | the rendering research reports, the 2026-11-11 audit, engine and UI comparisons; evidence, not the source of truth |

Each game keeps its plan, look, design choices, asset list, UI design system and references in `games/<name>/docs/`. If two files seem to disagree, the file that owns the topic wins, and the other one is fixed.

## 2. Where things stand

| Area | State |
|---|---|
| `engine/core`, `engine/jobs`, `engine/world` | built, tested |
| `engine/platform` | built: unified desktop/Android entry, device profile, touch regions, content opening, presentation binding |
| `engine/gpu` | planned: the Vulkan layer on `ash`, the one `unsafe` crate ([vk-1](briefs/vk-1-gpu.md)) |
| `engine/render` | built on wgpu: forward HDR, retained scene, 2 cascades, GTAO, bloom, TAA on High/Ultra (MSAA 4× elsewhere), dynamic resolution, cooked PBR; moving to `engine-gpu` and a render graph (vk-1 to vk-3) |
| `engine/assets`, `tools/cook`, `tools/texture-encode` | built: validated packs, cooked meshes, LODs, Basis textures, collision |
| `engine/physics`, `engine/destruction`, `engine/netcode`, `engine/sessions` | built, tested (sessions not yet carried over QUIC) |
| `engine/scene` | built: one Level schema for Blender and handwritten levels, prefabs, live edits |
| `engine/ui`, `tools/ui-cook` | built: v3 documents from HTML/CSS, retained layout, menus, gradients, Chrome conformance at three sizes |
| `engine/audio` | slice 1 built |
| `engine/app` | built: single-threaded client, headless and server runners, travel, asset uploader, telemetry |
| `engine/spatial`, `engine/animation`, `engine/devtools`, `engine/light` | planned |
| `kits/fps` | built; other kits planned |
| `games/sandbox`, `games/imagine` | built; both build signed Android APKs; **no phone has been measured or played on yet** |

**Now:** the Vulkan move: [vk-0-phone-baseline](briefs/vk-0-phone-baseline.md) on `main` and [vk-1-gpu](briefs/vk-1-gpu.md) on the `vulkan` branch in parallel, then vk-2 and vk-3. Engine work follows [RENDERING.md](RENDERING.md) §19 and the engine test scenes; IMAGINE's work follows its [PLAN.md](../games/imagine/docs/PLAN.md). Task status: [briefs/README.md](briefs/README.md).

## 3. Setup

| Need | Detail |
|---|---|
| Rust | stable 1.99+ (edition 2024) |
| Blender | `/Applications/Blender.app/Contents/MacOS/Blender` (not on PATH), used headless by `tools/blender/*.py` |
| Basis Universal | `tools/texture-encode` builds the vendored encoder through `basisu_c_sys` 0.9.1 |
| Vulkan SDK (from vk-1) | LunarG SDK with KosmicKrisp on the Mac, validation layers, and `slangc` for shaders |
| Google Chrome | headless, for UI conformance tests and mockup PNGs |
| Git LFS | for `.blend`, textures and audio ([CONTENT.md](CONTENT.md) §3) |
| Placeholder audio | `cargo run -p sfx-gen` regenerates IMAGINE's temporary WAVs |
| Dev machine | Apple M5, 24 GB: the desktop reference |
| Test phones | realme RMX5085 (Mali-G720, 2800×1280) and realme GT 6T RMX3853 (Snapdragon 7+ Gen 3, Adreno 732) |
| Android toolchain | Android SDK (platform 36, build-tools) and NDK, linked with the NDK clang (no `cargo-ndk`) |

## 4. Commands

```bash
scripts/gate.sh
```

```bash
scripts/gate.sh --full
```

```bash
cargo run -p check-layers
```

```bash
cargo run --release -p bench-gate -- --device apple-m5
```

```bash
scripts/build-android.sh imagine
```

## 5. Open questions (need the owner)

- The game title (working title IMAGINE).
- Whether to drop cross-platform physics bit-identity for Rapier's SIMD path ([ARCHITECTURE.md](ARCHITECTURE.md) §2), once measured on an idle machine.
- Rapier's arena allocations exceed the test-scene bound: raise the bound with the measured numbers or fork the character path ([PERFORMANCE.md](PERFORMANCE.md) §5).
- Proposed and waiting for a yes: the animation runtime ([ARCHITECTURE.md](ARCHITECTURE.md) §11); IMAGINE's working title and "Work Site" UI motif ([IMAGINE README](../games/imagine/docs/README.md) §1).
- Free or paid, before launch.

## 6. New session: how to continue

1. Read this file, then [../AGENTS.md](../AGENTS.md), then [PRINCIPLES.md](PRINCIPLES.md), then the doc for the area you are working on, then [briefs/README.md](briefs/README.md) (what is in flight).
2. Run `scripts/gate.sh --full` to see the real state of the code.
3. Take the first `ready` brief, or write the next one from [RENDERING.md](RENDERING.md) §19 (engine) or the game's plan, using the template in [WORKFLOW.md](WORKFLOW.md) §3.
4. Build, review and merge as in [WORKFLOW.md](WORKFLOW.md) §2.
5. Update the docs in the same change: the brief's status, ARCHITECTURE §1 crate states, §2 of this file, the owning doc for any decision, and the game's docs for game work.

To start a **new game**: `scripts/new-game.sh NAME`, write `games/NAME/docs/README.md` and a plan, add its zones to `tools/check-layers/layers.json` ([ARCHITECTURE.md](ARCHITECTURE.md) §0).

## 7. Former decision numbers

Until 2026-11-17 decisions were numbered D1–D83 in a separate log. Code comments and older briefs still cite them. Each now lives here:

| Old | Now | Old | Now |
|---|---|---|---|
| D1, D2, D3 | IMAGINE README §1 (Product) | D44 | ARCHITECTURE §10 (touch); IMAGINE README §1 (controls) |
| D4 | ARCHITECTURE §13 | D45 | ARCHITECTURE §10 (telemetry); §4 (presentation binding) |
| D5 | AGENTS.md; RENDERING §2 (the `unsafe` exception) | D46 | ARCHITECTURE §10 (UI documents) |
| D6 | ARCHITECTURE §2 | D47–D50 | IMAGINE README §1 (controls, HUD, settings, bots) |
| D7 | ARCHITECTURE §6 | D51, D52 | IMAGINE README §1 (arena and destruction); ARCHITECTURE §7 |
| D8 | RENDERING §17 | D53 | CONTENT §1; PERFORMANCE §5 (build times) |
| D9, D10, D11 | ARCHITECTURE §10 | D54 | PERFORMANCE §1 (first goal); RENDERING §5, §15, §16 |
| D12, D14 | ARCHITECTURE §11 | D55 | PRINCIPLES §3 (rule 3) |
| D13 | ARCHITECTURE §8 | D56 | RENDERING §5 |
| D15 | CONTENT §4 | D57 | ARCHITECTURE §8 (sessions) |
| D16 | CONTENT §3 | D58 (physics) | ARCHITECTURE §6 |
| D17 | RENDERING §1 | D58 (command path) | ARCHITECTURE §8 |
| D18 | replaced by the light cache, RENDERING §13 | D59, D60, D64 | ARCHITECTURE §8 (travel) |
| D19 | RENDERING §15 | D61 | RENDERING §5 |
| D20, D21, D23 | IMAGINE README §1 | D62 | IMAGINE README §1 (short engagements); ARCHITECTURE §9 |
| D22 | WORKFLOW §1 | D63 | CONTENT §4.4, §6 |
| D24 | ARCHITECTURE §0 | D65 | IMAGINE README §1 (mode capacity); ARCHITECTURE §10 (text fit), §4 (Android lifecycle) |
| D25, D26, D35 | ARCHITECTURE §3 | D66 | ARCHITECTURE §10; RENDERING §17 |
| D27 | ARCHITECTURE §5 | D67 | CONTENT §5.2; ARCHITECTURE §1 |
| D28 | RENDERING §6 | D68, D71 | RENDERING §3 |
| D29 | RENDERING §7 | D69 | ARCHITECTURE §10 (touch) |
| D30 | RENDERING §7, §14 | D70 | RENDERING §13 (light cache; brief light-cache-1) |
| D31 | RENDERING §9 | D72 | CONTENT §2; ARCHITECTURE §4 |
| D32 | RENDERING §8 | D73 | ARCHITECTURE §4 |
| D33 | ARCHITECTURE §4 | D74 | RENDERING §15 (brief device-tier) |
| D34 | ARCHITECTURE §6; PERFORMANCE §5 | D75 | ARCHITECTURE §10 (brief input-actions) |
| D36, D37 | ARCHITECTURE §2 | D76, D77 | ARCHITECTURE §12 |
| D38 | ARCHITECTURE §4 | D78, D79 | ARCHITECTURE §9 (briefs match-events, weapons-data) |
| D39 | RENDERING §16 | D80 | ARCHITECTURE §10 |
| D40, D42 | ARCHITECTURE §9; §6 | D81 | RENDERING §10 (brief stylized-lighting) |
| D41 | CONTENT §6 | D82 | RENDERING §2 |
| D43 | RENDERING §15 | D83 | PRINCIPLES §1; PERFORMANCE §3 |
