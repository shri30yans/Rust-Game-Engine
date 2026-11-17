# menu-stage: a lit 3D showcase behind the menus, with the operator and the weapon (2026-11-12)

On the Android emulator (2400×1080, 2026-11-12) every menu screen (home, modes, lobby, results) is flat UI over a black background. The approved home mockup (`games/imagine/docs/design/mockups/home.png`) has the operator standing on a lit set, with the arena behind.

**Why it is missing.**
- A menu backdrop scene exists (`ImagineSettings::menu_backdrop`, the `menu-backdrop.png` golden on desktop), but it is not visible on the phone path. Find out why and say so in the report.
- There is no operator character: only the CC0 base body in `games/imagine/content/characters/man_base/source/`.

This is section 2 of `imagine-frontend-2`, pulled forward so it runs now, in parallel with the UI engine work.

## Design
1. **The set.** A corner of Kiln Yard at golden hour: warm low sun, light haze, the arena's buildings behind.
   - It is game content (a menu level in `games/imagine/data/`), lit by the engine's existing lighting.
   - It must look right on Low (phones) as well as High. `render-high-black` is fixing High right now; judge it on Low, and on High once that merges.
2. **The operator.** A Blender build script in `games/imagine/content/characters/operator/blender/` builds the operator over the base body, and never edits the source file:
   - armour plates, a helmet with a visor, a vest with a number plate, and team-colour panels, all as separate meshes;
   - our own design (D23: nothing from THE FINALS);
   - a GLB with a desktop LOD and a phone LOD (at most 15k triangles), plus a hero render;
   - cooked by `tools/cook`, like the weapon pack;
   - the team colour is a material parameter.
   Keep the source licence's AI-assisted disclosure in `games/imagine/docs/ASSETS.md`. If the rig or clips cannot be cooked or played by the current engine, use a static pose that is part of the design (an idle stance authored in Blender) and report the animation gap as an engine brief. Do not fake animation.
3. **The weapon.** The cooked tactical pistol on a slowly turning display beside the operator. If the pistol is not drawn on the phone path (on the emulator the HUD showed no viewmodel during a match), find out why and report it.
4. **Cameras per screen.**
   - Home frames the operator on the left third, as in the mockup.
   - Modes and lobby pull back to the arena.
   - Results frames the operator on a podium.
   - The camera has a slight idle drift.
   Transitions ease over 400 ms. Each camera is data in the game, not code per screen.
5. **Shown.** Frames at 2400×1080 on Low and 1920×1080 on High for home, lobby and results, with the current UI on top, and the same on the Android emulator. To use the emulator:
   - start it with `$HOME/.local/trove-android/sdk/emulator/emulator -avd imagine_phone -gpu host -no-snapshot -no-audio` in the background;
   - then `adb -s emulator-5554` with `scripts/build-android.sh imagine`, install, `am start`, and `screencap`.

Done means:
- items 1 to 5 complete end to end;
- a golden for the home stage at both sizes;
- the four gates pass;
- the APK builds.

Read first:
- `AGENTS.md`;
- `games/imagine/docs/PLAN.md` §4;
- `games/imagine/docs/ASSETS.md`;
- `docs/briefs/imagine-frontend-2.md` §2;
- `games/imagine/game/src/{present.rs,scene.rs,menus.rs}`;
- `games/imagine/content/weapons_batch1/blender/common/` (the build-script pattern);
- `tools/cook`.

Do not touch:
- `engine/` and `kits/` (stop and report an engine gap instead);
- `games/imagine/ui/*.html` (the UI lane is rebuilding them);
- `games/imagine/content/weapons_batch1/` (the owner is editing it).

Docs in the same change:
- `games/imagine/docs/` (PLAN §4 and ASSETS);
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
