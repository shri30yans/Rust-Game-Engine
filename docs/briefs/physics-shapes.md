# physics-shapes: collision against real geometry, collision layers and surfaces (H-gap 1, D15, D55)

Goal: things collide with the shape they have, not a box around them. Done means:
1. Static level geometry collides as a triangle mesh.
2. Props and movers collide as convex hulls, or as a compound of hulls from convex decomposition done at cook time.
3. Every collider has a collision layer and a surface, and every query filters by layer and reports the surface it hit.
4. The cooker builds colliders from tagged Blender collision meshes, and from the render mesh where there is no tag.
5. The character walks smoothly over triangle-mesh floors, ramps and stairs, IMAGINE and the sandbox play as before, and the four gates pass.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D27, D34, D40, D55;
- `docs/CONTENT.md` §4.2–2.5 (the `_COL` naming and the `role` and `material` tags);
- `engine/physics/src/{collider,world,query,character,body}.rs`;
- `engine/assets/src/format/`, `tools/cook/src/`;
- `kits/fps/src/{controller,weapon}.rs`, `games/imagine/game/src/{scene,sim}.rs`, `engine/scene/src/model.rs`.

Unreal's model, which we follow: simple collision (`UCX_` convex hulls, boxes, capsules) for physics and characters, complex collision (the triangle mesh) for static geometry and precise traces; object channels with per-channel responses; a physical material on every hit result.

## Design

**1. Shapes in `engine-physics`, correct by construction.**
- **`ConvexShape`** (ball, cuboid, capsule, convex hull) is what moves and what is cast: character capsules, shape casts, overlap tests, dynamic bodies.
- **`ColliderShape`** is `Convex(ConvexShape) | TriMesh(TriMeshShape) | Compound(CompoundShape)`.
  - `TriMeshShape::new(vertices, indices)` validates finite vertices, in-range indices and no degenerate triangles. It builds with Rapier's `FIX_INTERNAL_EDGES` and `MERGE_DUPLICATE_VERTICES` flags, so characters do not snag on edges between triangles.
  - `CompoundShape` is a list of (pose, `ConvexShape`) pairs, 1 to 64 parts.
- Shape data is shared (`Arc`): cloning a shape is cheap and the same mesh can back many colliders.
- Inserting a `TriMesh` collider on a dynamic body is a typed error (Rapier advises against it). Static and kinematic bodies, and free colliders, may use one.
- `ConvexShape::hull(points)` fails with a typed error when the points are degenerate.
- The existing `ColliderShape::{ball, cuboid, capsule}` callers move to `ConvexShape`. Shape casts and the character controller take `ConvexShape` only, so a triangle mesh can never be cast.

**2. Layers and surfaces.**
- `CollisionLayers { member: LayerMask, collides_with: LayerMask }` with `LayerMask(u32)`, set on `ColliderBuilder` and mapped to Rapier's interaction groups.
- The engine names no layers. The game names its layers in data (IMAGINE: `world`, `character`, `prop`, `debris`, `trigger`, `projectile`) and maps them to bits once at setup.
- `SurfaceId(u16)` on every collider, stored in the collider's user data. The game owns the table of surfaces (IMAGINE: concrete, brick, wood, steel, glass, plaster…, from the `material` tag).
- **One query filter.** `QueryFilter { layers: LayerMask, exclude_body: Option<BodyId>, include_sensors: bool }` is taken by `ray_cast`, `shape_cast` and `intersects_shape`.
  - Delete `ray_cast_excluding` and `intersects_shape_excluding`: one query path each.
  - Every hit (`RayCastHit`, `ShapeCastHit`) reports `surface: SurfaceId`.
  - Migrate every caller in `kits/fps`, `games/imagine` and `games/sandbox`. The weapon ray uses the game's projectile layer filter; the character uses world, prop and character layers and excludes its own body.
- Continuous collision detection for fast bodies stays as it is.

**3. Cooked collision (`engine-assets` + `tools/cook`).**
- A new record in `MeshPack`: `collision: Vec<CollisionSet>`. Each set is a node index plus its shapes: `Box { pose, half_extents }`, `Hull { points }`, `TriMesh { positions, indices }`, or `Decomposed { hulls }`. Bump the format version and keep the validation strong (counts, finite values, index ranges).
- How the cooker picks a shape, in order:
  1. A child node named `<object>_COL`, using its `collision` tag: `box`, `hull`, `mesh` or `decompose` (default `hull`).
  2. Otherwise, by the object's `role` tag: `static` gives a triangle mesh of LOD0; `prop`, `mover` and `breakable` give one convex hull of LOD0 when it is convex within tolerance, else a decomposition; `trigger` gives a box sensor.
  3. Otherwise nothing, and the cook report lists the object.
- Convex decomposition runs **at cook time** (Parry's VHACD, `SharedShape::convex_decomposition_with_params`), with tunable resolution and a hull cap (default 16). It is deterministic: same input, same hulls (test it). The runtime never decomposes.
- The material tag becomes a surface name on the set; the game maps names to `SurfaceId`.
- `engine-physics` builds runtime shapes from the cooked records: `ColliderShape::from_cooked(&CollisionShape) -> Result<_, PhysicsError>`. `engine-physics` may depend on `engine-assets` (layer 1); add it to `layers.json`.

**4. Levels.** `engine-scene` shapes keep their box, sphere and capsule kinds. The level model can now also reference a cooked collision set by asset path and node, so a level can mix greybox shapes with cooked geometry; the world-pipeline brief uses this.

## Tests
- Shapes:
  - validation of every constructor (degenerate hull, bad index, NaN, empty compound, 65 parts);
  - a triangle mesh on a dynamic body is refused;
  - a shared shape reused by two colliders.
- Character on triangle meshes:
  - a 4×4 m floor of 2 triangles, then of 512 triangles: walking straight and diagonally never changes the grounded state and never loses speed at triangle edges (internal-edge test);
  - a 30° ramp and stairs of 18 cm steps, both as triangle meshes: the character climbs both;
  - a 1.2 m mantle onto a triangle-mesh box works as on a cuboid;
  - extend the existing penetration proptest to random triangle-mesh rooms: never deeper than the skin.
- Queries:
  - a layer filter excludes the right colliders;
  - `exclude_body` still filters every collider of that body;
  - a hit's surface is the one set;
  - a ray against a 50,000-triangle mesh: report the time per cast (release), as a measurement.
- Cooker:
  - each selection rule with fixture GLBs (generate them in the test with the `gltf` writer or commit small ones);
  - decomposition of an L-shaped solid gives at least 2 hulls whose volume is within 10% of the solid's;
  - the same input gives the same bytes twice;
  - a `MeshPack` with collision round-trips and is validated on decode.
- Determinism: the sandbox and IMAGINE scripted hashes are unchanged. If the query migration changes a hash, explain exactly why in the report.

**Do not touch:** `engine/render`, `engine/netcode`, `engine/app` beyond compile fixes, `engine/ui`.

**Docs in the same change:**
- `docs/ARCHITECTURE.md` §1 (physics, assets and cook rows);
- `docs/CONTENT.md` §4.2–2.5 (the selection rules and the `collision` tag);
- a new decision in `docs/README.md` §7 (convex versus triangle-mesh split, cook-time decomposition, one query filter, game-named layers and surfaces);
- `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
