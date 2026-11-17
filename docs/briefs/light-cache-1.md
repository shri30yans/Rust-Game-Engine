# light-cache-1: surfels, precomputed visibility and a CPU light solver (the core of the light engine)

Read `docs/PRINCIPLES.md` §1 and §2 first. This brief builds the light engine's data and solver. light-cache-2 draws its probes in the renderer after `render-high-black` merges (`engine/render` runs one brief at a time). This brief is complete on its own:
- a baker that turns a level into a light asset;
- a run-time solver that lights the asset;
- a CLI that bakes Kiln Yard and prints the result;
- all of it tested.

Done means:
1. `tools/bake` cooks `games/imagine/data/arenas/kiln_yard.level.ron` into a deterministic light asset per layout.
2. `engine-light` solves it on the CPU, with bounce light that converges over iterations.
3. Probes show correct colour bleeding in a Cornell-box test.
4. The solve cost on Kiln Yard is measured on this Mac and on the RMX5085 phone if attached, through a bench.
5. The four gates pass.

## Design

**Crates.**
- `engine/light` (new, layer 2; see the second answers below): the data model, the run-time solver and SH math. No GPU and no wgpu.
- `tools/bake` (new): the baker.
- Update `tools/check-layers/layers.json` and record it in the decision.

**1. Data model (`engine-light`).** All ids are newtypes, and the asset is validated once on load.
- `Surfel { position, normal, albedo (linear RGB), area }`: a small patch of static surface.
- `ProbeGrid { origin, spacing, dims, valid: bitset }`.
- `LightLinks`, a sparse, fixed-size row per receiver:
  - for each surfel: up to K `(SurfelId, weight)` pairs, plus `sky: [f32; 4]` (sky visibility as SH L1);
  - for each probe: up to K pairs plus the sky term.
  - K is a bake setting (default 64 for surfels, 128 for probes), like Enlighten's irradiance budget.
- `LightAsset { surfels, grid, links, layout: LayoutId }`, a versioned record serialised through `engine-assets`. Validation:
  - every link id is in range;
  - weights are finite and non-negative;
  - each row's weights sum to at most 1 (energy conservation);
  - the grid dims match the probe count.
  An asset that passes validation cannot make the solver index out of range.

**2. Baker (`tools/bake`).** Deterministic, using `engine/jobs` for parallelism with a fixed reduction order.
- **Geometry:** build triangles from the level's static shapes (boxes now; cooked meshes when the level references them), then a BVH. Use a small BVH of our own or a well-maintained crate; justify the choice.
- **Surfels:** sample each static surface on a grid of about `surfel_size` metres (default 0.75 m). Albedo comes from the shape's material colour (the same table `scene_present.rs` uses today, read through one function, not copied).
- **Probes:** a grid over the level bounds or its `probe_*` volumes (default spacing 1.5 m). A probe inside geometry, meaning more than 30% of its rays hit backfaces, is invalid.
- **Links:** from each surfel and each probe, cast N rays (default 256, cosine-weighted for surfels, uniform for probes) with a fixed-seed RNG.
  - A ray that hits a surfel adds weight to that surfel; the hit point maps to the nearest surfel on the hit face.
  - A ray that escapes adds to the sky SH.
  - Keep the top K weights and fold the dropped weight into the next-largest neighbours, so no energy is lost. Document the rule you choose.
- **Layouts:** one asset per layout. The level's layout states pose the movers before the bake.
- **Output:** `games/imagine/content/cooked/<level>.<layout>.light`. The CLI prints the counts, the bytes and the bake time.

**3. Solver (`engine-light`).**
- **Inputs:** a `LightEnv`:
  - sun direction, colour and intensity;
  - sky SH;
  - point and spot lights (position, colour, range);
  - emissive surfels.
