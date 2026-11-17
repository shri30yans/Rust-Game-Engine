# sandbox-movement: the engine-only sandbox game and its walk / jump / collide scene (build step 2)

Goal: create `games/sandbox/game` (package `sandbox`, library plus desktop binary) with `scripts/new-game.sh`, and one scene that proves physics and character movement: a floor, walls, a stair step, a ramp and a pile of dynamic crates, with a first-person character you can walk, jump and bump into them with. Done means: the scene runs headless under scripted input with exact assertions (in every gate run), the desktop binary opens a window and is playable, and all four gates pass. This is the "playable example after the base" the owner asked for: no fallbacks, no hacks, no filtering things out to satisfy a test.

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §0-1, `docs/README.md` §7 D24 and D36, `engine/app/src/{app,game,client,config}.rs`, `engine/world/src/{world,schedule,resource}.rs`, `engine/physics/src/{lib,world,body,collider,character}.rs` and `engine/physics/tests/api.rs` (usage examples), `engine/platform/src/input.rs`, `engine/render/src/gfx/{config,mod}.rs`, `engine/render/src/mesh.rs`.

## Layout and rules
```
games/sandbox/docs/README.md     what the sandbox is, how to run it, the scene's checklist
games/sandbox/game/Cargo.toml    package "sandbox"; [lib] and [[bin]] sandbox; dependencies: engine-app, engine-world, engine-physics, engine-core, platform, render, glam, thiserror, serde (derive) only
games/sandbox/game/src/lib.rs    module list and re-exports
games/sandbox/game/src/scene.rs  the scene as data (a list of static boxes, a ramp, crates) and `build_scene(world)`
games/sandbox/game/src/sim.rs    resources and systems
games/sandbox/game/src/present.rs the `Presenter`
games/sandbox/game/src/main.rs   desktop client
games/sandbox/game/tests/*.rs    scripted headless tests
```
Add `games/sandbox/game` to the workspace `members`. `layers.json` already has the `games/*/game` zone; the crate may only use engine crates. No kits. The sandbox never reads a file by a fixed path (the scene is Rust data for now).

`scripts/new-game.sh NAME`: copies `games/sandbox` to `games/NAME`, renames the package and crate paths, adds the member to the workspace `Cargo.toml`, and refuses (non-zero exit, message) if `games/NAME` exists or NAME is not `[a-z][a-z0-9_]*`. `set -euo pipefail`, no network. Test it in a temp copy of the repo layout, not in the real tree (a bash test script `scripts/tests/new-game.sh` that creates a temp directory, runs the script against it with an env var `REPO_ROOT`, and checks the result; the main script honours `REPO_ROOT` and defaults to the repo root).

