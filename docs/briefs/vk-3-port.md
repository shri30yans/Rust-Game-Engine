# vk-3-port: the rest of the renderer on Vulkan, wgpu deleted, and the phone numbers compared (D82)

Goal: one renderer, on Vulkan, at least as good as wgpu on every case and faster on the phones. Done means:
1. GTAO, TAA, bloom, dynamic resolution and every view mode run on `engine-gpu`; every golden at every tier passes with no skips.
2. wgpu, naga and every `.wgsl` file are gone from the workspace (`cargo tree` shows neither).
3. A steady frame allocates nothing on the CPU, at every tier (folds in `render-alloc-0`).
4. Opaque bins are drawn nearest-first; light tiles hold more than 32 lights with one projection per light; the HDR target is `B10G11R11_UFLOAT` where `GpuCaps` says it is renderable (folds in `render-perf-2`).
5. No file in `engine/render`, `engine/platform` or `engine/app` is over 500 lines of non-test code.
6. The vk-0 bench runs on the M5 and both phones; the results sit next to vk-0's in `bench/phones/`, and PERFORMANCE §1 shows wgpu against Vulkan per case, cold and after 30 minutes.
7. `scripts/gate.sh --full` and `scripts/build-android.sh imagine` pass; the `vulkan` branch is ready to merge into `main`.

Starts after vk-2 merges into `vulkan`. Lands on `vulkan`.

Read first: everything in vk-2's list; `docs/briefs/render-alloc-0.md` and `docs/briefs/render-perf-2.md` (their goals are part of this brief); `docs/research/2026-11-11-engine-audit.md` H7, M1, M4, C3; `docs/README.md` §7 D54.

## Design

- **Passes.** GTAO and denoise (compute), TAA (resolve, history, motion vectors from the prepass), bloom (down and up chain), exposure, and the governor's viewport dynamic resolution with retained full-size targets (audit C3: a scale change rebuilds nothing and keeps TAA history). Each is a `PassDesc` in the graph; the graph decides merging and transience.
- **Bloom on phones.** Prefer fewer, cheaper levels on Low (RENDERING tier table) and let the graph alias the mip chain; measure, do not assume.
- **Allocation.** Find each steady-frame allocation by backtrace and remove it at its cause (pre-sized buffers, reused scratch). Tests at every tier and for the IMAGINE match frame prove 0.
- **Bins and lights.** Opaque bins sorted by the distance of their nearest visible instance (a key sort, not a per-instance sort). Light tiles: one compute pass projects each light once to a screen rectangle; tiles test rectangles; the tile list is a variable-length list per tile with a fixed total capacity checked at the settings boundary.
- **HDR format.** `B10G11R11_UFLOAT` for the HDR colour and bloom targets when renderable, else `R16G16B16A16_SFLOAT`. The choice is made once from `GpuCaps` in the graph compile; it is a format, not a second code path.
- **GPU timers.** One timestamp pair per pass; the frame spans the first start to the last end; on tilers only the frame time is budget-valid (unchanged rule, PERFORMANCE §1).
- **File sizes.** `engine/render/src/gfx/mod.rs` (2,160 lines), `engine/render/src/scene/mod.rs` (1,038) and `engine/app/src/client.rs` (1,719) are rewritten or split by responsibility as they are ported.
- **What gets deleted.** wgpu and naga in `Cargo.toml` and `Cargo.lock`; every `.wgsl`; the wgpu backend files; `render-alloc-0` and `render-perf-2` rows become `dropped (folded into vk-3-port)`.

Tests:
- Every golden at every tier, no skips; TAA convergence test and GTAO A/B cost test ported.
- Dynamic resolution: a scale change creates no GPU resource (count from `engine-gpu`), and TAA history survives.
- Zero allocation per steady frame at Low, Medium, High and Ultra, and in the IMAGINE match frame.
- Light tiles: 64 and 256 lights give the same lit pixels as a brute-force reference shader at fixed points.
- Bin order: nearer bins are recorded first (unit test on keys).
- HDR format choice from fixture `GpuCaps`.
- Validation log empty in every GPU test; `scripts/android-test.sh engine-render` passes on both phones.

Do not touch: simulation crates, kits, game logic.

When a detail is not specified: pick the simplest correct design that keeps one code path, state it in the report, and continue. Stop only if the goal itself cannot be met.

Docs in the same change: this brief's status; `render-alloc-0` and `render-perf-2` rows; PERFORMANCE §1 comparison table; ARCHITECTURE §1 and §2.4; README §2; docs/research/2026-11-11-engine-audit.md C3, H7, M1, M4; D82 gains the measured result (wgpu against Vulkan on both phones).

## Gates
scripts/gate.sh --full
