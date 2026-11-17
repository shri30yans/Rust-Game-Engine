# render-scene-retained: a retained render scene with compact instance and vertex records (audit H4, H5, H2, H3; D56)

Goal: the renderer keeps what it draws between frames, so cost follows what changed, and every record it moves is about half the size. Done means: (1) a static scene uploads **0 instance bytes per frame** after the first frame (measured by a counter the renderer exposes); (2) the instance record is **64 bytes** (from 160) and the vertex is split into a **12-byte position stream** and a **12-byte attribute stream** (from 52), with shadow and depth passes binding positions only; (3) the immediate `Gfx::draw*` calls are gone: one path; (4) headless renders of the sandbox and IMAGINE are visually identical to before (image diff within the TAA noise bound, stated); (5) the render bench on the same machine and the same load, before and after, is in the report; the four gates pass.

Read first: `AGENTS.md`; `docs/README.md` §7 D32, D54, D55, D56; `docs/research/2026-11-11-engine-audit.md` H2, H3, H4, H5, J2 and §F; `docs/RENDERING.md` (passes, tiers); `engine/render/src/{mesh.rs,gfx/*}`, `engine/render/src/shaders/*`; `engine/app/src/{extract,scene_present,client,live_scene}.rs`; `games/sandbox/game/src/present.rs`, `games/imagine/game/src/present.rs` (every `draw*` caller).

## Design

Follow Unreal's scene model (D55): the game side registers a **primitive** once and later only marks its transform or material dirty; the render side owns a **scene** of persistent proxies in dense arrays, uploads only dirty entries, and culls over the arrays every frame (`FScene`, `FPrimitiveSceneProxy`, `MarkRenderTransformDirty`). Bevy's retained render world and Unity's SRP Batcher do the same.

**1. `RenderScene` in `engine-render`** (`src/scene/`), owned by `Gfx`:
- `add(&mut self, mesh: MeshId, desc: PrimitiveDesc) -> Result<PrimitiveId, RenderError>`; `set_transform(id, Affine3A)`; `set_look(id, Look)`; `set_visible(id, bool)`; `remove(id)`. `PrimitiveId` is an index plus a generation: a stale id is a typed error, never a wrong object.
- `PrimitiveDesc { transform: Affine3A, look: Look, layer: Layer, casts_shadow: bool }`, `Layer::{World, ViewModel, Glow}` (replaces `draw`, `draw_ex`, `draw_vm`, `draw_glow`). `Look` holds tint, emissive, roughness/metalness overrides, pattern and flags as typed values.
- Storage: dense CPU arrays (structure of arrays) mirrored in GPU storage buffers sized by capacity (grows by doubling, never per frame). A dirty list of slots per frame; upload writes only dirty slots (coalesce adjacent ones into ranges).
- **Instance record, 64 bytes:** affine 3×4 transform (48 bytes, three `vec4` rows) + tint RGBA8 and emissive as f16 (8 bytes) + material overrides and flags packed (8 bytes). Previous transforms live in a **separate** 48-byte-per-slot buffer bound only when TAA is on: when a slot moves, its previous value becomes the old current; a slot that moved last frame and not this frame gets previous := current once (track a "moved last frame" list) so static objects have zero velocity without per-frame uploads.
- **Culling:** every frame, test each visible slot's bounding sphere against the camera frustum and each shadow cascade (the per-cascade culling already on main moves here), write visible slot indices per mesh into a reused `u32` index buffer, and draw instanced with the shader reading `instances[indices[instance_index]]`. One cull loop over dense arrays; no per-object allocation.
- Counters in `FrameStats`: instance bytes uploaded, dirty slots, visible per pass.

**2. Vertex streams (H3).** `Vertex` becomes two streams: **position** `[f32; 3]` (12 bytes; procedural meshes keep f32). `engine-assets` is merged: its cooked `MeshLod` already stores positions as `PositionEncoding::{Float32, Quantized16}` (12 or 8 bytes, dequantised with per-mesh bounds) and its own 12-byte attribute stream (octahedral normal, tangent, half UV) for textured PBR. Design the position-stream binding and the per-mesh record so a Quantized16 mesh can be added later by giving the mesh its bounds and a vertex format, with no change to the shadow and depth passes; do not upload cooked meshes in this brief (that is C4) and **attributes** 12 bytes: octahedral normal `snorm16x2`, base colour `unorm8x4`, material (roughness, metalness, pattern, AO) `unorm8x4`. Shadow and depth-prepass pipelines bind only the position stream. Indices are `u16` when a mesh has < 65,536 vertices. Shader decode is one small function per stream; the procedural mesh builders keep their API and encode at build time.

