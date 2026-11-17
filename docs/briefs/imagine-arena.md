# imagine-arena: the first playable IMAGINE level, built on `kit-fps` (build step 3, `games/imagine/game`)

Goal: a playable first-person arena game, `games/imagine/game` (package `imagine`), on desktop and as an Android APK. You spawn in a greybox arena ("Kiln Yard"), run, jump, shoot a rifle, and fight bots that chase you and shoot back; you die, respawn, and keep a kill counter. Done means: scripted headless tests prove the game loop (spawn, kill, die, respawn, score), all gates pass, `cargo run -p imagine` opens a playable game on the desktop, and `scripts/build-android.sh imagine` produces a signed APK. Nothing is claimed about feel on a phone: say plainly in the docs that on-device play is unverified.

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §0-1, `docs/README.md` §7 D24, D36, D37, D38, D40, `kits/fps/src/lib.rs` and its `tests/common/mod.rs` and `tests/duel.rs` (the working example of wiring the kit), `games/sandbox/game/src/*` (client wiring, scene-as-data, presenter, Android entry; copy the PATTERNS, not the movement code, the kit replaces it), `games/sandbox/docs/README.md`, `engine/render/src/{mesh,ui}.rs` and `gfx/config.rs` (`Instance`, `Mat`, `MeshBuilder`, `Ui`), `engine/platform/src/input.rs`, `engine/app/src/client.rs`, `docs/RENDERING.md` §7 (first person), `games/imagine/docs/README.md`.

## Setup
Run `scripts/new-game.sh imagine`, then replace what it generated: the crate keeps the sandbox's layout (`src/{lib,main,android,scene,sim,present}.rs` plus new modules below), depends on `kit-fps`, and uses the kit for movement, health, weapons and bots; it does NOT depend on `sandbox`. The Android manifest goes in `games/imagine/android/AndroidManifest.xml` (package `com.imagine.game`, label "IMAGINE", `lib_name` `imagine`). `scripts/build-android.sh` takes an optional game name argument (default `sandbox`) and derives package, library name and manifest path from it; keep the sandbox build working and tested. Update `tools/check-layers/layers.json` only if needed. All numbers live in ONE module `src/data.rs` (consts and small validated structs): rifle, controller tuning, bot tuning, health, respawn delays, bot count (default 6), arena dimensions.

