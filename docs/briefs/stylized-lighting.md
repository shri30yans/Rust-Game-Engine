# stylized-lighting: each game sets its lighting as data, and IMAGINE gets a bright, clean, readable look (2026-11-12)

**What the owner sees.** The shadows are too strong. Large parts of the map are very dark, and visibility suffers. The owner wants a bright, clean, stylized look like competitive shooters such as Valorant:
- no visible sun;
- few or no large environment shadows;
- soft, even ambient light, so every area stays readable;
- only very subtle shadows under characters and objects, for depth;
- indoor areas, corners and surfaces facing away from the light never go pitch black;
- 3D depth and material quality kept, without harsh contrast;
- performance first, especially on low-end phones.

The fix must be in the lighting system itself, not a brightness slider turned up.

**Why it happens.**
- The engine has one hard-coded realistic look, and no game can choose another. `engine/render/src/gfx/config.rs` `Env::default()` sets:
  - a strong sun (intensity 3.6, warm colour, fixed direction);
  - two-cascade shadow maps at full strength;
  - a sky with a visible sun disk (`sky_color` in `scene_lighting.wgsl` adds `pow(mu, 64)` and `pow(mu, 8)` sun lobes);
  - auto exposure and full GTAO on desktop tiers.
- No game sets `Gfx::env`; `rg '\.env\.' -g '*.rs'` finds only the renderer itself. Both games run whatever the engine picked.
- In `shade_surface`, the sun term is multiplied by the hard shadow (`* sh`). The ambient is a weak hemisphere (`hemi * albedo * ao * ambient`), and GTAO darkens it further, so every shadowed or back-facing surface falls to near black.

## How Unreal does it, and what stylized shooters do
- In Unreal the look is level data, not engine code:
  - a **Directional Light** with `Cast Shadows` and `Atmosphere Sun Light`, where turning the latter off removes the sun disk from the sky;
  - a **Sky Light** for ambient, with intensity and a lower hemisphere colour;
  - a **Post Process Volume** with fixed exposure (min EV equal to max EV, so there is no auto exposure) and AO intensity;
  - per-character **capsule shadows** (`bCastCapsuleDirectShadow` and indirect capsule shadows): cheap, soft shadows under characters that need no shadow map.
- Competitive stylized shooters light for readability: a strong, even ambient level; soft or no environment shadows; contact shadows under characters for grounding; a fixed exposure; and a softened light falloff (wrapped diffuse), so shapes read from colour and shading, not from darkness.