**3. Engine-app extract (H5).** `ScenePresenter` keeps a `NetId → PrimitiveId` map. Per frame: iterate the frame's visuals densely; spawn → `add`; gone → `remove`; for existing ones, write `set_transform` only if the blended transform differs from the last value sent for that entity (bit compare). Static walls therefore cost a compare and no upload. A game's own draws (viewmodel, tracers, glow zones) become primitives it adds once and updates (`Layer::ViewModel` transforms stay in camera space as today).

**4. Migration, one path.** Convert every caller in `engine/app`, `games/sandbox` and `games/imagine` and delete `draw`, `draw_ex`, `draw_vm`, `draw_glow` and the per-frame buckets. No compatibility layer.

**Tests:**
- `RenderScene` unit tests: add/update/remove, stale id error, slot reuse bumps generation, capacity growth keeps contents, dirty ranges coalesce.
- Property test: random sequences of add/set/remove produce the same visible set (per mesh, per pass) as a brute-force recompute over the live primitives.
- Counters: a 1,000-primitive static scene uploads 0 instance bytes from frame 2 on; moving one primitive uploads one record (+ its previous transform when TAA is on).
- Instance and vertex encoding round trips: transform within float precision, normal within 0.5°, colours and material exact to 8 bits.
- Headless render golden comparison before/after for the sandbox scene and IMAGINE's arena (existing `headless_render` test infrastructure).
- Counting allocator: a steady frame allocates nothing in `RenderScene` and extract.
- Bench: the existing render bench (40×40 and 80×80 blocks, TAA on/off) before and after on the same machine under the same load, run back to back, numbers in the report and in `docs/research/2026-11-11-engine-audit.md`.

**Do not touch:** `engine/netcode`, `engine/physics`, `engine/assets`, `tools/cook`, `kits/`. Start this brief only after `movement-states` and `engine-audio-1` are merged (both edit `games/imagine/game/src/present.rs` and `engine/app/src/client.rs`); rebase on main first.

**Docs in the same change:** `docs/ARCHITECTURE.md` §1 render and app rows; `docs/RENDERING.md` (the retained scene, the record layouts); `docs/research/2026-11-11-engine-audit.md` H2, H3, H4, H5, J2 fixed with numbers; `docs/briefs/README.md` status `review`.

## Decisions after Codex's first stop (2026-11-12): extract iteration (H5)
1. **One dense query in `engine-world`.** `spawn` also stores the entity's `NetId` as a hidden component. It is not registered, so it stays out of replication and the state hash. Add `World::query` (name your call) that iterates hecs storage densely over a component tuple and yields `(NetId, items)`. No hecs `Entity` appears in the public API. The query does not allocate and runs in hecs archetype order. Test: it yields exactly the live entities that carry the components, with their correct `NetId`s, and nothing after a despawn.
2. **No lookups by search.** The extractor and `RenderFrame` stop relying on ascending-id order. Previous transforms, `transform_entry` and similar per-entity lookups use **slot tables indexed by `NetId` index**, with the generation checked so a reused index never reads a dead entity's data. They are pre-sized from the extract capacity and grow only when the id space does, never per frame. Delete `previous_for`, the sorted merge, and the `binary_search` in `transform_entry`.
3. **Change detection stays where the brief puts it.** `ScenePresenter` bit-compares the blended transform against the last one it sent. No ECS change ticks are added; the dense query makes the compare cheap. Measure extract time at 40×40 and 80×80 before and after, and report it.
4. The bench baseline in your report is fine. Use `tools/bench-gate` (`cargo run -p bench-gate -- --device apple-m5`) for the before/after comparison, and say if the machine was otherwise idle.
5. `RTK.md` is a personal file outside the repo; ignore it.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
