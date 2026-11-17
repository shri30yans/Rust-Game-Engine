# Batch 1 weapons: game-ready GLB assets

Seven weapons built in headless Blender from the approved concepts. Every file is **plain glTF 2.0 core**: no Draco, no meshopt, no KTX2, no `KHR_texture_transform`, no `KHR_materials_emissive_strength`, no `KHR_animation_pointer`, no skins, no morph targets. Textures are embedded PNGs, meshes are triangles, one scene per file, and animations are named TRS node clips.

## Files

| Weapon | Desktop GLB (2048 textures) | Mobile GLB (1024 textures) | Triangles | Materials | Validator (errors / warnings) |
|---|---|---|---|---|---|
| RAVEN-7 | `raven7.glb` (7.7 MB) | `raven7_mobile.glb` (2.9 MB) | 14,256 | M_RAVEN7, M_RAVEN7_Glow | 0/5 desktop, 0/5 mobile |
| BREACHER | `breacher.glb` (5.3 MB) | `breacher_mobile.glb` (2.2 MB) | 12,040 | M_BREACHER, M_BREACHER_Glow | 0/4 desktop, 0/4 mobile |
| OBLIVION | `oblivion.glb` (6.9 MB) | `oblivion_mobile.glb` (2.7 MB) | 13,098 | M_OBLIVION_Glow, M_OBLIVION | 0/3 desktop, 0/3 mobile |
| PARTYCRASHER | `partycrasher.glb` (6.0 MB) | `partycrasher_mobile.glb` (2.1 MB) | 8,762 | M_PARTYCRASHER, M_PARTYCRASHER_Glow | 0/3 desktop, 0/3 mobile |
| GRAVITY WELL | `gravitywell.glb` (5.1 MB) | `gravitywell_mobile.glb` (1.9 MB) | 8,076 | M_GRAVITYWELL, M_GRAVITYWELL_Glow | 0/4 desktop, 0/4 mobile |
| SHINOBI | `shinobi.glb` (5.8 MB) | `shinobi_mobile.glb` (2.0 MB) | 4,064 | M_SHINOBI, M_SHINOBI_Glow | 0/2 desktop, 0/2 mobile |
| PISTOL | `pistol.glb` (295 KB) / `pistol_bronze.glb` (295 KB) | `pistol_mobile.glb` (295 KB) | 6,321 | steel_slide, polymer_frame, checkered_grip, steel_barrel, steel_gunmetal, steel_polished_rim | 0/0 desktop, 0/0 mobile |

The extensionsUsed list is empty in all files. Separate texture PNGs sit in `textures/<weapon>/`, and per-weapon machine-readable reports are in `<weapon>_report.json`.

## Conventions for a custom loader

- **Units and axes:** metres, right-handed, **+Y up** (glTF standard). The weapon points along **-Z** (muzzle and blade tip), and its right side faces **+X**. In Blender that is +Y forward, +Z up.
- **Origin:** the root node sits at the grip (hand anchor). For SHINOBI it is the hand position on the handle.
- **Hierarchy:** `<TAG>` (root, never animated) → `<TAG>_Pivot` (whole-weapon motion: recoil, sway, equip, inspect, swings) → mesh nodes and socket nodes. Moving parts are separate mesh nodes with their pivot at their own origin. FX meshes for SHINOBI and PARTYCRASHER are nested where noted below.
- **Sockets:** empty nodes named `<TAG>_Socket_<Name>` (Muzzle, Eject, Sight, SupportHand, Tip, and so on). Use them for VFX, shell ejection, ADS alignment and IK.
- **Materials:** `M_<TAG>` is metal/rough PBR with four embedded textures: `baseColorTexture` (sRGB, with AO, decals, livery and edge wear baked in), `metallicRoughnessTexture` (G = roughness, B = metallic, R unused = 1), `normalTexture` (tangent space, OpenGL/+Y green, MikkTSpace) and `emissiveTexture` with `emissiveFactor` = (1,1,1). **No TANGENT attribute is exported**, so generate MikkTSpace tangents at load. That matches how the normals were baked, and the glTF spec says to do this when tangents are missing.
- `M_<TAG>_Glow` is a flat emissive material (emissiveFactor = accent colour, no textures) used only by `*FX`, `*LED*` and `*Pip*` overlay meshes. Add bloom in your renderer to get the neon look.
- **Light animation without extensions:** core glTF cannot animate material values, so every "light ramps up / flashes / chases" effect is done with TRS on the FX overlay nodes. **Scale ≈ 0.01 means off/hidden** and scale 1 means lit. Values above 1 are flares. If you would rather drive emissive intensity yourself, you can read the FX node scale as a 0–1+ intensity curve.
- **Clips:** 30 fps. Each clip is fully sampled (LINEAR, one key per frame) and starts at t = 0. A clip only has channels for the nodes it moves (listed per clip below), so **reset every animated node to its rest TRS (the node TRS in the file) before you start a clip**. Looping clips end on their first pose. Non-looping clips end at rest, with these exceptions: OBLIVION `Fire` leaves `Warhead` hidden until `Reload`; PARTYCRASHER `Fire` leaves the charge and LED FX dark until `Reload`; OBLIVION `LockOn` ends in the locked state (hold the last frame); GRAVITY WELL `Charge` ends with the orb at full size, so chain straight into `Fire`.
- **Mobile:** each file has 2 materials (body atlas + glow) and 4–16 small mesh nodes. The `_mobile` files are identical except their textures are 1024². Validator warnings are only `MESH_PRIMITIVE_GENERATED_TANGENT_SPACE` (tangents intentionally omitted; see above). There are 0 errors.

