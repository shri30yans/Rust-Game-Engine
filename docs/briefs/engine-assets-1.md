# engine-assets-1: the asset system and the cooker, slice 1: weapons from Blender to cooked, loadable data (audit H12, lane C steps C2–C3)

Goal: the owner's weapon pack (`games/imagine/content/weapons_batch1/export/*.glb`) becomes cooked, compact, validated engine data that a game can load in the background, with hot reload in dev. Done means: (1) `cargo run -p cook -- games/imagine/content/weapons_batch1/export games/imagine/content/cooked` turns every weapon `.glb` into a cooked mesh pack and KTX2 textures, deterministically (same input → byte-identical output); (2) `engine-assets` loads a cooked weapon on a background thread through a typed handle and exposes its meshes (with LODs), materials, texture data transcoded for the GPU in use, node hierarchy, sockets and animation clips; (3) editing a `.glb` and re-cooking swaps the asset in a running debug build; (4) nothing in either crate names IMAGINE or a weapon (D24): the tests use the pack as fixtures through paths passed in.

Read first: `AGENTS.md`; `docs/ARCHITECTURE.md` §0–1; `docs/CONTENT.md` §2 (conventions, tags, the cooking plan) and §5; `docs/README.md` §7 D16, D24, D25, D31, D54; `docs/research/2026-11-11-engine-audit.md` H2, H3, H12 and §F; `games/imagine/content/weapons_batch1/export/README.md` (the exact conventions of the files: axes, root/pivot/socket nodes, materials, missing tangents, clips at 30 fps, `_mobile` variants); `engine/core/src/watch*`, `engine/jobs/src/background.rs`; `engine/render/src/mesh.rs` (today's vertex format, for contrast only: do not change render here).

## Design

**New crate `engine/assets` → package `engine-assets`, layer 1** (add to the workspace and `tools/check-layers/layers.json`). Depends on `engine-core`, `engine-jobs`, `glam`, `thiserror`, `serde`/`postcard` (or a hand-written little-endian codec with a version header; pick one and justify), and the pure-Rust `basisu` transcoder crate (verify it builds on macOS, Linux, Windows, Android and wasm targets; if it does not, report and propose the alternative before using a C++ binding). No GPU dependency: the renderer uploads what this crate hands out.

- **Cooked formats** (`src/format/`), all versioned with a magic, a format version and a content hash of the source:
  - `MeshPack`: per mesh, per LOD: a **position stream** (16-bit quantised inside the mesh bounding box, 8 bytes per vertex incl. padding, or 12-byte `f32` if quantisation error > 0.1 mm for the box; record which), an **attribute stream** (octahedral normal 4 bytes, tangent 4 bytes with sign, UV as two `f16` or unorm16 4 bytes), and indices (u16 when < 65,536 vertices, else u32). Bounds (box + sphere) per mesh and LOD. LOD switch distances as screen-size thresholds. This is the layout render will adopt in the compact-records brief (audit H2, H3), so make the stream split explicit.
  - `Material`: metal/rough PBR factors + texture references (base colour sRGB, ORM, normal, emissive) + an `emissive` flag for glow overlays.
  - `Skeleton`-free `NodeTree`: names, parents (index < own index), rest TRS. Sockets are nodes whose names match the pack's `<TAG>_Socket_<Name>` convention, exposed through a generic `sockets()` view with a name, never hard-coded names.
  - `Clip`: name, duration, fps, per-node TRS channels (sampled keys, linear), loop flag if the source says so (if glTF has no loop flag, the game decides; do not guess).
  - Textures: KTX2 with Basis UASTC (one file for every platform).
- **Runtime** (`src/store.rs`, `src/handle.rs`, `src/load.rs`):
  - `Handle<T>`: a typed id + generation; cloning bumps a ref count kept in the store, not in the handle (no `Arc` per asset in the hot path); `AssetStore` owns slots per type, reports `Loading | Ready | Failed(error)`.
  - Loads run on `engine-jobs`' `BackgroundPool`; results are handed over at a `apply_loaded()` call the app makes once per frame (no locks in the game's reads).
  - `TextureData::transcode(target: GpuTextureFormatFamily)` where the family is `Bc7 | Astc4x4 | Etc2 | Rgba8` (an engine enum, not a wgpu type), chosen by the caller from the device profile.
  - Dev hot reload: the store polls cooked files through `engine-core`'s existing `watch` (no second watcher) and, on change, loads the new version in the background and swaps it at `apply_loaded()`; a failed load keeps the last good version and reports the error (same contract as levels, D41).
  - Unload: when a ref count reaches zero, the slot is freed at `apply_loaded()` and the caller is told (an event list) so the renderer can free GPU memory.

**New tool `tools/cook` → package `cook`, layer 5** (zone `tools`). Depends on `gltf`, `meshopt`, a MikkTSpace crate, `image` (PNG decode), and a Basis encoder. Encoder: prefer the upstream `basisu` command-line encoder if present on PATH (install with Homebrew if a formula exists, and document it in `docs/README.md` §3 Setup); otherwise a Rust binding crate to the encoder used **only inside `tools/cook`** (a tool may depend on C++ through a binding; our own code still has no `unsafe`). State which in the report.
- Mesh steps: read triangles; generate MikkTSpace tangents when absent (the pack omits them on purpose); weld; `meshopt` vertex-cache and vertex-fetch optimisation; LODs with `meshopt` simplify (LOD1 50 %, LOD2 25 % of triangles, stop when error exceeds a threshold); quantise as above.
- Texture steps: PNG → mip chain → KTX2 UASTC (sRGB flag for base colour and emissive, linear for ORM and normal; normal maps with the encoder's normal-map mode).
- Read node `extras` (Blender custom properties) into a `tags` map per node for later use (ASSETS §2.3); unknown tags are kept as strings, not interpreted.
- Output: `content/cooked/<name>.meshpack` + `content/cooked/textures/<hash>.ktx2` + a `<name>.report.ron` (triangles per LOD, bytes per stream, texture bytes per platform family). `content/cooked/` is git-ignored already.
- Cooking is incremental: an output whose recorded source hash matches is skipped. Same input → same bytes (no timestamps, sorted maps).

**Tests:**
- Cook the pistol and RAVEN-7 `_mobile` files in a temp dir: succeeds, deterministic (cook twice, compare bytes), report numbers sane (tri counts match the pack README within LOD0).
- Round trip: cooked mesh → decoded positions within quantisation error of the source; normals within 1°; indices identical in meaning (same triangles after optimisation, compared as sets).
- Tangents exist and are unit length with sign ±1.
- Sockets: every `*_Socket_*` node of the pistol appears in `sockets()` with the right world transform.
- Clips: `Fire` of the pistol has the documented duration and channels.
- Store: load in background, handle states, ref counting frees the slot, hot reload swaps on change and keeps the last good version on a broken file (property test on random byte corruption: never panics, always a typed error).
- Transcode: a KTX2 fixture transcodes to `Rgba8` and to `Bc7` with the expected sizes.

**Do not touch:** `engine/render`, `engine/app`, `engine/ui`, `games/imagine/game`, `kits/` (other briefs are editing them now). `games/imagine/content/` is input only: never modify it.

**Docs in the same change:** `docs/ARCHITECTURE.md` §1 (`engine-assets` and `tools/cook` rows), `docs/CONTENT.md` §5 (what slice 1 cooks, the encoder, the cooked layout), `docs/research/2026-11-11-engine-audit.md` H12 (slice 1 done, render upload next), `docs/README.md` §2 and §3 (any new tool), `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
