# movement-states: sprint, crouch, slide and mantle as one movement state machine (PLAN M1.1)

Goal: IMAGINE's player and bots move like a modern arena shooter: sprint, crouch, slide out of a sprint, and mantle onto ledges up to 1.6 m, every number in `tuning.ron`, every state change an event. Done means: in the running game, Shift sprints, C (or Left Ctrl) crouches, crouch while sprinting slides, Space at a ledge mantles; the capsule shrinks when crouched and refuses to stand under a low ceiling; all of it is deterministic (same commands → same state hash), and the four gates pass.

Read first: `AGENTS.md`; `games/imagine/docs/PLAN.md` §3.1 (tick order), §3.6 (movement), §3.9 (data rules); `docs/README.md` §7 D53 (data vs code), D55 (follow Unreal); `kits/fps/src/{controller,command,lib}.rs`; `engine/physics/src/{character,world,collider}.rs`; `games/imagine/game/src/{data,sim,present}.rs`, `games/imagine/data/tuning.ron`.

## Design

Follow Unreal's `UCharacterMovementComponent` (D55): one movement mode per character, crouch as a capsule-height change checked by an overlap test before standing (`CanUnCrouch`), mantle as a short scripted move along a curve with collision checked once at the start (as Lyra and ALS do).

**1. One state enum in `kits/fps`** (`src/movement.rs`, used by `Motor`):
```rust
pub enum MoveState {
    Walk,
    Sprint,
    Crouch,
    Slide { ticks_left: u16 },
    Air,
    Mantle { from: Vec3, to: Vec3, tick: u16, ticks: u16 },
}
```
Illegal combinations do not exist (no "crouching and sprinting" flags). Transitions, in one `fn next_state(...)` with a table test for each row:
- Walk ↔ Sprint: sprint held, on ground, moving forward (axis y > 0.5); stop sprinting on fire, on reload, or when the axis drops.
- Walk/Sprint → Crouch: crouch level is true. Crouch → Walk: crouch is false **and** the standing capsule fits (overlap test excluding the own body); otherwise remain crouched until it fits.
- Sprint → Slide: crouch changes to true while sprinting at horizontal speed ≥ `slide_min_speed`; gives `slide_boost` once, low friction (`slide_friction`), lasts at most `slide_ticks`; ends to Crouch if crouch remains true or standing does not fit, otherwise Walk. Jump cancels into Air keeping speed. A slide down a slope keeps accelerating by gravity along the slope.
- Any ground state → Air on jump or when ground is lost (coyote time stays as today).
- Air/ground → Mantle: jump pressed, a wall ahead within `mantle_reach`, a ledge top found between `mantle_min_height` and `mantle_max_height` (1.6 m) above the feet, the standing capsule fits on top, and the surface is walkable. Detection is two shape casts with the existing query API (forward at chest height, then down from above the ledge). The move lasts `mantle_ticks`, eased, collision-free inside the clearance just proven; at the end the state is Walk with zero vertical speed.
- Each change writes a `MovementEvent { actor, from, to }` into a per-tick `MovementLog` resource, cleared at the start of the tick like the existing `DamageLog`.

**2. Capsule height.** Crouch and slide use the crouched capsule (`crouch_height`). Add to `engine-physics` the one API needed: `PhysicsWorld::set_collider_shape(collider, shape)` (or the equivalent on the character's body), keeping the feet in place. Note: another Codex run (`physics-ramp-penetration`) is changing `engine/physics/src/character.rs` now: keep physics edits to this one function in `world.rs` or `collider.rs` and do not change `character.rs` behaviour.

**3. Commands.** `MoveCommand` gains `sprint: bool` and `crouch: bool`, both levels ("wants to sprint", "wants to crouch", as Unreal's `bWantsToCrouch`); toggle versus hold is decided by the game's input layer, which writes the level every tick. Bots set them through the same command (AI cannot cheat, PLAN §3.8): a bot sprints when its route target is far and it has no target, and never crouches or mantles yet (that waits for the navmesh).

**4. Data.** `tuning.ron` `movement` gains: `sprint_speed`, `crouch_speed`, `crouch_height`, `stand_height`, `slide_min_speed`, `slide_boost`, `slide_friction`, `slide_ms`, `mantle_reach_m`, `mantle_min_height_m`, `mantle_max_height_m`, `mantle_ms`, `camera_crouch_ms` (eye-height blend), validated into typed values (ranges, `crouch_height < stand_height`, etc.), hot-reloadable through the same `Tuning` path as today.

**5. IMAGINE.** Keys: Left Shift sprint (held), C and Left Ctrl flip the presenter's crouch latch, Space jump/mantle. The presenter writes the crouch level into every command tick. Touch: sprint when the move stick is pushed past 90 % forward (no button), a crouch button beside jump flips the same latch (follow `games/imagine/docs/design/TOUCH_SPEC.md`). Bots always write crouch false. The camera eye height blends between stand and crouch over `camera_crouch_ms`; a slide tilts the camera 3° (presentation only). The HUD shows nothing new.

**Tests:**
- Table test of every transition row above, including the refusals (no stand under a 1.4 m ceiling, no slide below min speed, no mantle at 1.7 m, no mantle onto a 60° slope).
- Mantle succeeds onto 0.8 m, 1.2 m and 1.6 m boxes; ends standing on top, never inside geometry.
- Property: for random command sequences in a box room with ledges and a low ceiling, the capsule never ends inside static geometry deeper than the skin width, and the state never says Crouch/Slide while the collider is the standing size or the reverse.
- Determinism: two runs of 600 ticks with the same scripted commands give the same state hash per tick.
- `tuning_reload` covers a changed movement section applied at a tick boundary and an invalid one rejected.

**Do not touch:** `engine/netcode`, `engine/assets`, `tools/cook`, `engine/render` (other runs are editing them). `engine/physics` only for the one shape API.

**Docs in the same change:** `games/imagine/docs/PLAN.md` M1.1 status; `docs/ARCHITECTURE.md` §1 kits row; `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