## RAVEN-7 (Northline Ordnance starter AR)

- Files: `raven7.glb`, `raven7_mobile.glb`. Triangles: **14,256**. Materials: `M_RAVEN7`, `M_RAVEN7_Glow`.
- Textures (2048 / 1024): `textures/raven7/RAVEN7_BaseColor_2048.png`, `textures/raven7/RAVEN7_ORM_2048.png`, `textures/raven7/RAVEN7_Normal_2048.png`, `textures/raven7/RAVEN7_Emissive_2048.png` and the matching `_1024` versions.
- Hero render: `../renders/raven7_hero.png`. Preview: `../renders/raven7_preview.mp4`.

| Clip | Frames (30 fps) | Seconds | Loop | What it does | Animated nodes |
|---|---|---|---|---|---|
| `Idle` | 0–60 | 2.00 | yes | breathing sway + light-pipe pulse | GlowFX, OpticFX, Pivot, RingFX |
| `Fire` | 0–9 | 0.30 | no | one shot: kick, bolt cycle, trigger, light flash | Bolt, GlowFX, OpticFX, Pivot, Trigger |
| `Reload` | 0–66 | 2.20 | no | mag out (drops off-screen), fresh mag in at f30, slap, charge | Bolt, ChargingHandle, GlowFX, Mag, Pivot |
| `Equip` | 0–24 | 0.80 | no | raise from below + charge, lights boot up | Bolt, ChargingHandle, GlowFX, OpticFX, Pivot |
| `Inspect` | 0–120 | 4.00 | no | showcase both sides, mag check, chamber check | Bolt, ChargingHandle, GlowFX, Mag, Pivot |

Node hierarchy:
```
- RAVEN7 (node)
  - RAVEN7_Pivot (node)
    - RAVEN7_Body (mesh)
    - RAVEN7_Bolt (mesh)
    - RAVEN7_ChargingHandle (mesh)
    - RAVEN7_GlowFX (mesh)
    - RAVEN7_Mag (mesh)
    - RAVEN7_OpticFX (mesh)
    - RAVEN7_RingFX (mesh)
    - RAVEN7_Socket_Eject (node)
    - RAVEN7_Socket_MagWell (node)
    - RAVEN7_Socket_Muzzle (node)
    - RAVEN7_Socket_Sight (node)
    - RAVEN7_Socket_SupportHand (node)
    - RAVEN7_Trigger (mesh)
```

## BREACHER (Northline Ordnance pump breaching shotgun)

- Files: `breacher.glb`, `breacher_mobile.glb`. Triangles: **12,040**. Materials: `M_BREACHER`, `M_BREACHER_Glow`.
- Textures (2048 / 1024): `textures/breacher/BREACHER_BaseColor_2048.png`, `textures/breacher/BREACHER_ORM_2048.png`, `textures/breacher/BREACHER_Normal_2048.png`, `textures/breacher/BREACHER_Emissive_2048.png` and the matching `_1024` versions.
- Hero render: `../renders/breacher_hero.png`. Preview: `../renders/breacher_preview.mp4`.

