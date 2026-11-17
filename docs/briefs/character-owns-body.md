# character-owns-body: a character controller always owns and ignores its own capsule (engine-physics API fix; audit J4 follow-up)

Goal: make it impossible for a character to collide with its own collider. Measured on 2026-11-11: the J4 bench called `move_character` for characters that carry a kinematic capsule, so each sweep hit its own capsule; a step cost about 10 ms, and with `move_character_excluding` 0.43 ms (p50 0.23 ms). Done means: there is one move function and it always excludes the character's own body; the sandbox and every bench and test use it; J4's numbers in `docs/research/2026-11-11-engine-audit.md` are re-measured with it.

Read first: `AGENTS.md`; `docs/README.md` §7 D55 (follow Unreal); `engine/physics/src/{character,world,body,collider}.rs`; `engine/physics/{tests,benches}/*`; `kits/fps/src/controller.rs` (already excludes its body); `games/sandbox/game/src/sim.rs`.

## Design
- Follow Unreal's `UCharacterMovementComponent`, which always moves and ignores its own `UCapsuleComponent`: `CharacterController` is created **with its body**. `CharacterController::spawn(world: &mut PhysicsWorld, config: CharacterConfig, position: Vec3) -> Result<(Self, CharacterState), PhysicsError>` creates the kinematic body and its capsule collider and keeps the `BodyId`; `body()` returns it.
- One move function: `move_character(&self, state, input, world) -> CharacterMovement`, always filtering out `self.body`. Delete `move_character_excluding` and the old unfiltered behaviour. Writing the new pose to the kinematic body stays where it is today, or moves into one `apply(&self, world, movement)` method if that makes the call sites simpler; pick one and use it everywhere.
- Update all callers: `kits/fps/src/controller.rs`, `games/sandbox/game/src/sim.rs`, `engine/physics/benches/{physics,determinism}.rs`, `engine/physics/tests/*`. Expected hashes in tests may change only where a character previously hit its own collider; explain each changed value in the report.
- Re-run the J4 bench (modes A and C, twice each) when the machine is otherwise idle if possible, and replace the J4 table and recommendation in `docs/research/2026-11-11-engine-audit.md` with the new numbers. Note the load average next to the numbers.

Tests: a test that spawns a character, moves it on flat ground for 600 ticks and asserts it never reports contact with its own collider (or, simpler, that its movement equals the movement of the same capsule as a bare shape query with no body); existing tests pass.

When a detail is not specified: pick the simplest correct design that keeps one code path, follow Unreal where in doubt (D55), state it in the report, and continue.

Do not touch: `engine/app`, `engine/render`, `engine/ui`, `engine/netcode`, `engine/assets`, `tools/`, `games/imagine` (other briefs are editing them).

Docs in the same change: `docs/research/2026-11-11-engine-audit.md` J4 and M5, `docs/ARCHITECTURE.md` §1 physics row, `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
