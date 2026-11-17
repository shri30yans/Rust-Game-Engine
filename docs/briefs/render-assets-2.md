# render-assets-2: asset uploader with a byte budget, one slot type, the cooked pistol in hand, textured goldens and bench (audit C4, H12; finishes render-assets)

**Status:** review

`render-assets` part 1 is merged:
- cooked vertex formats (`Float32` and `Quantized16` with per-mesh bounds records);
- `Material = Procedural(Look) | Textured(MaterialId)` with one bind group per material;
- one lighting function taking `SurfaceInputs`;
- the bind-group layout (group 0 view, 1 material, 2 reserved for probes, 3 free) within `downlevel_defaults`;
- LOD per slot with 10% hysteresis and shadows one LOD coarser;
- removal refused while a resource is in use;
- `AssetStore` frame-boundary `Ready` and `Reloaded` events and `path_by_id`.

This brief finishes the rest. Done means:
1. Assets loaded through `AssetStore` appear on screen through one uploader, within a per-frame byte budget, without hitching.
2. Meshes, textures and materials use one generic slot type.
3. IMAGINE's first-person weapon is the cooked tactical pistol.
4. Textured goldens and a textured bench case exist.
5. The four gates and bench-gate pass.

Read first:
- `AGENTS.md`;
- `docs/briefs/render-assets.md` (the original design; its steps 4–6 are what remains);
- `docs/README.md` §7 D54, D55;
- `engine/render/src/gfx/mod.rs` (`add_cooked_mesh`, `add_texture`, `add_transcoded_texture`, `add_material`, the `remove_*` methods and the per-type generation vectors);
- `engine/assets/src/store.rs`;
- `engine/app/src/`;
- `games/imagine/game/src/present.rs` (the viewmodel);
- `games/imagine/content/weapons_batch1/`.

## Design

## Contract

Following Unreal's component-to-asset ownership model, a component references an asset while the
engine owns that asset's render data. A reload recreates the render data and re-points the existing
primitive; the component, primitive identity and asset handle stay unchanged.

`ClientApp` owns one `AssetStore`, one `AssetUploader` and the `JobPool` handle. At each frame
boundary, before the presenter's per-frame asset hook, it calls `store.apply_loaded()` followed by
`uploader.update(&mut store, &mut gfx, &pool, budget)`. `ClientAssets { store: &mut AssetStore,
uploader: &AssetUploader }` is the only presenter access to assets and is passed to `Presenter::setup`
and to the existing per-frame presenter hook that receives the renderer.

For each `MeshPack`, the uploader produces an index-aligned `GpuPack` of mesh and material handles.
CPU pack data (including nodes, sockets and collision) remains in `AssetStore`. Textures named by a
pack are loaded by content hash through the store; background `prepare_texture` jobs return through
a bounded channel. Materials are created after their textures upload, and the pack becomes Ready
only when all its meshes and materials exist. Missing or failed textures fail the pack with their
name. Empty material texture slots may use renderer defaults.

Mesh and texture uploads share the tier's byte budget and FIFO queue. A reload builds a complete
replacement under that budget, then swaps all affected mesh and material references at one frame
boundary before removing the old resources. A count mismatch rejects the reload and retains the last
good pack. Unloaded packs queue resource removal until no primitive uses them.

**1. One generic slot type.**
- `Slots<T>` with `SlotId { index: u32, generation: u32 }`, a free list for reuse, and a generation bump on remove. Put it in `engine-core` if a second engine crate can use it; otherwise in `engine-render`. Say which and why.
- `MeshId`, `TextureId` and `MaterialId` become typed wrappers over `SlotId`, so they cannot be mixed up.
- Replace the three hand-written index plus generation vector pairs in `Gfx` with three `Slots<_>`.
- The in-use checks stay. Make the texture-in-use check O(1): keep a per-texture use count in the slot, not a scan over all materials.
- **Tests:** a stale id is refused after reuse; a proptest over random add and remove sequences keeps ids unique among live entries; after removing everything, the memory counters return to their start values.

**2. One way to add a texture.**
- Transcoding becomes a pure function that can run on any thread: `prepare_texture(&TextureData, ColourSpace, TextureFamily) -> PreparedTexture`.
- `Gfx::add_texture(PreparedTexture) -> TextureId` only uploads.
- Delete the old transcode-and-upload `add_texture` and rename `add_transcoded_texture` to this single path.
- The device's family comes from `Gfx::texture_family()`.

**3. `AssetUploader` in `engine-app`.**
- It owns the connection from `AssetStore` events to `Gfx`.
- On `Ready`, it schedules `prepare_texture` jobs on the `JobPool` for the asset's textures. Results come back through a bounded channel.
- Each frame, at the frame boundary, it uploads prepared textures and meshes in order until the tier's byte budget is used: 8 MB desktop, 2 MB phone, from tier data. Whatever doesn't fit waits for the next frame.
- An asset becomes visible, meaning its materials are created and primitives may use it, only when all its parts are uploaded.
- `Reloaded`: build the new resources, swap the primitive's mesh or material at a frame boundary, then remove the old ones. No frame shows the primitive missing.
- `Unloaded`: remove the resources once no primitive uses them.
- The uploader allocates nothing per frame when idle.
- **Tests:**
  - a 40 MB pack spreads over several frames, and no frame uploads more than the budget, checked through `FrameStats` upload bytes;
  - a hot reload with no missing frame;
  - unload frees the memory;
  - the idle counting-allocator test.

**4. IMAGINE's weapon is the cooked pistol.**
- `pistol.glb` (default finish) is cooked by `tools/cook` into IMAGINE's content and loaded through `AssetStore` and the uploader.
- It is drawn as a `Layer::ViewModel` primitive with its textured materials, replacing the procedural viewmodel mesh.
- Tracers and the muzzle flash start at the cooked `socket_muzzle` node.
- `pistol_bronze` and `pistol_crimson` are loaded and kept unused, ready for the loadout.
- While the pistol is still uploading, the viewmodel is absent. It is never a placeholder that pops.
- Keep the `present.rs` edits to the viewmodel and the asset hook-up.

**5. Goldens and bench.**
- Headless goldens of a textured fixture (base colour, normal and ORM, committed small or generated in the test), on the desktop tier and the phone tier settings, sampling real PBR output.
- IMAGINE's viewmodel golden is re-recorded, with the diff described.
- A steady-frame counting-allocator test of the textured scene at 0 bytes.
- A `grid40-textured` bench case added and recorded with `--record` and a note.
- Run bench-gate three times and report all three. Part 1 saw one run with `grid40-taa` p99 +10.8%. If p99 regresses on two of three runs, find the cause before recording.

**Do not touch:** `engine/netcode`, `engine/physics`, `engine/sessions`, `engine/ui`, `kits/`. `imagine-combat-feel` may land while you work: run `git merge main` before you finish.

**Docs in the same change:**
- `docs/RENDERING.md` §3 and §9 (GX2 done);
- `docs/ARCHITECTURE.md` §1 (render, assets and app rows);
- `docs/research/2026-11-11-engine-audit.md` C4 and H12 status;
- `docs/CONTENT.md` §5;
- a new decision in `docs/README.md` §7 (the slot type, `prepare_texture` plus a single add path, the uploader budget, and the bind-group layout from part 1);
- `games/imagine/docs/` (the weapon now comes from the pack);
- `docs/briefs/README.md`: `render-assets` and `render-assets-2` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
