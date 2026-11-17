# world-pipeline-2: apply level edits live in the running game, sample arena golden, reproducible from Blender (finishes world-pipeline)

`world-pipeline` part 1 is merged:
- stable source ids in the cooked index;
- the shared `Level` fields with cooked `AssetId` references;
- layouts, movers, switches and cues authored in Blender;
- the prefab registry with typed blocks and validated overrides;
- content-hash watching, and stable-id level diffs (computed and tested);
- the sample arena drawn through the `AssetUploader`.

What remains: the running game still reinstalls the whole level when a level changes, so the diff is computed and then thrown away.

Done means:
1. Saving a level in Blender updates the running game in place. Moved, added and removed objects and changed prefab blocks are applied from the diff, with no travel and no reinstall.
2. The sample arena has a golden.
3. The sample is regenerated from its script by real Blender, byte-identically.
4. The four gates pass.

Read first:
- `AGENTS.md`;
- `docs/briefs/world-pipeline.md` (the original design; its live-edit section is what remains);
- `docs/README.md` §7 (the world-pipeline decision);
- `engine/app/src/{live_scene,scene_present}.rs`;
- `engine/scene/src/{spawn,model,prefab}.rs`;
- `games/{sandbox,imagine}/game/src/scene.rs`;
- `tools/blender/`.

## Design

**1. Apply the diff, in one place.** Only two live operations exist: a move in place, and a replace (remove, then spawn). Nothing else is diffed.
- **Stable-id map.**
  - `Spawned` owns `BTreeMap<AssetId, SpawnedEntry>`, keyed by the authored source id. Each entry holds `{ id: NetId, body: Option<BodyId>, authored: Transform }`.
  - Every spawn path fills it: instances, prefabs, colliders and bodies.
  - The existing positional lists stay for game roles and are rebuilt from the same spawn. There is still one owner: `Spawned`.
  - The diff applier is the only code that edits the map after load.
- **`LevelDiff` classification**, made by the cooker diff:
  - `moved`: only the transform changed, for an instance, a prefab or a collider placement;
  - `replaced`: anything else changed (mesh asset, material override, collision kind or shape, prefab id, prefab blocks);
  - `added`;
  - `removed`.
  Collider and shape changes are always `replaced`. There is no shape diff.
- **Apply**, in `engine-app`, at a frame boundary, through one function both games use:
  - **move** sets the entity transform, the body pose and the primitive transform in place;
  - **replace** removes the entity, its body and its primitive, then spawns from the new definition through the normal spawn path. The new entity gets a new `NetId`; live edit is desktop dev only, with no replication;
  - **add** spawns;
  - **remove** despawns.
- **Gameplay-changed rule**, with no mutation tracking. The applier checks the live entity against its `SpawnedEntry`:
  - the entity no longer exists, because gameplay destroyed it: skip it, never restore it, and drop it from the map;
  - its current transform differs from `authored`, because physics or gameplay moved it: skip moves and replaces for it until the next travel;
  - otherwise apply.
  A replace rebuilds the instance's block state from the file, which resets a damaged crate's health. That is the documented rule.
  Log every skip with the source id and the reason.
- Delete the full-level reinstall path for live edits once the diff path works. Travel still installs fully.
- Desktop dev builds only; it stays off on Android (same rule as `imagine-device-fixes`).

**2. Tests.**
- Move, replace, add and remove, each applied to a running world. For pristine entities, the entity count, transforms and blocks match a fresh load of the edited level.
- An entity moved by physics is skipped, with a log line.
- A destroyed crate is not restored by an unrelated edit.
- No frame shows a missing primitive during an edit. Use the uploader's frame-boundary contract.
- A counting-allocator test: an idle watcher allocates nothing per frame.

**3. Sample golden.**
- A headless golden of the sample arena in the sandbox, at the desktop and phone tiers.
- A second golden after switching to the `lockdown` layout, so mover poses show.

**4. Reproducible from Blender.**
- Blender is installed at `/Applications/Blender.app/Contents/MacOS/Blender`. Find it through a `BLENDER` environment variable with that path as the default on macOS. The scripts already use this pattern if `make_pistol.py` has it; reuse it.
- Run `make_sample_arena.py`, then export, then cook, twice. Show that the outputs match the committed files byte for byte. If they do not, fix the cause (sorting, float formatting, timestamps) and commit the regenerated files.
- Add a test that runs only when Blender is found. It must say clearly that it was skipped otherwise, never pass silently.

**5. Naming.** Rename the `turret_placeholder` prefab to `turret_base`. It is a real prefab with health and a steel surface, and the name should say what it is. The original brief's wording was mine.