## Design
1. **`LightingProfile`, one type, owned by the renderer, set by the game as data.** It replaces `Env`; delete `Env` and its `Default`, so no game silently inherits a look. The profile holds:
   - `key: KeyLight`: direction, colour, intensity, `wrap` (0 to 1; diffuse becomes `saturate((n·l + wrap) / (1 + wrap))`), and `sky_disk: bool` (whether the sky draws a sun disk);
   - `ambient: Ambient`: sky colour, ground colour and intensity for the hemisphere, plus `floor`. `floor` is the minimum diffuse light any surface receives, as a fraction of the key light's diffuse, so a back face or a corner is never black. It is applied in the one lighting function, not as a post brighten;
   - `environment_shadows: EnvironmentShadows`, an enum `Off | Soft { strength, softness }`:
     - `Off` removes the shadow pass from the `FramePlan`, so it costs nothing;
     - `Soft` keeps the cascades but blends the shadow term, `mix(1, sh, strength)`, with a wider filter;
   - `contact_shadows: ContactShadows`, an enum `Off | Blob { radius_scale, opacity, softness }`: soft blob shadows projected under characters and marked dynamic objects (Unreal's capsule shadows). They run on every tier, need no shadow map, are drawn as small decal quads after the opaque pass, and take their size from each instance's bounds;
   - `ao: AmbientOcclusion { strength, minimum }`: GTAO and material AO are applied as `max(mix(1, ao, strength), minimum)`;
   - `exposure: Exposure`, an enum `Fixed(ev) | Auto { min_ev, max_ev }`;
   - fog and sky colours, as `Env` has today.

   Check every field once, when the profile is set (finite, in range, colours non-negative), with typed errors. `Gfx::set_lighting(profile)` is the only way in. The frame plan derives the shadow pass from `environment_shadows`, as it already does for the prepass. Do not add a second branch in the shader for "stylized" versus "realistic": one lighting function reads the profile's numbers, and a realistic profile is just different data.
2. **The one lighting function** in `scene_lighting.wgsl` uses the profile:
   - wrapped diffuse for the key light;
   - shadow strength applied once;
   - hemisphere ambient with the floor;
   - AO strength and minimum;
   - the specular environment term unchanged;
   - `sky_color` draws the sun lobes only when `sky_disk` is set, and fog in-scatter follows the key light colour only when `sky_disk` is set.

   The viewmodel, procedural and textured surfaces all go through it.
3. **The game sets its look in data.**
   - `games/<game>/data/lighting.ron`, loaded through the game's existing data path and hot-reloaded with keep-last-good, like `tuning.ron`. A level may override it with an optional `lighting` field in its level file; the level wins.
   - IMAGINE ships a bright stylized profile:
     - no sky disk;
     - `environment_shadows: Off`;
     - `contact_shadows: Blob` for characters and props;
     - high ambient with a floor of about 0.45;
     - wrap about 0.5;
     - AO strength about 0.4 with minimum 0.7;
     - fixed exposure;
     - a clear, saturated sky.
   - Sandbox ships a neutral profile with soft shadows (`Soft { strength: 0.35, .. }`), to keep the shadow path tested.
   - Tune IMAGINE by looking at the combat scene at High and Low, and say what you changed and why.
4. **Phones.** With `environment_shadows: Off` the shadow pass and its targets are not created. Blob shadows are the only shadows on every tier. Record the frame cost on this Mac at 1080p before and after, for IMAGINE at High and at Low (frame p50 and p99, and the shadow pass time before), with the existing bench or a headless timing run. Do not run on the phone unless one is attached (`adb devices`).

Tests:
- profile validation rejects each bad field, with a typed error naming it;
- `FramePlan` has no shadow pass and allocates no shadow targets when environment shadows are `Off`, and has them when `Soft`;
- **no pitch black, judged on pixels:** IMAGINE's combat scene at every tier through `render::golden`:
  - the 5th-percentile luminance of the frame is above a floor;
  - an indoor or back-facing region is above a floor;
  - add a "percentile luminance" expectation to `render::golden::Plausible` for this, with its own unit test;
- blob shadows: a character standing on a flat floor darkens the floor directly under it by the set opacity, within tolerance, and leaves the floor 2 m away untouched;
- the sky has no sun disk when `sky_disk` is false: the sky pixels in the key-light direction match the surrounding sky gradient within tolerance;
- re-record the IMAGINE and Sandbox goldens that change, look at each one, and describe in the report what changed and why.

Decision: written into `docs/RENDERING.md` §10: lighting is game data through one `LightingProfile` (as Unreal's Directional Light, Sky Light and Post Process Volume); stylized readability through wrapped diffuse, an ambient floor, soft or no environment shadows and capsule-style blob contact shadows; `Env` deleted.

Running at the same time:
- `light-cache-1` (`engine/light`, `tools/bake`): do not touch those;
- `match-events` and `weapons-data` (`kits/fps`, IMAGINE `sim.rs`, IMAGINE viewmodel files): keep IMAGINE edits to the lighting data file and its loading.
- The reviewer integrates.

Read first:
- `AGENTS.md`;
- `docs/RENDERING.md`;
- `engine/render/src/gfx/config.rs` (`Env`, `Post`), `gfx/frame/prepare.rs`;
- `engine/render/src/shaders/scene_lighting.wgsl`, `scene.wgsl`, `shadow.wgsl`, `post.wgsl`;
- `engine/render/src/frame_plan.rs`, `tier.rs`;
- `engine/render/src/golden/`;
- IMAGINE's `games/imagine/game/src/data.rs` (how tuning loads and hot-reloads).

Docs in the same change:
- `docs/RENDERING.md` (the lighting model and the profile);
- `docs/ARCHITECTURE.md` §1 (render row);
- the decision in `docs/RENDERING.md` §10;
- `games/imagine/docs/` (the look);
- `docs/briefs/README.md` status `review`.

Small choices are yours. For naming, plumbing or where a function lives, pick the simplest option that keeps one path, say it in the report, and continue. Stop only when the goal itself conflicts.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