| Clip | Frames (30 fps) | Seconds | Loop | What it does | Animated nodes |
|---|---|---|---|---|---|
| `Idle` | 0–60 | 2.00 | yes | sway + optic pulse | OpticFX, Pivot |
| `Fire` | 0–14 | 0.47 | no | heavy kick + crown heat flash (follow with Pump) | CrownFX, Pivot, Trigger |
| `Pump` | 0–18 | 0.60 | no | rack the pump; spawn a shell at Socket_Eject on f7 | Pivot, Pump |
| `Reload` | 0–60 | 2.00 | no | roll to show the port, load two shells from the side saddle, rack | Pivot, Pump, Shell |
| `Equip` | 0–24 | 0.80 | no | raise + rack | OpticFX, Pivot, Pump |
| `Inspect` | 0–120 | 4.00 | no | showcase both sides, crown glow, pump check | CrownFX, Pivot, Pump |

Node hierarchy:
```
- BREACHER (node)
  - BREACHER_Pivot (node)
    - BREACHER_Body (mesh)
    - BREACHER_CrownFX (mesh)
    - BREACHER_OpticFX (mesh)
    - BREACHER_Pump (mesh)
    - BREACHER_Shell (mesh)
    - BREACHER_Socket_Eject (node)
    - BREACHER_Socket_LoadPort (node)
    - BREACHER_Socket_Muzzle (node)
    - BREACHER_Socket_Sight (node)
    - BREACHER_Socket_SupportHand (node)
    - BREACHER_Trigger (mesh)
```

## OBLIVION (Craterforge Heavy bunker-buster launcher)

- Files: `oblivion.glb`, `oblivion_mobile.glb`. Triangles: **13,098**. Materials: `M_OBLIVION_Glow`, `M_OBLIVION`.
- Textures (2048 / 1024): `textures/oblivion/OBLIVION_BaseColor_2048.png`, `textures/oblivion/OBLIVION_ORM_2048.png`, `textures/oblivion/OBLIVION_Normal_2048.png`, `textures/oblivion/OBLIVION_Emissive_2048.png` and the matching `_1024` versions.
- Hero render: `../renders/oblivion_hero.png`. Preview: `../renders/oblivion_preview.mp4`.
- Revision 2: a loaded RPG-style rocket is seated in the muzzle. `OBLIVION_Warhead` is the whole rocket: the motor body (it runs about 17 cm back inside the tube, so it can slide in and out), a steel band and four small fins at the tube mouth, a wide bronze warhead with an orange band, a chisel nose cone with three claws and a steel fuze tip. Its pivot sits on the muzzle plane. `Fire` launches it forward with a spin and hides it at f7. In `Reload` a fresh rocket appears in front of the muzzle, lines up, slides straight in and twist-locks. The new `OBLIVION_Socket_RocketTip` marks the nose at rest.

| Clip | Frames (30 fps) | Seconds | Loop | What it does | Animated nodes |
|---|---|---|---|---|---|
| `Idle` | 0–60 | 2.00 | yes | heavy sway + amber optic breathing | OpticFX, Pivot |
| `LockOn` | 0–45 | 1.50 | no | optic light ramps up, three lock pips light in sequence; hold last frame while locked | LockPip1, LockPip2, LockPip3, OpticFX, Pivot |
| `Fire` | 0–30 | 1.00 | no | launch: the rocket accelerates out of the tube spinning, disappears at f7 (scale 0.01) and stays hidden until Reload; back-blast flash | BackblastFX, OpticFX, Pivot, Trigger, Warhead |
| `Reload` | 0–75 | 2.50 | no | a fresh rocket is brought up in front of the muzzle, lines up with the tube, slides straight in and twist-locks; optic re-arms | OpticFX, Pivot, Warhead |
| `Equip` | 0–30 | 1.00 | no | shoulder the launcher, optic boots | OpticFX, Pivot |
| `Inspect` | 0–120 | 4.00 | no | showcase both sides, warhead twist check | Pivot, Warhead |

Node hierarchy:
```
- OBLIVION (node)
  - OBLIVION_Pivot (node)
    - OBLIVION_BackblastFX (mesh)
    - OBLIVION_Body (mesh)
    - OBLIVION_LockPip1 (mesh)
    - OBLIVION_LockPip2 (mesh)
    - OBLIVION_LockPip3 (mesh)
    - OBLIVION_OpticFX (mesh)
    - OBLIVION_Socket_Backblast (node)
    - OBLIVION_Socket_Muzzle (node)
    - OBLIVION_Socket_RocketTip (node)
    - OBLIVION_Socket_Shoulder (node)
    - OBLIVION_Socket_Sight (node)
    - OBLIVION_Socket_SupportHand (node)
    - OBLIVION_Trigger (mesh)
    - OBLIVION_Warhead (mesh)
```

