# arena-materials: the arena gets real surface textures (2026-11-12)

The owner asked where the arena's textures are. There are none.
- Kiln Yard (`games/imagine/data/arenas/kiln_yard.level.ron`) is a blockout of boxes.
- Each box has a `MaterialKind` (Floor, Wall, Concrete, Brick, and so on) that maps to a flat colour plus a small procedural shader pattern (`engine/app/src/scene_present.rs` `material_style`, `scene_patterns.wgsl`).

**Why it was possible.** Materials are a colour table in code, not assets. Nothing in the pipeline can carry a texture set to a level surface, and boxes have no UVs.

## How other engines handle blockouts
- **Unreal.** Blockout and modular geometry use world-aligned (triplanar) material functions, so a box gets proper concrete or metal at any size without UVs.
- **Unreal and Unity.** Materials are assets: a material instance points at texture maps plus parameters.

## Design
1. **Material assets.**
   - A cooked material is base colour, normal, and an ORM map (occlusion, roughness, metallic), as KTX2 through the shared `tools/texture-encode` crate.
   - Add a scale in metres and a tint.
   - `MaterialKind` in level data names a material asset. The level cook checks that it exists (a cook error otherwise). The colour table in `scene_present.rs` goes.
2. **Triplanar sampling** in the scene shader, for level geometry without UVs, blended by the normal.
   - Cooked meshes with UVs keep their UVs.
   - Mobile (Handheld): one texture fetch set per axis, at a measured cost. Give the number from the RMX5085.
3. **Textures.** CC0 sets from ambientCG or Poly Haven:
   - concrete, painted metal, brick, worn wood, floor tiles, and an emissive panel;
   - record each source URL and licence in `games/imagine/content/textures/SOURCES.md`.
4. **Show it.** IMAGINE combat frames at Low and High in the report, and an on-device frame from each test phone.

Done means:
- items 1 to 4 complete end to end;
- the four gates pass;
- the APK builds.

Starts after `render-high-black`, because `engine/render` runs one brief at a time.

Read first:
- `AGENTS.md`;
- `engine/app/src/scene_present.rs`;
- `engine/render/src/shaders/scene*.wgsl`;
- `tools/cook`;
- `tools/texture-encode`;
- `engine/scene/src/model.rs` (`MaterialKind`);
- `games/imagine/data/arenas/`.

Docs in the same change:
- `docs/ARCHITECTURE.md` §1;
- a decision in `docs/README.md` §7 (material assets; triplanar for level geometry);
- `games/imagine/docs/README.md`;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