## Arena (`src/scene.rs`, as data like the sandbox scene)
About 60 m x 60 m, a perimeter wall (4 m high, no gaps), floor, and a layout that makes fights interesting:
- a central two-storey building (6 m x 14 m): a ground-floor room with two doorways, an upper walkway reached by a stair (steps of 0.25 m) on one side and by a 1.0 m ledge you must JUMP onto on the other (the jump from the kit's tuning must clear it, assert that in a test), a low parapet around the upper walkway;
- four corner cover clusters made of crates (1 m and 2 m boxes), a long low wall (1.2 m) across the east half for crouch-free cover, a ramp (18 degrees) up to a 1.5 m raised platform in the west, a shallow trench (cover without fall damage) in the south;
- at least 12 bot waypoints forming a loop through open ground, the building doorways and the ramp, no waypoint inside geometry (a test places a capsule at every waypoint and asserts no overlap using `intersects_shape`), 4 player spawn points and 6 bot spawn points spread apart (a test asserts pairwise distance >= 8 m and no overlap with geometry, and line of sight between at most half of spawn pairs so spawns are not trivially exposed).
Every collider is built from one list of `SceneBody` data used for both physics and drawing (as the sandbox does). Visual look: distinct tints per surface kind (floor, walls, crates, building, ramp, platform) using the render crate's existing procedural patterns; sky and lights from the renderer defaults; do not tune beyond that in this brief.

## Simulation (`src/sim.rs`, `src/rules.rs`)
- Player and bots are entities with the kit's components (`Locomotion`, `Health`, `Team`, `Weapon`; bots also `Bot`), one kinematic capsule body each, spawned from data. Player team 0, bots team 1; the kit's friendly-fire off.
- Rifle: damage 20, 600 RPM (fire interval derived from the tick rate in data, not a literal tick count), magazine 30, reload 2.0 s, spread cone 1.5 degrees growing 0.4 degrees per shot and recovering at 4 degrees per second, range 120 m. Health 100. Bots: reaction 0.25 s, aim speed, aim error starting at 6 degrees shrinking to 1.5, sight 60 m, FOV 110 degrees. Expressed in seconds/degrees in `data.rs` and converted once with checked arithmetic.
- Game rules as a small deterministic state machine in `rules.rs`, run by a system after `fps.damage`: when an entity's `Health` reaches 0 it is "down"; the player respawns after 3 s at a spawn point chosen as the one farthest from living bots (ties broken by index); a downed bot respawns after 5 s at a bot spawn point farthest from the player; respawn restores health, ammo, and clears the body velocity and places the physics body (`set_body_pose`). A `Score { kills, deaths }` resource (hashed into the world per D36) counts the player's kills and deaths. Killing and dying are only ever counted once per death.
- The player's command comes from input only through one function (like `set_command`) and is a kit `MoveCommand`; a downed player's command is ignored.
- `HudEvents` resource: copies hit markers (the player hit something, the player was hit with the direction of the shooter) and kills from the kit's `DamageLog` after each damage pass with short decay timers so the HUD can draw them even though the kit clears the log each tick.

## Presenter and look (`src/present.rs`, `src/hud.rs`)
- Keyboard and mouse: WASD, Space jump (press edge and held both fed to `MoveCommand`), left mouse fire (already `Key::Fire`), R reload; mouse look from the engine. Touch (engine, see below): left stick moves, right-half drag looks, a SECOND right-half finger fires, a quick tap on the left half jumps, a quick tap on the right half reloads.
- Drawing: the arena, bots as a capsule body plus a head sphere tinted by team and blended between ticks via `RenderFrame::blended`, a visible tracer (a thin emissive stretched box for 3 ticks from the muzzle to the hit point) and a short muzzle flash light on the player's shots, bot shots too. A first-person weapon model (box-built rifle) if `Gfx` exposes the viewmodel path (`vm_proj` exists in the globals; read `docs/RENDERING.md` §7); if it does not, skip it and say so in the docs (do not fake it with world geometry).
- HUD with the render crate's `Ui`: crosshair that widens with weapon spread, health bar, ammo `mag / reserve` with "RELOADING" while reloading, hit marker flash and kill flash, damage direction indicators, kill/death counter, a respawn countdown screen when downed, and fps on a debug key (F3). Layout in logical units scaled to the window and safe for a 20:9 phone.

## Engine additions (additive only, each with tests)
`engine/platform/src/input.rs`: `Input::jump_tap() -> bool` (a left-half touch that ended within 12 frames of starting and moved less than 12 px, true for one frame) and `Input::reload_tap() -> bool` (same on the right half; the quick touch must not leave a look delta behind larger than its movement). Tests for: tap vs drag vs hold, left vs right, one-frame lifetime, a tap does not fire the stick or fire.

## Tests (`games/imagine/game/tests/`)
1. `scene.rs`: waypoint and spawn validity, the 1.0 m ledge is jumpable with the data's controller tuning and the step stair is walkable (physics-backed), the arena is closed (rays from the centre in 16 directions hit a wall).
2. `gameplay.rs` (headless, scripted, fixed seed): (a) the player aims at a stationary bot and kills it; `Score.kills == 1`; the bot respawns after 5 s at its spawn rule; (b) a bot with line of sight kills a stationary player within a bounded time, the player respawns after 3 s, `Score.deaths == 1`; (c) with the full 6 bots running for 60 simulated seconds nothing panics or errors, health stays within bounds, and `state_hash` is identical across two runs of the same seed and differs for another seed; (d) the player cannot move while downed.
3. `no_alloc.rs`: one `#[test]` using the project's counting allocator: the game's own systems (rules, HudEvents, transform sync) allocate nothing after warm-up; kit combat is already measured; do NOT claim physics is allocation-free (D40 records it is not).
4. Unit tests for `data.rs` conversions and `rules.rs` (respawn selection ties, score once per death), proptest where an invariant exists.
5. Android: the existing host-side `scripts/tests` style test for `scripts/new-game.sh` still passes; extend with a check for the new game-name argument of `build-android.sh` using a dry-run or argument-validation path (no NDK needed in CI).

## Docs
`games/imagine/docs/README.md` (status: playable arena, how to run, controls, tuning in `data.rs`, what is unverified), `docs/README.md` §2 games row, `docs/ARCHITECTURE.md` §1 (games), `docs/briefs/README.md` row `imagine-arena` -> `review`, D41 only if you make a decision another crate must follow.

## Do not
Put game words (rifle, Kiln Yard, bot tuning) in `kits/` or `engine/`. Depend on the sandbox. Add dependencies. Duplicate kit logic (no second controller, no second weapon system). `#[allow]` in non-test code (except the existing Android module pattern), `unwrap`, `expect`, `unwrap_or*`, `.ok()`, `let _ =`, panicking indexing, truncating `as` casts, stubs, TODO, dead code, `#[ignore]`d or weakened tests, sleeps in tests, a silent fallback when a needed engine feature is missing.

## Gates
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
cargo clippy --target aarch64-linux-android -p imagine --lib -- -D warnings   # with the NDK linker env from scripts/build-android.sh
bash scripts/build-android.sh imagine
```
Report the APK path, size, `apksigner verify` and the badging summary.
