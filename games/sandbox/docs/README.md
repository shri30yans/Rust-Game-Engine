# Sandbox

An engine-only game: one scene for checking the character and the physics. It uses only engine
crates (no kit, no other game), so it is also the template for new games.

## Run it

    cargo run -p cook -- sandbox
    cargo run -p sandbox

Controls: W/S or Up/Down walk forward and back, A/D strafe, Space (or the fire key) jumps, F3
toggles the frame telemetry panel. On Android the touch controls are below; the telemetry
panel starts shown and a tap on its top-left corner toggles it. The mouse turns the view: `SandboxPresenter::grab_mouse` asks the engine to grab the window, and the
engine's mouse look produces the `ViewAngles` that the presenter passes into each `PlayerCommand`.
The simulation faces the character along that view, and the camera uses the same view.

## Android build

    scripts/build-android.sh

Produces `target/android/sandbox-debug.apk`: a signed arm64 APK, full screen, landscape, minimum
Android 8.0 (API 26), target API 35. The native library is `libsandbox.so` (the crate's `cdylib`),
and `android_main` in `game/src/android.rs` is its entry point. The script reads `ANDROID_HOME`
(default `~/.local/trove-android/sdk`) and needs the platform 36 jar, build-tools, an NDK, Java
(`java`, `keytool`) and the `aarch64-linux-android` Rust target. It links with the NDK clang
directly; `cargo-ndk` is not used. Re-running it is safe.

Install and start it on a phone with USB debugging on:

    adb install -r target/android/sandbox-debug.apk

Touch controls: the left half of the screen is a floating stick (drag from anywhere on the left
half; it walks and strafes). The right half turns the view by drag. A JUMP button on the right
jumps while held. The button is drawn as a plain ring, because the sandbox has no HUD or font. The
stick reads as centred inside 30 % of full deflection. The presenter returns the button layout from
`Presenter::touch_buttons`, and the client installs it into the input on the first frame.

Not verified: the APK is built, signed and checked with `apksigner verify` and `aapt2 dump
badging`, and the Android code is cross-compiled, but on-device behaviour has NOT been verified.
No device or emulator was available when this was built, so it has not been run.

## Test it

    cargo run -p cook -- sandbox
    cargo test -p sandbox

The scripted headless tests are in `game/tests/`: `movement.rs` (the character and the scene) and
`determinism.rs` (the state hash). No test is ignored.

Each command in the scripted tests carries a view: walking toward +X uses a fixed yaw
(`FACING_PLUS_X`, built through `ViewAngles::new`).

## Scene checklist

The scene is the level file `game/levels/sandbox.ron` . The numbers below describe that file.

- Floor: 40 m square, top at y = 0.
- Walls: four, 4 m high, on the floor's edge.
- Stair step: 0.25 m high, 2 m wide, at x = -10.
- Ramp at 20 degrees, starting at x = 3, z = -8, rising towards +X.
- Steep ramp (test scene): 50 degrees, starting at x = -15, z = 15. Above the 45 degree limit.
- Crate stack: 18 dynamic 0.5 m cubes, 3 x 3 x 2, at x = 12, z = 6.

## Live editing (debug builds)

In a debug desktop build the level is read from `game/levels/sandbox.ron` once at startup, and the
presenter checks that file for a change four times a second. Save an edit and the scene reloads. A
file that fails to parse, fails to validate or fails to spawn keeps the running scene, and the first
line of the error shows in a red bar at the top of the window. A good edit replaces the scene all at
once and clears the bar. Release builds and Android use the level compiled into the binary and never
read the file. The gravity in the file must equal the game's constant, so a level that disagrees is
rejected.

## Character and physics numbers

| Quantity | Value |
| --- | --- |
| Tick | 60 Hz fixed |
| Capsule | half height 0.5 m, radius 0.3 m (feet 0.8 m below the centre) |
| Walk speed | 4.0 m/s |
| Jump speed | 5.0 m/s (apex 1.274 m from v^2 / 2g) |
| Gravity | 9.81 m/s^2 |
| Skin width | 0.01 m |
| Step height / min width | 0.3 m / 0.2 m |
| Slope limit | 45 degrees |
| Ground snap | 0.15 m |
| Eye height | 1.5 m above the feet |

## Measured results (scripted scenarios, one run)

| Scenario | Measured | Expected / tolerance |
| --- | --- | --- |
| Fall from 3 m, rest | feet 0.020 m above floor | within 0.02 m |
| Walk 60 ticks at full forward | 3.998 m | 4.0 m +/- 2 % (-0.06 %) |
| Jump apex | 1.233 m | 1.274 m +/- 5 % (-3.2 %) |
| Walk into far wall | centre x = 19.680 m | 19.7 m +/- 0.05 m |
| Climb 0.25 m step | feet 0.270 m | 0.25 m +/- 0.03 m (0.02 m skin) |
| Walk 1 m wall | stops at x = -4.570 m, feet 0.010 m | stays on near side |
| Walk up 20 degree ramp (60 ticks) | feet 0.355 m at x = 3.863 m | feet above 0.2 m |
| Walk up 50 degree ramp (120 ticks) | highest feet 0.197 m, final x = -15.002 m | feet never above 0.2 m |
| Walk into crate stack (120 ticks) | capsule never overlaps the world, stops at x = 10.909 m | no overlap on any tick |
| Crate stack after 120 ticks | all 18 at rest, max speed 0 m/s | every speed < 0.05 m/s |

The 50 degree ramp numbers are after the fix in `engine-physics` (see below). Before it, the feet
rose to 0.570 m and the character reached x = -14.688 m. The margin under the 0.2 m threshold is
only 3 mm, because the character lifts to the ramp's toe and then stops.

## The 50 degree ramp fix

The character used to climb a 50 degree ramp although the slope limit is 45 degrees. Rapier's
`move_shape` slides the horizontal motion along a face it cannot walk on, and the slide projects part
of that motion upward. The autostep was ruled out by experiment: with autostep disabled the feet
rose to the same 0.5698 m. The fix is in `engine-physics/src/character.rs`: after a move that rises
onto a steep surface, the controller repeats the move with the horizontal part removed, and the
ground snap refuses a steep surface. Both checks use a ray down from the feet, with reach
`step height + ground snap + skin width`. A shape cast was tried first and broke the walk (the normal
inside the skin is unreliable), so it is not used for this check.

## Notes

- The crate stack reads exactly 0 m/s after settling; this is probably Rapier putting the bodies to
  sleep. Not checked against the engine's sleep settings.
- The walk tolerance is met with margin. The jump apex is 3.2 % below v^2 / 2g; the cause (discrete
  integration or the controller's skin) is not isolated.
- The "no overlap" check uses `PhysicsWorld::intersects_shape` with a `QueryFilter` excluding the player's body, so
  the character's own body cannot count as an overlap. The character has no collider of its own (the
  controller moves a kinematic body), so the exclusion changes no result today.

## Starting a new game from this one

    scripts/new-game.sh NAME

This copies `games/sandbox` to `games/NAME`, renames the crate, and adds it to the workspace. Run
`scripts/tests/new-game.sh` to check the generator itself. It runs against a scratch copy and
never touches the repository.
