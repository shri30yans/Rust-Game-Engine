# Content: from Blender to the game's pack

The single source of truth for how every model, map, texture, sound, font, UI screen and data file gets from its source into a game, for every game on the platform. Each game keeps its content in `games/<name>/content/` and its asset list in its own docs (IMAGINE: [ASSETS.md](../games/imagine/docs/ASSETS.md)). How cooked assets are drawn: [RENDERING.md](RENDERING.md). Principles: [PRINCIPLES.md](PRINCIPLES.md) §3.

## 1. Data, not code

**Decision.** If a designer would tweak it, it is data. Rust holds rules and systems only, in small crates so a change rebuilds little. There is no scripting language.

| Kind of change | Unreal | Unity | Ours |
|---|---|---|---|
| Engine | C++ modules | C++ runtime | `engine/*` crates |
| Game rules | C++ module + Live Coding | C# assemblies | the game crate; `subsecond` patches function bodies in desktop dev builds |
| Glue and quick logic | Blueprints | — | data: modes made of blocks, event timelines, interactables |
| Numbers | Data Assets, Data Tables | ScriptableObjects | validated RON tuning files, hot-reloaded |
| Maps | levels, World Partition | scenes, Addressables | cooked levels from Blender, hot-swapped |
| UI | UMG | UI Toolkit | UI documents cooked from HTML and CSS, hot-reloaded |
| Shaders and effects | material graphs, Niagara | Shader Graph, VFX Graph | Slang shaders and effect files, cooked into the pack |
| Build caching | Derived Data Cache | Library folder | the cooker's cache by content hash |

Rules:
1. Numbers, layouts, events, effects, sounds, UI and text never live in Rust.
2. Data is validated once, on cook or load. A bad value is an error that names the file, the record and the field, and never reaches the simulation.
3. Hot reload swaps only valid data at a tick or frame boundary and keeps the last good version.
4. Three build profiles: `dev` (own crates unoptimised, dependencies at `opt-level = 3`: about 1 s rebuilds, playable speed), `play` (release without LTO, incremental, for playtests), `release` (thin LTO, for shipping and benches).

**Rejected:** a scripting language now (Lua, WASM, visual scripting: a second language, debugger and binding layer; it may return for player mods); the game as a dynamic library reloaded at run time (unsafe across Rust versions; `subsecond` does it safely).

## 2. Runtime content packs

**Decision (built).** Every game names one pack through `GameEntry::CONTENT`. The cooker writes it to `content/cooked/<game>.pak`; each entry has a validated relative `AssetPath`, byte range and content hash. `engine-assets::PackReader` validates the table and every entry hash before mounting. Loaders take logical paths and return a typed missing-entry error. Desktop, Android and headless start-up use the same reader; `engine-platform` only supplies the stream (desktop file, APK asset or development file).

- **What goes in:** `content/cooked/`, `content/audio/`, `content/fonts/`, `data/`, `ui/`, `game/levels/`, and the engine's shaders under `engine/shaders/`. Docs, Blender sources, exports and reports never enter a pack. The cooker reports packed bytes per top-level folder.
- **Freshness:** a deterministic test compares freshly gathered bytes with the checked-in pack and says to run `cargo run -p cook -- <game>` when stale. Android packaging stages the same `.pak` as `assets/<game>.pak`.
- **Reload:** in desktop development `cook --watch <game>` polls every input by content hash every 250 ms, updates changed cooked outputs, and rewrites the one pack; the platform reports its revision; `engine-app` reopens and validates it, and the asset store and game systems reload entries whose hashes changed. APK packs are immutable. There is no per-type file watcher and no debug-only source path.

**Why.** Build-machine paths cannot identify content on a phone, and packaging must be checked against the exact bytes the game loads.

## 3. Folders

Every game has the same layout under `games/<name>/`; tools take the root as an argument, never a fixed path.

