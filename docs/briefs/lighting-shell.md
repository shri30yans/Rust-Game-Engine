# lighting-shell: baked light for each arena layout, probes for everything that moves, tile-friendly passes (GX6/GX7 revised, D18 revised, D52, D54)

Goal: arenas get convincing indirect light and reflections that cost almost nothing at run time on phones. Done means:
1. A bake tool renders, with the engine's own renderer, irradiance probes and reflection probes for the indestructible shell, once per arena layout.
2. At run time, every surface (static, moving, characters, the weapon) takes its diffuse bounce light from the irradiance grid and its specular from box-projected reflection probes.
3. A Director layout change blends between the two layouts' probe sets during the transition.
4. Render passes stop storing what nobody reads (tile-based GPUs).
5. The sample arena from `world-pipeline` looks lit rather than flat, bench-gate passes, and the four gates pass.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D17, D18, D51, D52, D54, D55;
- `docs/RENDERING.md` §4 (all) and §7 (budgets);
- `games/imagine/docs/PLAN.md` §3.3 (layouts; "each layout can have its own baked light") and §5.1;
- `engine/render/src/` (lighting, GTAO, the targets, tiers, the headless renderer);
- `docs/briefs/render-assets.md` and `docs/briefs/world-pipeline.md` (both merged before this starts: `SurfaceInputs`, probe volumes and layouts in the level).

**Why this replaces the first GI step of D18.** D18 planned fully dynamic probe GI because destruction changes the map. Two decisions since then change the trade-off:
- **D52:** destruction happens inside an indestructible shell.
- **PLAN §3.3:** an arena is a small set of named layouts.

So the shell's light can be baked per layout and stored as probes, which is cheap enough for every phone. Dynamic GI stays the later high-tier step for what breaks. Unreal does the same on mobile: precomputed lighting, volumetric lightmap probes for dynamic objects, and captured reflection probes, with Lumen only on high-end devices.

## Design

**1. The bake tool: `tools/bake`, a headless binary that uses `engine-render`.** For each layout of a level:
- **Pose:** put every mover in its layout pose and every switchable light in its layout state.
- **Irradiance grid:** for each probe volume (the `probe_*` volumes in the level, or the level bounds when there are none), place probes on a regular grid (spacing from the volume, default 1.5 m).
  - Render a small cubemap at each probe (e.g. 32² per face, HDR) with direct light, sky and the previous bounce.
  - Project it to **L1 spherical harmonics** (4 RGB coefficients).
  - Run 2 bounces: bounce 2 renders with bounce-1 probes applied.
- **Wall leaks:** a probe that ends up inside geometry (a backface test from its cubemap) is marked invalid, and the runtime skips invalid probes in its trilinear blend (renormalising the weights).
- **Reflection probes:** at each reflection probe marker, render a 128² HDR cubemap and prefilter it into its roughness mips (GGX importance sampling). Store it with its box-projection volume.
- **Output** per level and layout: `<arena>.<layout>.probes`, a versioned `engine-assets` record. It holds the grid (origin, spacing, dimensions, packed SH as f16, a validity mask) and the reflection probes (box, cubemap mips as RGB9E5 or RGBA16F; state the size). It is deterministic: the same inputs give the same bytes (test it).

**2. Runtime.**
- The grid uploads as 3D textures (one per SH band, linear filtering), with the validity mask beside them.
- The lighting function (`SurfaceInputs`, from render-assets) samples:
  - diffuse indirect from SH at the world position, normal-biased by about 0.3 m to avoid self-shadowing;
  - specular from the nearest one or two box-projected reflection probes (blended), with the sky as fallback;
  - both multiplied by GTAO and specular occlusion.
- This replaces the flat ambient term. There is one path for every tier; Low samples one reflection probe and no blend.
- **Layout transitions:** the Director's layout change already has a duration. Probe sets for the source and target layout are both resident while it runs, and the shader lerps by the transition's progress (one uniform).
- **Destruction inside the shell:** probes are unchanged; GTAO darkens the openings. Note in RENDERING.md that dynamic GI for this is the later GX7 step.

**3. Tile-friendly passes.**
- Every attachment says what it really needs:
  - depth: `StoreOp::Discard` after the last pass that reads it;
  - MSAA colour: resolve, then discard the multisampled image;
  - GTAO and bloom intermediates: discard after their consumer;
  - first use: `LoadOp::Clear`, never `Load`, unless the previous content is read (TAA history is).
- Write the table of attachments and their load and store ops into RENDERING.md §7.
- Add a debug check (dev builds) that fails if a pass loads an attachment no earlier pass stored.
- We cannot measure bandwidth on the M5. Measure frame time before and after on the M5 and on the Android device if one is attached (`scripts/build-android.sh`), and state which.

**4. Budget.** On the M5 at 1080p, High: the probe sampling costs at most 0.3 ms over flat ambient on the grid80 bench. Add a `grid40-probes` bench case with a baked fixture and record it in the baseline.

## Tests
- Bake:
  - SH projection of an analytic environment (constant, then a single direction) within 1% of the closed form;
  - a probe inside a box is marked invalid;
  - two bakes give the same bytes;
  - the record round-trips and is validated on decode.
- Runtime:
  - the trilinear blend with invalid probes renormalises (unit test on a fixture grid);
  - a box-projection unit test;
  - the layout lerp at 0, 0.5 and 1 matches the two sets and their average.
- Golden images:
  - the sample arena, layout `open` and layout `lockdown`, at High and Low;
  - a red-wall test scene where the floor next to the wall picks up red bounce (assert the channel ratio in a region).
- The attachment-ops debug check passes for every tier.
- Bench-gate passes; the new case is recorded.

**Do not touch:** `engine/netcode`, `engine/sessions`, `engine/ui`, `engine/physics`, `kits/`.

**Docs in the same change:**
- `docs/RENDERING.md` §3 (Indirect light and Reflections rows), §4.4–4.5 (revised plan: baked per layout first, dynamic later), §6.2 (the attachment ops table) and §9 (GX6/GX7 order);
- a new decision in `docs/README.md` §7 that revises D18 (bake per layout with probes; dynamic GI later and high-tier only);
- `docs/CONTENT.md` (probe volumes and how to bake);
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
## Observed before this brief (2026-11-12)
In the IMAGINE combat golden (`games/imagine/game/tests/fixtures/imagine-combat-feel.png`, 640×360), the Kiln Yard buildings render as near-black silhouettes against the sky. Find out whether this comes from the sun direction, from missing ambient or sky light (no irradiance yet), or from exposure. The baked SH grid and the reflection probes in this brief should fix it. If the cause is a bug (for example a sign error in the sun direction, or exposure metering the sky), fix it here, and say so in the report.