## PARTYCRASHER (Pylon Works cluster-bomb launcher)

- Files: `partycrasher.glb`, `partycrasher_mobile.glb`. Triangles: **8,762**. Materials: `M_PARTYCRASHER`, `M_PARTYCRASHER_Glow`.
- Textures (2048 / 1024): `textures/partycrasher/PARTYCRASHER_BaseColor_2048.png`, `textures/partycrasher/PARTYCRASHER_ORM_2048.png`, `textures/partycrasher/PARTYCRASHER_Normal_2048.png`, `textures/partycrasher/PARTYCRASHER_Emissive_2048.png` and the matching `_1024` versions.
- Hero render: `../renders/partycrasher_hero.png`. Preview: `../renders/partycrasher_preview.mp4`.

| Clip | Frames (30 fps) | Seconds | Loop | What it does | Animated nodes |
|---|---|---|---|---|---|
| `Fire` | 0–24 | 0.80 | no | launch: pod kicks, the six charges and the six LEDs chase 1..6 then stay dark until Reload | ChargeFX0, ChargeFX1, ChargeFX2, ChargeFX3, ChargeFX4, ChargeFX5, ChargeFX6, LED1, LED2, LED3, LED4, LED5, LED6, Pivot, Pod, Trigger |
| `Reload` | 0–72 | 2.40 | no | pod slides out, spins a full turn while the six charges re-arm one by one, slams home; LEDs refill 1..6 | ChargeFX0, ChargeFX1, ChargeFX2, ChargeFX3, ChargeFX4, ChargeFX5, ChargeFX6, LED1, LED2, LED3, LED4, LED5, LED6, Pivot, Pod |
| `Idle` | 0–60 | 2.00 | yes | sway + LED ripple | LED1, LED2, LED3, LED4, LED5, LED6, Pivot |
| `Equip` | 0–30 | 1.00 | no | raise; LEDs boot 1..6 | LED1, LED2, LED3, LED4, LED5, LED6, Pivot |
| `Inspect` | 0–120 | 4.00 | no | showcase both sides, pod twist (120 deg = symmetric rest) | Pivot, Pod |

Node hierarchy:
```
- PARTYCRASHER (node)
  - PARTYCRASHER_Pivot (node)
    - PARTYCRASHER_Body (mesh)
    - PARTYCRASHER_LED1 (mesh)
    - PARTYCRASHER_LED2 (mesh)
    - PARTYCRASHER_LED3 (mesh)
    - PARTYCRASHER_LED4 (mesh)
    - PARTYCRASHER_LED5 (mesh)
    - PARTYCRASHER_LED6 (mesh)
    - PARTYCRASHER_Pod (mesh)
      - PARTYCRASHER_ChargeFX0 (mesh)
      - PARTYCRASHER_ChargeFX1 (mesh)
      - PARTYCRASHER_ChargeFX2 (mesh)
      - PARTYCRASHER_ChargeFX3 (mesh)
      - PARTYCRASHER_ChargeFX4 (mesh)
      - PARTYCRASHER_ChargeFX5 (mesh)
      - PARTYCRASHER_ChargeFX6 (mesh)
    - PARTYCRASHER_Socket_Muzzle (node)
    - PARTYCRASHER_Socket_Sight (node)
    - PARTYCRASHER_Socket_SupportHand (node)
    - PARTYCRASHER_Trigger (mesh)
```

## GRAVITY WELL (Shirokaze Labs gravity cannon)

- Files: `gravitywell.glb`, `gravitywell_mobile.glb`. Triangles: **8,076**. Materials: `M_GRAVITYWELL`, `M_GRAVITYWELL_Glow`.
- Textures (2048 / 1024): `textures/gravitywell/GRAVITYWELL_BaseColor_2048.png`, `textures/gravitywell/GRAVITYWELL_ORM_2048.png`, `textures/gravitywell/GRAVITYWELL_Normal_2048.png`, `textures/gravitywell/GRAVITYWELL_Emissive_2048.png` and the matching `_1024` versions.
- Hero render: `../renders/gravitywell_hero.png`. Preview: `../renders/gravitywell_preview.mp4`.