## Simulation (`sim.rs`)
Systems are plain `fn(&mut World, &mut SystemContext<'_>) -> Result<(), WorldError>`; state lives in world resources (D36).
- `SandboxPhysics`: use `PhysicsWorld` directly as a resource (it implements `Resource`).
- `Player` resource (own `impl Resource`): the `CharacterController`, `CharacterState`, the player's `NetId`, the player's kinematic `BodyId`, and the pending `PlayerCommand { strafe: Axis, forward: Axis, jump: bool, view: ViewAngles }` (`Axis` is an enum of negative, zero, positive; `ViewAngles` is the engine's validated view, see D37). `hash_into` writes the character position, vertical velocity and grounded flag.
- `Bodies` resource: `Vec<(NetId, BodyId)>` of every dynamic and static body in the scene, in spawn order (no HashMap, deterministic).
- Systems in order: `character_move` (set the yaw from the command's `view`, build `CharacterInput` from the command rotated by that yaw, call `move_character`, apply with `set_next_kinematic_pose`, update `CharacterState`), `physics_step` (`PhysicsWorld::step`), `sync_transforms` (for each `(NetId, BodyId)` write the body pose into the entity `Transform` using `world.get_mut::<Transform>`, any error propagates; also write the player's `Transform` from `CharacterState`).
- Constants live in `scene.rs` or `sim.rs` as named `const`s with units in the name: `WALK_SPEED_MPS`, `EYE_HEIGHT_M`, gravity and jump come from `CharacterConfig`.
- Tick rate 60 Hz. `SandboxGame` implements `Game` (`GameInfo { name: "sandbox", protocol_id: 1, protocol_version: 1 }`): `setup` registers nothing replicated that it does not need, builds the scene, inserts the resources and adds the three systems.
- A pub fn `set_command(world: &mut World, command: PlayerCommand) -> Result<(), WorldError>` is the only way input enters the simulation.

## Scene (`scene.rs`)
Plain data: a 40 x 40 m floor (static box), four boundary walls, one 0.25 m stair step, one 20 degree ramp (a rotated static box), a 3 x 3 stack of 0.5 m dynamic crates (mass from the collider) placed so they settle in the first second, and the player spawning at a fixed point. Every entity gets a `NetId` via `World::spawn` and its `Transform` (scale = box size so the presenter can draw it).

## Presenter and binary (`present.rs`, `main.rs`)
`SandboxPresenter` implements `app::Presenter`: `setup` uploads one unit cube mesh; `apply_input` reads WASD / arrow keys (`Input::held(Key)`) and Space (check `input.rs` for the right Key names) into a `PlayerCommand` with the `look` it receives and calls `set_command`; `grab_mouse` returns true; `camera` puts the eye at the blended player position + `EYE_HEIGHT_M` with `look.yaw()` and `look.pitch()`, and returns an error when the player is missing from the frame; `draw` draws every transform in the frame as a tinted cube (static grey, crates orange, ramp and step lighter), ground included. `main.rs` builds `AppConfig` (60 Hz, seed 1, catch-up 8, `max_entities` 256), the device profile (`DeviceProfile::detect` or its real name in `engine/platform/src/profile.rs`), `ClientApp::new`, and calls `run_client("Sandbox", (1280, 720), &mut client)`, printing the error and exiting non-zero on failure. Mouse look comes from `ClientApp`: each frame it sets `input.want_grab` from `Presenter::grab_mouse` and turns `ViewAngles` from mouse motion while the window is grabbed.

## Tests (all in `games/sandbox/game/tests/`, headless, no GPU, using `app::App` and `run_headless` / `App::step`)
1. `determinism.rs`: two runs of the same 600-tick scripted input give identical `state_hash` every tick (compare the vectors); a different script gives a different final hash.
2. `movement.rs` (each is a separate `#[test]` with a scripted command stream and exact numeric assertions; tolerances stated and small):
   - falls and lands: after 60 ticks the player is grounded and its feet are on the floor within the skin width;
   - walking forward for 60 ticks at full wish moves about `WALK_SPEED_MPS` metres (within 2 %);
   - jump: the apex height matches `jump_speed² / (2 * gravity)` within 5 % and the player lands again, grounded;
   - blocked by a wall: walking into a wall for 120 ticks never ends inside it and stops at the wall minus the capsule radius;
   - step: walking into the 0.25 m step climbs it; walking into a 1 m wall does not;
   - ramp: walking up the 20 degree ramp ends higher than it started; the slope limit stops a steep (50 degree) test ramp spawned only in this test;
   - crates: walking into the crate stack never intersects a crate (use the physics `intersects_shape` with the capsule inset by the skin width, as `engine/physics/tests/api.rs` does) and the crates have settled (speed below a stated threshold) by tick 120.
3. `scene.rs` unit tests: every box in the scene has positive size, spawn point is inside the floor and not inside any other box.
4. A bash test for `new-game.sh` (above) run by a Rust integration test is NOT required; just make the script's own test script pass and mention how to run it in `games/sandbox/docs/README.md`.
5. `cargo run -p sandbox` is run by you once for a few seconds to confirm the window opens without an error (state the result in your final message; if no display is available say so).

If a test shows the physics or character controller misbehaving (jitter, falling through, climbing walls), do NOT adjust a test or hide the symptom: report it with the numbers in your final message.

## Docs
`docs/README.md` §2 Sandbox row to "built (movement scene)"; `docs/ARCHITECTURE.md` §1 row 7 and the "Making a new game" line (script now exists); `games/sandbox/docs/README.md` (new); `docs/briefs/README.md` row for this brief set to `review`.

## Do not
Touch engine crates (if an engine API is missing or wrong, stop and describe it in your final message). No `#[allow]` in non-test code, no `unwrap`, `expect`, `unwrap_or*`, `.ok()`, `let _ =`, panicking indexing, truncating `as` casts, `Rc`/`RefCell`/`Mutex`, `unsafe`, `static mut`, wall-clock or thread RNG in simulation code.

## Gates
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