**Do not touch:** `engine/netcode`, `engine/physics` internals, `engine/render` internals, `engine/ui`, `kits/`. Other briefs running at the same time:
- `render-high-black` (`engine/render`);
- `ui-css-layout` (`engine/ui`, `tools/ui-cook`, IMAGINE screens);
- `light-cache-1` (new `engine/light`, `tools/bake`).
Run `git merge main` before you finish; you are authorised to merge main into this branch.

**Docs in the same change:**
- `docs/CONTENT.md` (live edit behaviour and the gameplay-state rule);
- `docs/ARCHITECTURE.md` §1 (app and scene rows);
- the world-pipeline decision in `docs/README.md` §7, extended;
- `games/imagine/docs/PLAN.md` §6;
- `docs/briefs/README.md` status `review`.

## Answers to the stop on 2026-11-12 (Kiro asked; these are decisions; follow Unreal's actor model, D55)

1. **One authored object is one entity.** Spawning creates exactly one entity per authored record, carrying both its visual and its collider (in Unreal, one actor with mesh and collision components).
   - Change the cooked and prefab spawn paths so they no longer make a separate collider entity.
   - `SpawnedEntry { id: NetId, body: Option<BodyId>, authored: Transform }` then holds.
2. **Source ids are unique per level, checked at load.** A level where two records share an `AssetId` (an instance and a cooked collider, or anything else) is a `LevelError` naming both. With rule 1, a cooked object's mesh and collider are one record.
3. **Scene content comes from the game once, at registration.** Add to the `Game` trait in `engine-app` one method returning the game's scene content: its `PrefabRegistry` and its collider and surface mapping, as one engine-scene type, for example `SceneContent`.
   - The engine-app applier uses only that, so the engine never names a game (D24). It is the same object initial spawning uses, so there is one path.
4. **The game reads the cooked level, and only that.** The level the game loads and watches is the cook's output. For the sample arena that is `games/imagine/content/cooked/sample_arena.level.ron`.
   - Remove the `data/arenas/sample_arena.level.ron` copy and its `include_str!`.
   - Tests load the cooked file.
   - `cook --watch` writes, the game watches the same file, and the live diff applies.
5. **Every record has a stable id.** Hand-authored `ShapeDef` records get a required `id: AssetId`. The level loader rejects a missing or duplicate id.
   - Migrate `kiln_yard.level.ron` and every other hand-written level with a one-off script, using each shape's `name` as the seed for its id, and commit the migrated files. Do not commit the script.
   - Shapes are then diffed like everything else; delete the "shapes are not diffed" rule.

## Review of the Kiro run (2026-11-12): finish it on top of `kiro/world-pipeline-2`

The run on `kiro/world-pipeline-2` (14 commits) built live diffs, stable ids, the Blender reproducibility test and the sample arena. Keep that work. It is not mergeable until the items below are fixed. This finishing run starts after `asset-source` and `golden-images` merge. The reviewer merges main into `kiro/world-pipeline-2` first, and the run builds on that branch.

1. **The three sample-arena goldens are blank.** `sample-arena-{desktop,phone,lockdown}.png` are the same pure-white image, so the test compares white to white. Find the cause (camera, exposure, lights, or the scene not loading), fix it, and re-record through `render::golden`, which refuses a uniform image. Report the cause and show the three images.
2. **Files over the 500-line hard limit:** `engine/scene/src/spawn.rs` (1,702), `engine/scene/src/model.rs` (1,493), `engine/app/src/live_scene.rs` (1,321).
   - Split each into a folder by responsibility (tenet 5), for example spawn: plan, instances, prefabs, visuals; model: level, shapes, layouts, validation; live scene: diff, apply, transaction.
   - `mod.rs` only declares, re-exports and documents.
   - Unit tests over about 150 lines go to a sibling `tests.rs`.
3. **One hot-reload path.** Delete the live scene's own level watcher and the `cfg!(all(debug_assertions, not(target_os = "android")))` gate in `ClientApp::poll_live_scene`. Live edits are driven by the pack reader's change event for the level's `AssetPath` (asset-source), the same event every other system reloads on, on every platform and build type.
4. **No presenter asset hook.** Delete `Presenter::load_frame_assets`. Cooked assets referenced by the level are loaded by the engine from the game's pack by `AssetPath`; a presenter never acquires engine assets. Check whether `Presenter::before_frame` is still needed afterwards. If nothing reads it, delete it.
5. **No build-machine paths.** Delete the new `CARGO_MANIFEST_DIR` content paths in `games/*/game/src/scene.rs`. Content comes from `start.content`.

## Gates
scripts/gate.sh