| Clip | Frames (30 fps) | Seconds | Loop | What it does | Animated nodes |
|---|---|---|---|---|---|
| `Idle` | 0–60 | 2.00 | yes | slow coil spin (1 rev / 2 s) + glow breathing | CoilGlowFX, Coils, Pivot |
| `Charge` | 0–45 | 1.50 | no | coils spin up (accelerating), glow swells and the gravity orb grows at the muzzle; chain into Fire from the last frame | CoilGlowFX, Coils, OrbFX, Pivot |
| `Fire` | 0–18 | 0.60 | no | orb launches from the muzzle, coils dump their charge | CoilGlowFX, Coils, OrbFX, Pivot, Trigger |
| `Reload` | 0–60 | 2.00 | no | spent energy cell ejects from the rear bay, fresh cell slots in, coils restart | Cell, CoilGlowFX, Coils, Pivot |
| `Equip` | 0–24 | 0.80 | no | raise; coils spin up | CoilGlowFX, Coils, Pivot |
| `Inspect` | 0–120 | 4.00 | no | showcase both sides, cell check, coils turning | Cell, Coils, Pivot |

Node hierarchy:
```
- GRAVITYWELL (node)
  - GRAVITYWELL_Pivot (node)
    - GRAVITYWELL_Body (mesh)
    - GRAVITYWELL_Cell (mesh)
    - GRAVITYWELL_CoilGlowFX (mesh)
    - GRAVITYWELL_Coils (mesh)
    - GRAVITYWELL_OrbFX (mesh)
    - GRAVITYWELL_Socket_Muzzle (node)
    - GRAVITYWELL_Socket_Sight (node)
    - GRAVITYWELL_Socket_SupportHand (node)
    - GRAVITYWELL_Trigger (mesh)
```

## SHINOBI (Shirokaze Labs energy katana)

- Files: `shinobi.glb`, `shinobi_mobile.glb`. Triangles: **4,064**. Materials: `M_SHINOBI`, `M_SHINOBI_Glow`.
- Textures (2048 / 1024): `textures/shinobi/SHINOBI_BaseColor_2048.png`, `textures/shinobi/SHINOBI_ORM_2048.png`, `textures/shinobi/SHINOBI_Normal_2048.png`, `textures/shinobi/SHINOBI_Emissive_2048.png` and the matching `_1024` versions.
- Hero render: `../renders/shinobi_hero.png`. Preview: `../renders/shinobi_preview.mp4`.
- Revision 2: the tsuka (handle) is one straight piece with a constant elliptical cross-section (no taper or swell), on the blade axis. It has an even, mirror-symmetric diamond wrap: 10 diamonds along the exposed handle, two per turn, so the left and right sides match. The old label plate and gold pinstripes were removed so the wrap runs uninterrupted from the fuchi to the kashira.

| Clip | Frames (30 fps) | Seconds | Loop | What it does | Animated nodes |
|---|---|---|---|---|---|
| `Idle` | 0–60 | 2.00 | yes | breathing sway + jade glow pulse | DiscFX, EdgeFX, Pivot |
| `Equip` | 0–24 | 0.80 | no | quick-draw along the blade axis (as if from a saya) then flourish | DiscFX, EdgeFX, Pivot |
| `SwingLight` | 0–18 | 0.60 | no | fast horizontal slash right-to-left; use Socket_TrailStart/TrailEnd for the trail ribbon; hit window f6-f10 | EdgeFX, Pivot |
| `SwingHeavy` | 0–36 | 1.20 | no | overhead wind-up and downward cleave; hit window f19-f24 | DiscFX, EdgeFX, Pivot |
| `Parry` | 0–24 | 0.80 | no | blade raised flat across the body; the parry disc flares and spins (block window f5-f16) | Disc, DiscFX, EdgeFX, Pivot |
| `Inspect` | 0–120 | 4.00 | no | turn the blade edge-on, full twist to show both faces, disc spin | Disc, DiscFX, Pivot |