| Folder | Holds | In git |
|---|---|---|
| `content/blender/` | `.blend` sources (maps, kits, characters, weapons) | yes (LFS) |
| `content/textures/` | source textures (PNG, EXR) | yes (LFS) |
| `content/audio/` | source sounds (WAV) | yes (LFS) |
| `content/fonts/` | OFL fonts and their licences | yes |
| `content/weapons/`, `content/effects/`, `content/modes/` | weapon, effect and mode data (`*.ron`) | yes |
| `content/prefabs/` | prefab definitions (`*.prefab.ron`) | yes |
| `content/test_scenes/` | engine test maps (sandbox only) | yes |
| `ui/` | UI pages (HTML, CSS), cooked documents, the theme | yes |
| `game/levels/` | handwritten level files (`*.ron`) | yes |
| `content/exported/` | glTF written by the Blender script | no (generated) |
| `content/cooked/` | the pack and intermediate cooker outputs | the top-level `<game>.pak` only |

Shared by all games: `tools/blender/` (Blender Python scripts), `tools/cook`, `tools/ui-cook`, `tools/texture-encode`.

**Binary sources are in Git LFS:** `.blend`, `.glb`, `.mp4`, source textures and source audio (`.gitattributes`), because task branches built in worktrees see only committed files. Blender backups (`*.blend1`) and vendored Python packages (`_pydeps/`) are ignored.

## 4. Blender is the editor

**Decision.** Maps and models are built in Blender, tagged with custom properties, exported by one script and cooked. We build no level editor; an in-engine editor waits until Blender is shown to be the bottleneck. Blender is at `/Applications/Blender.app` (not on PATH):

```bash
/Applications/Blender.app/Contents/MacOS/Blender --background --python tools/blender/export.py -- games/imagine/content/blender/kiln_yard.blend
```

### 4.1 Rules for every Blender file
- Metres, 1 unit = 1 m. Forward is −Y in Blender; the exporter converts (glTF +Y up).
- Origins sit where the object touches the ground or attaches (a weapon's grip, a door's hinge).
- Scale and rotation are applied before export (the script checks).
- One material per surface type, named from the material list (§7.3).

### 4.2 Naming

| Name | Meaning |
|---|---|
| `<object>_COL` | child collision mesh; its `collision` tag selects `box`, `hull`, `mesh` or `decompose` (default `hull`) |
| `_LOD0`, `_LOD1`, `_LOD2` | detail levels (the cooker generates missing ones) |
| `socket_*` (empty) | attachment point: `socket_muzzle`, `socket_eject`, `socket_sight`, `socket_grip`, `socket_hand_r`, `socket_back` |
| `spawn_*`, `zone_*`, `obj_*`, `probe_*`, `sound_*` | spawn points, mode zones, objectives, reflection probes, ambient sound zones |
| `<mover>@<layout>` (empty child) | a mover's world pose in one named layout |
| `CUE_<name>` (empty) | a cue that selects a layout |

**Layouts** are ordered by the scene property `layouts` (comma-separated lower_snake_case, the first is the default). Every `role=mover` object has exactly one `<mover>@<layout>` child per layout; its world transform is the pose. A switchable object has `switch=<name>` and one boolean `layout_<layout>` per layout (`on`/`off`). A cue empty `CUE_<name>` has `layout=<target>` and `duration_s > 0`; firing it moves every mover and changes every switch that differs between the current and target layouts over that duration. Missing or extra poses or properties are export and cook errors naming the object and field. Pose empties have no ID of their own; the mover's ID keys its poses.

### 4.3 Custom properties (tags)

Blender custom properties travel in glTF extras; the cooker reads them.

| Property | Values | Used by |
|---|---|---|
| `role` | `static`, `mover`, `breakable`, `trigger`, `interact`, `prop`, `glass` | the cook: static geometry is merged per grid cell and gets light-cache surfels; movers, breakables and props stay separate instances; a `breakable` building is split into structural elements and pre-cut chunks, lit by probes only |
| `material` | `concrete`, `brick`, `wood`, `steel`, `glass`, … | impact effects, footsteps, penetration, collision surface id |
| `collision` | `box`, `hull`, `mesh`, `decompose` | the shape for a matching `_COL` child |
| `vhacd_resolution` | `16`–`256` | per-object decomposition resolution |
| `vhacd_max_hulls` | `1`–`64` | per-object hull cap |
| `mover` | an action, or start and end transforms, plus `speed` | movers driven by event timelines |
| `cue` | an event name | objects an event acts on |
| `team` | `0`–`3` | spawns, zones |
| `mode` | comma-separated mode names | objects that exist only in some modes |
| `surface_class` | `opaque`, `masked`, `translucent` (optional) | an override of the cooked class (§5.2); forcing `opaque` on a texture with holes is a cook error unless acknowledged |