- **Direct light at surfels:** visibility to the sun and to each light by ray cast against the same BVH, which ships in the asset as a compact BVH. The solver re-casts only when the light changes, a budgeted number of surfels per update (default 2,000 rays), so a moving sun converges over a few frames.
- **Bounce:** `L_i = E_i + albedo_i * Σ_j w_ij * L_j(prev) + sky_i · skySH`, one iteration per update. This gives multi-bounce over frames (Enlighten's progressive solve).
- **Probes:** `probe_p = Σ_j w_pj * L_j + sky_p · skySH`, projected to SH L1 (4 RGB coefficients).
- **Output:** a probe buffer (f16 SH plus validity) ready for upload. No allocation per update: pre-sized buffers; prove it with the counting allocator.
- **Determinism:** same inputs give the same probe bytes.

**4. Tests.**
- SH projection matches the closed form for a constant and a single-direction environment, within 1%.
- **Cornell box** (a fixture level: a white room, a red left wall, a green right wall, a light). After convergence the probe near the red wall has R/G above a threshold and the probe near the green wall the reverse. Assert the ratios.
- **Energy:** a closed white room with albedo 0.5 converges to the analytic sum of a geometric series, within 2%.
- A probe inside a box is invalid.
- The bake is deterministic (same bytes twice).
- The asset round-trips, and validation rejects a corrupt link id.
- Solver updates allocate nothing.

**5. Bench.** `cargo bench -p engine-light` (or a bench example) for Kiln Yard: bake time, surfel and probe counts, and the solve cost per iteration and per sun change. Report it on this Mac. If a phone is attached (`adb devices`), run the bench binary on it (`adb push` plus `adb shell`); otherwise say so.

**Do not touch:** `engine/render`, `engine/ui`, `tools/ui-cook`, `engine/netcode`, `engine/app/src/live_scene.rs`. Other briefs running at the same time:
- `render-high-black` (`engine/render`);
- `ui-css-layout` (UI);
- `world-pipeline-2` (`engine/scene` spawn, `engine/app` live scene).

If you need a small read-only accessor in `engine/scene`, add it and say so.

**Docs in the same change:**
- `docs/ARCHITECTURE.md` §1 (new crates);
- `docs/RENDERING.md` §13 (the light cache) updated with what was built and measured, and the first part of `lighting-shell`;
- `docs/RENDERING.md` §13 (point to RENDERING §13);
- `docs/briefs/README.md` status `review`.

## Answers to the stop on 2026-11-12 (Kiro asked; these are decisions, not suggestions)

1. **A level with no authored layouts.** Every level has a **base state**: the level as written, with no layout applied. Authored layouts are poses over that base.
   - Add `enum LayoutState { Base, Named(LayoutId) }` in `engine-scene`, next to the layouts, with `Level::layout_states()` returning `Base` followed by each authored layout.
   - The bake makes one light asset per state, named `<level>.base.light` and `<level>.<layout>.light`. `LightAsset` stores a `LayoutState`.
   - This is not an invented default: the base state is what the game already spawns when no layout is applied. Kiln Yard therefore gets `kiln_yard.base.light`.
2. **Material colours.** Move the material table (base colour, roughness, metallic and pattern per `MaterialKind`) from `engine/app/src/scene_present.rs` into `engine-scene`, as the one public function `MaterialKind::surface()`.
   - `scene_present.rs` and `engine-light` both call it, and the private copy is deleted. That makes one table, not two.
   - This small `engine-app` edit is authorised. `world-pipeline-2` also edits `scene_present.rs`: keep your change to that one function. Do not merge other branches; the reviewer integrates overlapping briefs at merge.

## Answers to the second stop on 2026-11-12 (these replace any earlier wording they contradict)

1. **`engine-light` is layer 2,** beside `engine-render` and `engine-physics`. It knows nothing about levels, layouts or materials.
   - Its input is a `LightScene`: static triangles with per-triangle albedo, plus probe volumes and a probe spacing.
   - Its output is a `LightAsset` (surfels, grid, links and BVH).
   - `tools/bake` (layer 5) reads the level through `engine-scene`, builds one `LightScene` per `LayoutState` (with albedo from `MaterialKind::surface()`, the one table now in `engine-scene`), and writes `<level>.<state>.light`.
   - The pairing of a level state with its light asset lives in `engine-app` (layer 4), which knows both. `LightAsset` therefore does not store a `LayoutState`. Add a row for `engine/light` at layer 2 in `layers.json`.
2. **Stale bakes are caught at load, not trusted.** The light asset's header `source_hash` is a hash of the `LightScene` it was baked from.
   - A `LightScene::content_hash()` in `engine-light` computes it.
   - When light-cache-2 pairs a level state with its asset, it rebuilds the hash from the level and rejects a mismatch with an error naming the level and state. In this brief, add that check as a function in `tools/bake` with a test; light-cache-2 calls it.
3. **One versioned record envelope in `engine-assets`.**
   - Turn the existing header (magic, format version, source hash, payload hash) into a generic `encode_record(kind: AssetKind, source_hash, &T)` and `decode_record::<T>(kind, bytes)`, where `AssetKind` is a four-byte tag owned by the payload crate (`*b"MESH"`, `*b"LGHT"`). Decoding checks the kind, version and payload hash.
   - The owner crate validates the payload after decode (`LightAsset::validate`).
   - Move `MeshPack` onto the same envelope and delete the `Asset` enum, so there is one encoding path. Cooked mesh packs are re-cooked: run the cook and commit the outputs.
   - This `engine-assets` edit is authorised. It is the canonical boundary every future asset type uses (as in Unreal, one package format with typed exports).

## Review of the first attempt (2026-11-12, rejected; build it again against this brief)

A first attempt (Kiro) was rejected and deleted. It must not be repeated:
- It had no tests. The seven tests in **4. Tests** are the definition of done, not extras.
- Direct light had no visibility: no ray was cast against the BVH, so walls never shadowed the sun. Direct light at a surfel is occluded by the scene, tested by a surfel behind a wall receiving no sun.
- `SpotLight` was stored and never read. Every field is read, or it is absent.
- Probes were projected along one fixed direction, which keeps no direction at all. Each probe's SH L1 is the projection of the radiance arriving from its link directions: store the unit direction per probe link at bake and project each link's radiance along it. Test it: a probe beside a lit red wall has its red SH gradient pointing at the wall.
- `#![allow(missing_docs)]`, bare indexing and `n`/`p` names. Use the workspace lints with no crate-level allows; use ids that cannot be out of range after `validate` (checked iterators or `get` with a typed error at the boundary).
- A 5.5 MB cooked light asset was committed. Commit it only if it is under 1 MB; otherwise make the bake part of `tools/cook`'s run, so it is a cooked output like the meshes and is not hand-committed.

Decisions are not numbered: the design lives in `docs/RENDERING.md` §13.

`asset-source` (Codex, running) also edits `engine-assets` (a content pack and `AssetPath`). Keep your `engine-assets` change to the record envelope (`format/`). Do not merge other branches; the reviewer integrates overlapping briefs at merge.

## Gates
scripts/gate.sh
