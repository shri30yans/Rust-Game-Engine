# depth-invariance: the scene pass shades every pixel the prepass covered, by construction (2026-11-12)

**What went wrong.** At High the world drew black. The depth prepass wrote depth and the opaque scene pass tested it again with strict `Less`, so every fragment at the stored depth was dropped (`render-high-black` fixed this by switching to `LessEqual`).

**Why it is still possible.** `LessEqual` only works if both passes compute bit-identical depth, and nothing guarantees that:
- The position is computed by two separate functions, `prepass_vertex` and `cooked_vertex` in `engine/render/src/shaders/scene.wgsl`, over two separate bindings (`pre_instances`/`pre_visible_indices`/`pre_mesh_records` against `instances`/`visible_indices`/`mesh_records`), with the quantized decode written twice.
- Neither vertex output is marked `@invariant`. Shader compilers may contract or reorder the arithmetic differently in two entry points (Metal and mobile Vulkan drivers do), so equal-looking code can give different depth.
- The opaque pass still writes depth after the prepass, and its compare state is chosen by `bool`s in two copies of the same expression in `engine/render/src/gfx/pipelines.rs` (`if glow || vm || !ctx.reuses_prepass_depth { Less } else { LessEqual }`).

## How Unreal does it
- With a full depth prepass (`r.EarlyZPass` covering all opaque), the base pass tests depth with "near or equal" and does not write depth.
- The prepass and the base pass get their position from the same vertex-factory function, and the position output is declared invariant (HLSL `precise`, which Unreal wraps as `MakePrecise`), so the two passes produce the same depth.
- WGSL has the same tool: `@invariant` on `@builtin(position)` guarantees identical results across entry points when the data and control flow match ([WGSL spec, `invariant`](https://www.w3.org/TR/WGSL/#invariant-attr)). Metal honours it only when the compile option for invariance is set, so check what wgpu/naga do on Metal and report it.

## Design
1. **One position path.**
   - One WGSL function computes clip position from (mesh record, instance slot, local position), and one function decodes quantized positions. Every opaque vertex entry point calls them, the prepass and the scene pass alike.
   - Both passes read the same instance, visible-index and mesh-record buffers through one bind group layout for that data. Delete the `pre_*` duplicate bindings.
   - Mark the position output `@invariant` in every entry point that takes part in the prepass contract.
2. **Depth state is a function of the plan, written once.**
   - Add `enum OpaqueDepth { Own, FromPrepass }` in the frame plan, derived from `Prepass`.
   - `Own` means compare `Less` and write depth. `FromPrepass` means compare `LessEqual` and do not write depth, as Unreal's base pass does.
   - Each draw group (world opaque, viewmodel, glow, sky, particles) takes its depth state from one function of (`OpaqueDepth`, draw group). Delete the duplicated `bool` expression.
   - The viewmodel is not in the prepass and stays `Own`. Say so in the type, not in a comment.
3. **A test that guards the class.**
   - Headless, at every tier, with the prepass on and off: every pixel whose depth is not the far plane was shaded by the opaque pass.
   - Do it with a debug view that writes a constant from the opaque fragment shader, and count the covered pixels that do not carry it. The count must be 0.
   - Run it on the IMAGINE combat scene and on a fixture with coplanar and quantized meshes.
   - Report it on this Mac (Metal). If the RMX5085 is attached (`adb devices`), run the same check on it.

Done means:
- items 1 to 3 complete;
- goldens unchanged, or re-recorded with the reason stated;
- the four gates pass;
- the APK builds.

Read first:
- `AGENTS.md`;
- `docs/RENDERING.md`;
- `docs/README.md` §7 D68;
- `engine/render/src/frame_plan.rs`;
- `engine/render/src/gfx/pipelines.rs`;
- `engine/render/src/gfx/frame/{prepass.rs, scene.rs}`;
- `engine/render/src/shaders/scene.wgsl`.

Do not touch `engine/app`, `engine/assets`, `engine/platform`, `engine/ui` or `games/` beyond test fixtures. `asset-source` and `light-cache-1` are running there.

Docs in the same change:
- `docs/RENDERING.md` (the prepass contract);
- a decision in `docs/README.md` §7 (D71: one invariant position path; base pass depth from the plan, as Unreal);
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