### 4.4 The export script (`tools/blender/export.py`)
1. Checks every rule above and stops with a plain list of problems (object name and what to fix): metric metres, applied scale and rotation, origin on a bounds face, `_COL` and `_LOD` names, known tag values, team range, cue and socket names.
2. Exports binary glTF to `content/exported/` with extras, sockets and animations.
3. Runs from the command line or a button in Blender.

**Stable IDs.** The exporter keeps a random non-zero 64-bit `asset_id` on the scene and each object (stored as hex, since Blender cannot hold every u64), assigned only when missing or invalid; a duplicated object gets a new id with a warning. Each node also carries the scene's `source_asset_id`. **Why:** Blender renames must not break placed references.

## 5. Cooking (`tools/cook`)

The cooker is deterministic: the same inputs give the same bytes, outputs are written incrementally by source hash with no timestamps, and the encoder settings and revision are recorded and checked before incremental skips. No game path or asset name appears in it.

### 5.1 Meshes
- **Container:** a versioned `.meshpack` (eight-byte `ENGASSET` tag, format version, 64-bit source hash, postcard payload).
- **Streams:** positions in their own stream (16-bit mesh-box quantisation when the worst error is at most 0.1 mm, otherwise `f32`); a 12-byte attribute record (octahedral normal, packed octahedral tangent with handedness, two half-float UVs); 16- or 32-bit indices. Missing tangents come from `bevy_mikktspace`.
- **Optimisation:** exact-match vertex welding, cache and fetch ordering (meshoptimizer), LODs at 50% and 25% of source triangles (stopping at meshopt's 2% error limit), screen-size thresholds 0.50 and 0.25, bounds and simplification error per LOD.
- **Planned:** meshoptimizer clusters with bounds and per-cluster quantisation, meshopt disk codecs decoded on the job pool ([RENDERING.md](RENDERING.md) §8).
- **Nodes and animation:** transforms parent-first; extras become sorted string tags; sockets stay generic nodes; animation sampled at 30 Hz, linear.
- **Report:** source hash, triangles and stream bytes by LOD, estimated texture upload bytes per GPU format family.

### 5.2 Materials and textures
- **Materials** hold metal/roughness values and references to content-hashed textures.
- **Textures** become UASTC KTX2 with mipmaps (base colour and emissive sRGB; ORM and normal linear), through `tools/texture-encode`, the one Basis encoder crate shared by `cook` and `ui-cook` (the `basisu_c_sys` binding builds the vendored encoder, so output never depends on what is on `PATH`; a test checks the same pixels encode to the same bytes). `engine-assets` transcodes per device ([RENDERING.md](RENDERING.md) §9).
- **Planned: texture role decides block size** (6×6 albedo and normals, 8×8+ masks) with Basis v2 ASTC LDR or XUASTC.
- **Planned: surface class at cook.** Each material section's `SurfaceClass` (opaque, masked with a cutoff, translucent) is computed from the effective alpha over the texels its UVs touch; the glTF alpha mode is only a hint. All-opaque sections drop their alpha channel; masked triangles are split into solid (opaque), empty (deleted) and mixed (masked); alpha mips preserve coverage; roughness mips absorb normal variance. Every promotion and demotion is logged. Full rules: [RENDERING.md](RENDERING.md) §7.

### 5.3 Collision
Selection is deterministic: an `<object>_COL` child uses its `collision` tag. Otherwise `role=static` cooks LOD0 triangles as a triangle mesh; `prop`, `mover` and `breakable` cook one convex hull when the mesh passes the convexity test, otherwise a compound from VHACD; `trigger` cooks a box sensor. The `material` tag becomes the surface name.

A mesh is convex only when it is closed and manifold, its signed volume is non-zero, and its convex hull is at most 2% larger. VHACD defaults: resolution 64, 16 hulls, concavity 0.0025 (limits 16–256, 1–64, `(0, 0.1]`); invalid overrides fail cooking with the object name.

### 5.4 Shaders
`tools/cook/src/shaders/` compiles every `.slang` under the shader root to SPIR-V with `slangc`, reads Slang's reflection into `ShaderReflection`, and writes `ShaderModuleData` records under `engine/shaders/<name>` in the pack. A missing `slangc` is a cook error naming the setup step. (Planned: [vk-1](briefs/vk-1-gpu.md).) The shader gate (Mali Offline Compiler: no spills, register and cycle limits) runs on the same set.

### 5.5 The index
After cooking, `index.ron` maps each node `asset_id` to its mesh pack, node index and source hash; a source file's id maps to its pack with no node. It is rebuilt deterministically; duplicate ids fail cooking and name both owners. Runtime collision sets and level records use ids from the index, never paths.

## 6. Levels, prefabs and live edits

**Levels are validated records (built).** A level is a RON record read by `engine-scene`: `version`, `name`, `gravity` (must equal the game's constant, since the character controller uses the constant too), and lists of `shapes`, `instances`, `colliders`, `prefabs`, `volumes`, `layouts`, `cues`, `spawns`, `waypoints` and `markers`. Blender scenes cook to a `<source>.level.ron`; handwritten greybox files use the same schema (new lists default to empty).
- A greybox shape has a unique source id, a `kind` (`Cuboid(half)`, `Sphere(radius)`, `Capsule(half_height, radius)`, `Ramp(half, tilt_deg)`), translation, rotation in degrees, a `body` (`Static`, `Dynamic(mass_kg)`, `Kinematic`) and a `look` (a material name or `Custom(colour_hex, roughness, metallic)`, with an optional sRGB hex `tint`).
- Every entity-producing record has a required unique source id; one record spawns one entity with its visual and collider. Instances refer to an object id with a transform and optional material override; cooked collider sets are listed by object id; volumes are tagged boxes (`Zone`, `Trigger`, `Probe`, `Sound`, `Objective`); layouts map mover ids to poses and switch ids to states; cues map names to a target layout and duration. No level record contains a cooked path.
- `Visual` is plain render-free data; mapping it to GPU meshes is the presenter's job.
- **Installs are all-or-nothing:** the new level spawns first; the previous one is removed only after every record spawned. A level that fails part-way is cleaned up and the running level is untouched.

**Prefabs.** A prefab is a reusable object file (door, crate, charge, barrel) with parts, physics and effects. The cooker validates and copies each definition into `cooked/prefabs/` with a stable id. A placement stores the prefab id, a transform and optional component field overrides. Games register the typed component blocks they understand (`PrefabRegistry`, `SceneContent`); `engine-scene` rejects unknown blocks and bad values with the file and field, and never knows a game's blocks.

**Live edits (desktop debug only).** `LiveScene` compares entity-producing records by stable id at the frame boundary: a transform-only change moves the entity and its body in place; a changed record is spawned through the normal path before its old entry is removed; additions and removals use the same path; the game then rebuilds derived state. Live edits never travel or replicate, and Android does not watch levels. **Gameplay owns an entity once it changes it:** a destroyed entity is never restored; an entity whose transform differs from its last accepted authored transform is skipped for moves, replacements and removals until travel; a pristine prefab replacement rebuilds from the file (resetting its state). Every skip is logged with the id and reason.

The generated sample kit (`tools/blender/make_sample_arena.py`) uses a 4 m module grid, one trim-sheet material and separate `_COL` children.

## 7. Reference

### 7.1 Size budgets (desktop; phone budgets come from the device profile)

| Asset | Triangles | Textures |
|---|---|---|
| First-person weapon | 10–20k | one 2K set |
| Third-person weapon | 3–5k + 2 LODs | shares the set at a lower mip |
| Character | 15–30k + 2 LODs | one 2K set |
| Kit piece | 0.5–5k | trim sheets + tiling materials |
| Whole map in view | ≤ 5M after culling | resident within the tier's texture budget |

### 7.2 Licences
Only CC0, OFL, or assets whose licence explicitly allows use in a sold game. Each third-party file's source and licence go in `content/LICENSES.md`.

### 7.3 Material names
`concrete`, `brick`, `plaster`, `wood`, `steel`, `metal_corrugated`, `glass`, `dirt`, `gravel`, `asphalt`, `plastic_sheet`, `rubber`. A new material is added here first, then in the destruction and sound tables.
