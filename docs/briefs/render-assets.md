# render-assets: cooked meshes and textured PBR on screen, LOD, freed meshes, the pistol in hand (audit C4, H12 follow-up, GX2)

Goal: what the cooker makes is what the player sees. Done means:
1. A cooked `MeshPack` draws through the retained `RenderScene` with its PBR material: base colour, normal, occlusion-roughness-metal (ORM) and emissive textures, transcoded per device and mipmapped.
2. Meshes and textures can be freed.
3. Each primitive picks its LOD by screen size, with hysteresis.
4. Uploads respect a per-frame byte budget.
5. IMAGINE's first-person weapon is the cooked tactical pistol from `games/imagine/content/weapons_batch1/`.
6. The procedural greybox look still works, bench-gate passes, and the four gates pass.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D17, D29, D31, D32, D54, D55, D56;
- `docs/RENDERING.md` §3, §5.2, §6.4–6.7 and §7 (tiers and budgets);
- `docs/CONTENT.md` §5;
- `engine/assets/src/` (`MeshPack`, `MeshLod`, `PositionEncoding`, `Material`, `TextureData::transcode`, `AssetStore`);
- `engine/render/src/{gfx/, scene/, mesh.rs, tier.rs, shaders/scene*.wgsl}`;
- `engine/app/src/scene_present.rs`;
- `games/imagine/game/src/present.rs` (the viewmodel);
- `tools/blender/make_pistol.py` and `games/imagine/content/weapons_batch1/`.

## Design

Follow Unreal's split (D55): an asset is loaded off-thread (`AssetStore`, already built), the render side creates GPU resources from it within an upload budget, and a material is a set of textures plus parameters shared by every primitive that uses it.

**1. Two vertex formats, one draw path.**
- Procedural meshes keep their position stream and their 12-byte attribute stream (normal, colour, pattern).
- Cooked meshes use the cooked streams as they are: positions as `Float32` (12 B), or `Quantized16` (8 B, dequantised with the mesh's bounds from the per-mesh record), plus the cooked 12-byte attributes (octahedral normal, tangent with sign, half-float UV).
- `MeshId` records which format it has. Pipelines are keyed by (pass, vertex format, material kind), created once per tier, never per frame.
- The shadow and depth passes still bind positions only; `Quantized16` decodes in the vertex shader from bounds in the per-mesh record. No other change to those passes.

**2. Materials.**
- `MaterialId` (index plus generation) in `engine-render`. `Gfx::add_material(MaterialDesc)` holds texture handles and factors (base colour, metallic, roughness, emissive strength, normal scale, alpha mode `Opaque | Mask(cutoff)`).
- A material is one bind group (textures plus a per-tier anisotropic sampler).
- `PrimitiveDesc` gains `material: Material`, where `Material` is `Procedural(Look) | Textured(MaterialId)`. One enum, no flags.
- Binning (D29) sorts by pipeline, then material, then mesh. The visible-index buffer from render-scene-retained stays the per-mesh draw input.
- **Lighting is one function.** `scene_lighting.wgsl` takes a `SurfaceInputs` struct (albedo, normal, roughness, metallic, AO, emissive). The procedural path fills it from patterns; the textured path fills it from textures, with tangent-space normal mapping (MikkTSpace tangents from the cooker). No second lighting model.

**3. Textures.**
- `Gfx::add_texture(&TextureData, ColourSpace) -> TextureId` transcodes on the background pool to the family the device supports: BC7 on desktop, ASTC 4×4 on phones that have it, ETC2 otherwise, RGBA8 as the last resort.
- The device profile picks the family once from the adapter features; test the selection table.
- Every mip uploads. sRGB formats are used for colour and emissive, linear for normal and ORM.
- A texture's memory is counted in `FrameStats` (bytes by family).

**4. Lifetimes and the upload budget.**
- `Gfx::remove_mesh`, `remove_material` and `remove_texture` free GPU memory.
- Removing something a live primitive still uses is a typed error, so a primitive can never point at freed memory.
- An `engine-app` `AssetUploader` connects `AssetStore` events to `Gfx`: Ready creates GPU resources, Unloaded removes them, and a hot reload swaps them at a frame boundary.
- Uploads per frame are capped by bytes (tier data, e.g. 8 MB desktop, 2 MB phone). Larger loads spread over frames, and the asset shows when complete. This replaces "meshes are never freed" (H12).

**5. LOD.**
- In the existing cull loop, each visible primitive computes its projected sphere size and picks the LOD.
- The thresholds come from the cooker's per-LOD screen error (meshopt simplification error, stored per `MeshLod`; add it to the format if absent, with a version bump).
- Hysteresis: switch down only when 10% past the threshold, and back up only when 10% inside it. The last LOD is stored per slot.
- Shadow cascades use one LOD coarser than the camera.
- Count primitives per LOD in `FrameStats`. No cross-fade yet; that is GX9.

**6. IMAGINE's weapon.**
- The tactical pistol (`pistol.glb`, the default finish) is cooked by `tools/cook` into IMAGINE's content and loaded through `AssetStore`.
- It is drawn as a `Layer::ViewModel` primitive with its textured materials, replacing the procedural viewmodel mesh.
- The muzzle socket from the cooked nodes (`socket_muzzle`) is where tracers and the muzzle flash start.
- The other finishes (bronze, crimson) are loaded but unused, ready for the loadout screen.
- If the pack has no textures, its materials still draw from their factors; state which in the report.

## Tests
- Headless golden images:
  - a sandbox test scene with cooked textured meshes (a fixture GLB with base colour, normal and ORM textures, cooked in the test or committed small): one desktop golden and one with the phone tier settings, within the stated TAA-noise bound;
  - the existing sandbox and IMAGINE goldens unchanged except IMAGINE's viewmodel, whose golden is re-recorded; explain the diff.
- Transcode: family selection for each feature set; every family produces the right number of mips and byte sizes.
- Lifetimes:
  - removing an in-use mesh or material is refused;
  - after removing everything, the texture and mesh memory counters return to their start values;
  - a hot-reloaded mesh swaps without a frame where the primitive is missing.
- The upload budget is never exceeded in a frame, and a 40 MB pack loads over several frames.
- LOD:
  - selection with hysteresis on a scripted camera dolly (no flip-flopping at the threshold);
  - shadow LOD one level coarser;
  - a proptest that the LOD is monotonic in distance.
- Counting allocator: steady frames of a textured scene allocate 0 bytes after warm-up.
- Performance: bench-gate passes on apple-m5. Add a `grid40-textured` case to the bench and the baseline, recorded with `--record` and a note.

**Do not touch:** `engine/netcode`, `engine/physics`, `engine/sessions`, `engine/ui`, `kits/`. A `net-play-1` merge may land on main while you work, and `imagine-menus` will follow. Run `git merge main` before you finish and keep `present.rs` edits to the viewmodel and the asset hook-up.

**Docs in the same change:**
- `docs/RENDERING.md` §3 (Materials, Textures and Geometry rows) and §9 (GX2 status);
- `docs/ARCHITECTURE.md` §1 (render and assets rows);
- `docs/research/2026-11-11-engine-audit.md` C4 and H12 status;
- `docs/CONTENT.md` §5 (LOD error);
- a new decision in `docs/README.md` §7 (one lighting function with `SurfaceInputs`, the `Material` enum, upload budget, removal refusal);
- `games/imagine/docs/` (the weapon now comes from the pack);
- `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
