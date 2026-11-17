# match-events: one event log per tick, and movers players can ride (PLAN M0.1 + M1.4) (2026-11-12)

**What is wrong.**
- Systems tell each other what happened through private logs that each one clears itself. The kit clears its damage log inside `fps.damage` and its movement log in `fps.movement_log.clear`. `feedback.capture` must therefore run between `fps.weapon` and `fps.damage` (see the header of `games/imagine/game/src/sim.rs`). Moving one system breaks feedback, sound and the HUD without a compile error.
- Layout cues move walls in the simulation (`arena.layouts`, `activate_layout_cue` in `games/imagine/game/src/scene.rs`), but a player standing on a moving platform does not ride it, a wall moving into a player does not push them out, and nothing can be lethal. That is the core of the Director (PLAN §3.4, §3.5).

**Why it was possible.** There is no shared event path in the tick, so each system invented its own. Movers were added as poses only, with no contract with character movement.

## How Unreal does it
- **Events.** Gameplay systems broadcast typed messages (Lyra's `GameplayMessageSubsystem`; GAS gameplay events). Senders do not know receivers, and nobody clears another system's state.
- **Movers.** `CharacterMovementComponent` has a movement base: a character standing on a moving component inherits its motion every tick. A kinematic mover that sweeps into a character pushes it out of the way (`MoveComponent` with push). A mover that crushes asks the game what to do (damage, stop or reverse).

## Design
1. **One typed event log per tick, owned by the engine schedule.**
   - In `engine-world`: `Events<E>`, a resource type a game or kit registers once (`world.add_events::<E>(capacity)`).
   - The schedule clears every registered log once, before the first system of the tick. No system clears a log, and the clear-log systems are deleted.
   - Writers push, readers iterate. Reading never consumes, so the order between readers does not matter.
   - The capacity is sized at registration from data (actors × events per actor per tick). The steady tick allocates nothing (prove it with the counting allocator).
2. **The kit's events move onto it.** `kits/fps` defines its event types (shot, hit, damage, kill, footstep, landed, and whatever its private logs carry today). `fps.weapon`, `fps.damage` and `fps.locomotion` write them; IMAGINE's feedback, hearing, rules and audio read them.
   - Delete the kit's private damage and movement logs, `clear_movement_log_system`, and the ordering comment in `sim.rs`.
   - The schedule order is then only what the data flow needs (PLAN §3.1), and a test proves that feedback output is the same whichever order the readers run in.
3. **Movers are a kit contract, not an IMAGINE special case.** The tick order is PLAN §3.1 exactly: movers set their pose (step 3), characters move riding their mover (step 4), the physics step (step 5), then overlaps are resolved (step 6). The rules are PLAN §3.5. In `kits/fps`:
   - **Riding (step 4).** A character whose ground contact is a mover body takes that mover's velocity this tick before its own move (Unreal's movement base). Jumping off keeps that velocity, so a player can launch from a moving platform.
   - **Push-out (step 6).** After the physics step, a character overlapping a mover is moved out along the mover's motion with a physics shape cast. If that way is blocked, it goes to the nearest free spot. The character never ends inside geometry; the existing embedded-move refusal in `engine-physics` is the check. Being pushed is never damage unless the mover is lethal.
   - **Lethal flag.** A mover marked lethal in the level data kills a character it overlaps that has no free spot (crushed), and writes a kill event with the mover as the cause.
   - IMAGINE's layout transition drives mover bodies through this contract. The lethal flag and the mover list come from the level file and are checked at load.
4. **Warnings.** A layout cue writes a `DirectorWarning { cue, starts_in }` event a data-set time before movers start, so the HUD and sound (later briefs) have one source. Only the event is in scope here, not the HUD piece.

Tests:
- the event log: cleared once per tick, readable by any number of readers, no allocation at steady state, capacity from data;
- feedback, hearing and rules give the same output under every order of the reader systems;
- riding: a character standing on a platform moving 3 m sideways ends 3 m over with no slip, at 60 ticks/s;
- push-out: a wall moving into a standing character pushes it and never leaves it embedded; property test over random mover speeds and directions;
- riding: jumping off a moving platform keeps its velocity;
- push-out when blocked: the character goes to the nearest free spot;
- lethal: a lethal crusher with no free spot kills and writes a kill event with the mover as cause; a non-lethal mover never damages;
- a cue writes its warning event the set time before its movers start;
- IMAGINE's existing match tests and goldens pass unchanged.

Decision: written into `docs/ARCHITECTURE.md` §12 (one engine-owned event log per tick; movers as a kit contract: riding, push-out, lethal, as Unreal's movement base).

Runs beside `weapons-data` (which also edits `kits/fps` and IMAGINE's `sim.rs`) and `asset-source` (IMAGINE start-up and content loading). Keep edits to `sim.rs` to the schedule and the event registration, so the reviewer can integrate them. Do not touch `engine/render`, `engine/ui`, `engine/assets`, or `kits/fps/src/weapon.rs` beyond switching its log writes to events.

Read first:
- `AGENTS.md`;
- `games/imagine/docs/PLAN.md` §3.1, §3.4, §3.5;
- `games/imagine/game/src/{sim.rs, scene.rs, feedback.rs, hearing.rs, rules.rs}`;
- `kits/fps/src/{health.rs, movement.rs, controller/}`;
- `engine/world/src/` (schedule and resources).

Docs in the same change:
- PLAN.md: mark M0.1 done and M1.4 done (or say what is left);
- `docs/ARCHITECTURE.md` §1 (world and kit rows);
- the decision in `docs/ARCHITECTURE.md` §12;
- `docs/briefs/README.md` status `review`.

Small choices are yours. For naming, plumbing or where a function lives, pick the simplest option that keeps one path, say it in the report, and continue. Stop only when the goal itself conflicts.

## Gates
scripts/gate.sh