Node hierarchy:
```
- SHINOBI (node)
  - SHINOBI_Pivot (node)
    - SHINOBI_Body (mesh)
    - SHINOBI_Disc (mesh)
    - SHINOBI_DiscFX (mesh)
    - SHINOBI_EdgeFX (mesh)
    - SHINOBI_Socket_BladeBase (node)
    - SHINOBI_Socket_OffHand (node)
    - SHINOBI_Socket_Tip (node)
    - SHINOBI_Socket_TrailEnd (node)
    - SHINOBI_Socket_TrailStart (node)
```

## PISTOL (Long-slide tactical pistol — Midnight Slate & Bronze colorways)

- Files: `pistol.glb` (Midnight Slate), `pistol_mobile.glb`, `pistol_bronze.glb` (Bronze & Black). Triangles: **6,321**. Materials: `steel_slide`, `polymer_frame`, `checkered_grip`, `steel_barrel`, `steel_gunmetal`, `steel_polished_rim`.
- Hero renders: `../renders/pistol_hero.png`, `../renders/pistol_bronze_hero.png`. Side & perspective renders: `../renders/pistol_side_midnight.png`, `../renders/pistol_persp_midnight.png`, `../renders/pistol_side_bronze.png`, `../renders/pistol_persp_bronze.png`.
- Build script: `blender/pistol/build_pistol.py` (also at `tools/blender/make_pistol.py`).

| Clip | Frames (30 fps) | Seconds | Loop | What it does | Animated nodes |
|---|---|---|---|---|---|
| `Idle` | 0–60 | 2.00 | yes | breathing sway | PISTOL_Pivot |
| `Fire` | 0–9 | 0.30 | no | one shot: muzzle flip, slide blowback, trigger pull | PISTOL_Pivot, slide, trigger |
| `Reload` | 0–54 | 1.80 | no | drop magazine along grip angle, insert fresh magazine, rack slide | PISTOL_Pivot, magazine, slide |
| `Equip` | 0–24 | 0.80 | no | draw from holster and rack slide | PISTOL_Pivot, slide |
| `Inspect` | 0–90 | 3.00 | no | showcase left and right profiles with press-check | PISTOL_Pivot, slide |

Node hierarchy:
```
- PISTOL (node)
  - PISTOL_Pivot (node)
    - barrel (mesh)
    - controls (mesh)
    - frame (mesh)
    - grip_panels (mesh)
    - magazine (mesh)
    - PISTOL_Socket_Eject (node)
    - PISTOL_Socket_MagWell (node)
    - PISTOL_Socket_Muzzle (node)
    - PISTOL_Socket_Sight (node)
    - sights (mesh)
    - slide (mesh)
    - socket_eject (node)
    - socket_grip (node)
    - socket_mag (node)
    - socket_muzzle (node)
    - socket_sight (node)
    - trigger (mesh)
```

## Rebuilding from source

Each weapon has a standalone build script, `blender/<weapon>/build_<weapon>.py`, that regenerates everything from scratch: model, livery and decal sheets, procedural source materials, the 2048 atlas bake, the 1024 copies, animations, both GLBs, the report JSON and the hero render.

```bash
# Blender 4.5+ / 5.2 LTS
blender -b -P blender/raven7/build_raven7.py            # full build, including the hero render
blender -b -P blender/pistol/build_pistol.py -- --color midnight
blender -b -P blender/pistol/build_pistol.py -- --color bronze --blend games/imagine/content/blender/pistol_bronze.blend --glb games/imagine/content/exported/pistol_bronze.glb
SKIP_PREVIEW=1 blender -b -P blender/raven7/build_raven7.py  # skip the preview MP4
QUICK=1 blender -b -P blender/raven7/build_raven7.py         # geometry/look check only (no bake/export)
# preview MP4 from a finished build:
blender -b blender/raven7/raven7.blend -P blender/common/render_from_blend.py -- raven7 preview
```

- Output goes to `$WEAPONS_ROOT` (default `/workspace/weapons`): `export/`, `export/textures/<weapon>/`, `renders/`, `blender/<weapon>/`.
- Fonts (SIL OFL Google Fonts) are read from `blender/common/fonts/`. Pillow is installed into `blender/common/_pydeps` on first run, using pip from Blender's own Python.
- `blender/common/wkit.py` is the shared kit (geometry helpers, materials, bake, clips, export, studio). Its code is already inlined into each `build_<weapon>.py`, so a build script needs only Blender plus the fonts folder.
- Optional env vars: `BAKE_SAMPLES` (16), `HERO_SAMPLES` (128), `PREVIEW_SAMPLES` (8), `AGX_LOOK`.
