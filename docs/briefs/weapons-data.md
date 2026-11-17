# weapons-data: three real weapons from the pack, defined once in data (PLAN M1.2) (2026-11-12)

**What is wrong.**
- IMAGINE has one gun. Its numbers are a `rifle` section in `games/imagine/data/tuning.ron`, its name is a string there, and its viewmodel mesh is picked separately in `present.rs`. That is two sources of truth: the HUD has shown "VX-9 CARBINE" over the pistol model (`hud-fit` brief).
- The weapon pack in `games/imagine/content/weapons_batch1/` has a rifle (RAVEN-7), a pump shotgun (BREACHER) and a pistol, with sockets and clips (`export/README.md`). The game uses only the pistol mesh, as a stand-in rifle.
- There is no loadout, no weapon swap, no aim-down-sights and no recoil pattern, so the guns cannot be judged for feel (PLAN pillar "Feels great", weight 30 %).

## How Unreal does it
- A weapon is a data asset (Lyra's `ULyraWeaponItemDefinition` and `ULyraRangedWeaponInstance`, or a `UDataAsset`). It holds the mesh, the firing values, spread curves (heat that grows per shot and recovers), the recoil and the sounds. Gameplay code reads the definition, and the HUD reads its display name from the same asset.
- An inventory (`ULyraInventoryManagerComponent` and `ULyraEquipmentManagerComponent`) holds a loadout of definitions; equipping one swaps the viewmodel and the active fire rules, with an equip time.

## Design
1. **One weapon definition per weapon, in data.** `games/imagine/data/weapons.ron` lists weapons by id. Each has:
   - a display name (the HUD reads only this);
   - its mesh, given as the cooked weapon asset (the pack's desktop and mobile variants; the device tier picks one);
   - its fire rules: fire mode (auto, semi, pump), rate, damage with distance falloff, pellets, magazine, reload time, equip time;
   - its spread: hip and ADS cones, heat added per shot, recovery per second;
   - its recoil: an authored pattern of view kicks (pitch and yaw per shot, looping past the end), recovery speed, and an ADS scale;
   - ADS: zoom (FOV scale) and time in and out;
   - its sounds, by the ids already in `sounds.ron`.
   Load checks every field once, with errors naming the file, the weapon and the field (unknown mesh, unknown sound, zero rate, empty pattern). It hot-reloads at a tick boundary with the keep-last-good rule, like `tuning.ron`. Delete the `rifle` section from `tuning.ron` and every other copy of a weapon value or name in code.
2. **The kit stays game-agnostic.** `kits/fps` gets the generic types: a `WeaponDef` (the fields above, with no names or meshes it knows), a `Loadout` of definitions, and `Equipped` state (equipping timer, ADS amount, heat, recoil index). Fire modes are an enum. The kit applies recoil to the view as a command-side input so the server and prediction agree, and decays it.
3. **Three weapons, tuned to play differently:**
   - RAVEN-7 as the rifle: automatic, mid range.
   - BREACHER as the shotgun: pump, pellets, short range, high damage close.
   - PISTOL: semi-automatic, fast equip, the secondary.
   The player's default loadout is rifle plus pistol, with the shotgun in the mode data as an alternative primary, so modes can choose (PLAN §3.2 Loadout block). Bots get loadouts from the same data.
4. **Swap and ADS.**
   - Swap with the existing input path (next and previous weapon, and the number keys while input-actions is pending).
   - ADS on the aim input: narrows spread, scales recoil, zooms FOV and slows movement by a data value.
   - The equip timer blocks fire. Reload is cancelled by a swap.
5. **Viewmodel.** The viewmodel shows the equipped weapon's own mesh from the cooked pack, placed by its grip origin (the pack's convention). Procedural motion is driven by the equipped state: sway from look input, bob from movement, kick from recoil, and the ADS pose aligning the `Sight` socket to the screen centre. Clip playback (the pack's Fire and Reload clips) is a later brief; leave the clips unused rather than half-wired.
6. **HUD.** The weapon name and ammo come from the definition and the equipped state, through the existing HUD bindings.

Tests:
- load errors for each rejected field, naming the file and the weapon;
- hot reload: a changed damage value applies at the next tick; a broken file keeps the last good set and reports;
- fire rules per mode (auto holds, semi needs a release, pump has its interval);
- the recoil pattern is followed shot by shot and loops; recovery returns the view;
- spread heat grows per shot and recovers;
- swap: the equip time blocks fire, and swap cancels reload;
- ADS changes spread, recoil and FOV by the data values;
- determinism: the same commands give the same state hash with and without the recoil path (prediction parity);
- headless IMAGINE match: the player fires each of the three weapons at a bot, and the HUD shows the right name;
- a render golden of each weapon's viewmodel at High, through `render::golden` with a "not the clear colour" expectation on the weapon region.

Decision: written into `docs/ARCHITECTURE.md` §12 (weapon definitions as data assets with a loadout and equipped state, as Lyra; the only source of a weapon's name, mesh and values).

Runs beside `match-events`, which also edits `kits/fps` and IMAGINE's `sim.rs` (events replace the kit's private logs; keep your shot, hit and damage writes easy to move onto events), and `asset-source`, which changes how IMAGINE loads content (load weapons.ron through whatever loading path main has when you start, and keep that part small). The reviewer integrates. Do not touch `engine/render`, `engine/ui` or `engine/assets`.

Read first:
- `AGENTS.md`;
- `games/imagine/docs/PLAN.md` §1, §3.2, §3.6;
- `games/imagine/content/weapons_batch1/export/README.md`;
- `kits/fps/src/{weapon.rs, command.rs, aim_assist.rs}`;
- `games/imagine/game/src/{data.rs, present.rs, hud.rs, sim.rs}`;
- `games/imagine/data/{tuning.ron, sounds.ron, modes.ron}`.

Docs in the same change:
- PLAN.md: M1.2 done (or what is left);
- `games/imagine/docs/` (weapon data format);
- `docs/ARCHITECTURE.md` §1 (kit row);
- the decision in `docs/ARCHITECTURE.md` §12;
- `docs/briefs/README.md` status `review`.

Small choices are yours. For naming, plumbing or where a function lives, pick the simplest option that keeps one path, say it in the report, and continue. Stop only when the goal itself conflicts.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
