# imagine-combat-feel: closer fights, readable targets, aim slowdown, damage falloff, cleaner touch controls, combat metrics (PLAN §3.6, §4)

Goal: combat on a phone is fast, readable and fair. Today enemies are tiny on screen, fights happen at long range and the touch controls are cluttered. Done means:
1. Typical fights happen at 8–25 m.
2. At the default 55° vertical FOV, an enemy fills at least 11% of the screen height at 15 m and 14% at 12 m. FOV is a player setting from 45° to 75°; projection remains Hor+.
3. Touch players get aim slowdown near enemies.
4. Every weapon has damage falloff.
5. The default touch layout is clean.
6. Enemies read clearly against the arena.
7. A headless bot match reports combat metrics before and after.
8. The four gates pass.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D23, D24, D42, D55;
- `games/imagine/docs/PLAN.md` §3.6, §3.8 and §4;
- `games/imagine/docs/LOOK.md`;
- `games/imagine/data/tuning.ron` and `games/imagine/data/arenas/kiln_yard.level.ron`;
- `games/imagine/game/src/{data,scene,sim,present,touch,hud,feedback,settings}.rs` and `games/imagine/ui/hud.html`;
- `kits/fps/src/{weapon,bot,controller}.rs`;
- `engine/app/src/{client,late_camera}.rs`.

## Measured starting point (2026-11-12)

- Character: 1.8 m tall (capsule half height 0.6 m, radius 0.3 m).
- Rifle `range_m`: 120.
- Bots: `sight_m` 60, `engage_m` 60.

A 1.8 m target at 60 m covers about 1.7° of view, roughly 3% of the screen height. At 15 m with the 55° default vertical FOV it covers about 6.3°, roughly 11.5%; at 12 m it covers about 7.8°, roughly 14.4%. The main cause is engagement distance, not character size.

## Design

Order of levers:
1. Bring fights closer.
2. Make targets read better.
3. Help aiming.

Do not scale characters up. Characters stay 1.8 m. Use IMAGINE's 55° vertical FOV default (45°–75° player setting) with Hor+ projection; do not change the engine camera default.

**1. Closer fights (game data).**
- Bots: `sight_m` 35, `engage_m` 25, and keep `stop_distance_m` 8. Bots will close in rather than snipe.
- Rework `kiln_yard.level.ron` (greybox shapes only; the Blender arena comes later through world-pipeline):
  - three connected lanes instead of one open yard;
  - cover every 5–10 m in exposed ground;
  - at least two routes between the spawn areas;
  - two small height changes (a raised walkway or platform reachable by ramp or mantle);
  - one clear landmark per lane (a distinct shape or colour).
  - Spawns keep an escape route and are out of the direct sight of the other team's spawn.
- **Sightline test** (`games/imagine/game/tests/sightlines.rs`):
  - sample walkable points on a 2 m grid at eye height;
  - cast 16 horizontal rays from each against the arena's static colliders, using the world layer filter once physics-shapes lands, else the current query;
  - assert that at least 95% of rays stop within 40 m and none exceeds 60 m;
  - print the median free distance;
  - assert no point in a spawn area can see the other spawn area.

**2. Readable targets (game side only; do not touch `engine/render`).**
- Characters get a team-coloured emissive accent band (chest and head) through their procedural `Look`. Enemies use a saturated warm accent, allies a cool one. Keep the arena palette in LOOK.md lower in saturation than the accents.
- Write the rule into LOOK.md: character accents are the most saturated colour on screen.
- **Screen-size test:** a pure function `target_screen_fraction(height_m, distance_m, vfov_rad)` and a test using tuning's camera FOV and character height, asserting at least 11% at 15 m and 14% at 12 m.

**3. Damage falloff (kit-fps, game-agnostic).**
- `DamageFalloff { full_until_m, min_from_m, min_scale }` in `kits/fps/src/weapon.rs`, validated:
  - `0 < full_until_m < min_from_m <= max_range`;
  - `0 < min_scale <= 1`.
- Damage scale is 1 up to `full_until_m`, falls linearly to `min_scale` at `min_from_m`, and holds there. It is applied to each hit by its distance in the simulation, so the server and the predicted client agree.
- Store it in `WeaponParams`; there is no default that hides missing data.
- IMAGINE's VX-9 carbine: full damage to 25 m, 0.6 from 50 m, `range_m` 80.
- Tests:
  - validation of each bound;
  - a proptest that the scale is monotonic non-increasing in distance and stays within `[min_scale, 1]`;
  - a hit at 10 m, 37.5 m and 60 m gives 20, 16 and 12.

**4. Aim slowdown (kit computes, engine applies, game decides).**
- **Kit:** `kits/fps/src/aim_assist.rs`.
  - `AimAssist::look_scale(eye, forward, targets: &[AimTarget], params) -> f32`. `AimTarget { centre, radius }`.
  - Params: `cone_deg`, `slowdown` (the scale at the centre), `max_range_m`.
  - It returns `slowdown` when the aim line passes through a target's sphere, ramps linearly back to 1 at `cone_deg` outside it, and returns 1 beyond `max_range_m` or with no targets. The nearest-angle target wins.
  - Pure, allocation-free, no world access.
  - Tests: no targets gives 1; on target gives `slowdown`; the ramp is continuous and monotonic; the range cut; a proptest that the result is within `[slowdown, 1]`.
- **Engine:** add one hook to the `Presenter` trait in `engine/app`: `fn look_scale(&self) -> f32`, default 1.0. `ClientApp` multiplies the look delta (mouse and touch drag) by it before accumulating `ViewAngles`.
  - Validate it: a non-finite value or one outside (0, 1] is a typed `AppError`, never clamped silently.
  - The engine names no target and knows nothing about aim assist.
  - Test it with a test presenter returning 0.5: the same motion turns half as far.
- **Game:** each frame the presenter builds the enemy target list from the latest frame (alive enemies only, no allies, no self) into a reused buffer and stores `look_scale` for the next frame.
  - Starting values: `slowdown` 0.65, `cone_deg` 4, `max_range_m` 40.
  - On by default for touch, off by default for mouse, with a toggle in settings.
- **No magnetism, no bullet bending.** The shot still goes where the view points, and the server validates hits as now, so multiplayer stays fair. Write this in PLAN §3.6.

**5. Touch controls (`games/imagine/ui/hud.html` and `touch.rs`).**
- Default layout:
  - movement on the left joystick, and pushing it past 90% for 0.25 s sprints;
  - camera by dragging anywhere on the right half;
  - one large fire button under the right thumb;
  - ADS smaller, above fire;
  - jump next to the right thumb, crouch below it;
  - reload small, beside the ammo counter;
  - SET becomes a small gear icon in the top corner.
- The second (left) fire button is off by default and is a setting.
- Buttons must not cover the centre 40% of the screen. Add a layout test: no button rectangle intersects the centre region at 16:9 or at 20:9.
- Keep button ids stable; a removed default is hidden, not renumbered. Re-cook `hud.ui.ron` with `cargo run -p ui-cook -- games/imagine/ui/hud.html games/imagine/ui/hud.ui.ron`.

**6. Shot feedback.**
- Hit markers and damage directions already exist (`feedback.rs`).
- Add crosshair expansion that follows the weapon's current spread.
- Add a small camera recoil kick per shot that recovers over about 120 ms. It is presentation only and never changes the aim the simulation uses.
- Impact particles wait for the particle system; do not fake them.

**7. Combat metrics (kit-fps, game-agnostic).**
- `kits/fps/src/combat_stats.rs`: `CombatStats` updated from the damage log and weapon fire each tick, with pre-sized storage. It records:
  - shots fired and hits (hit percentage);
  - a histogram of engagement distances (hit distance, 5 m buckets, giving the median);
- first-sighting and first-hit distances, time from first sighting to first hit, and time to kill;
- time from first visible entry into engage range to first hit;
  - **deaths before reacting**: died less than 0.5 s after first taking damage, without firing back.
- IMAGINE exposes it in its headless runs. Add a test that runs a 6-bot match for 3,600 ticks with a fixed seed and prints the metrics. Assert the median engagement distance is between 8 and 25 m.
- In the report, give the same numbers measured on `main` before your change (run the same scenario there) and after.

## Tests (summary)
- Sightline and spawn-visibility test.
- Screen-fraction test.
- Falloff validation, proptest and values.
- Aim-assist unit tests and proptest, and the engine `look_scale` test.
- Touch layout centre-clearance test.
- Combat metrics run with before and after numbers.
- Determinism: the scripted hashes change because the arena and tuning change. Re-record them and say why.
- The IMAGINE golden image changes because the arena changes. Re-record it and describe the change.

### Aim timing follow-up

With the original 3°/s aim-error recovery, the fixed-seed match first saw targets at a mean 15.4 m and first hit at 10.4 m. Mean sighting-to-hit was 309.5 ticks and mean engage-to-hit was 290.2 ticks. First sighting was already inside the 25 m engage range on average, so the delay was not bots closing from 35 m to 25 m. Raising aim-error recovery to 9°/s reduced those means to 22.8 and 20.0 ticks; the first-sighting and first-hit means were 15.4 m and 13.0 m.

**Do not touch:** `engine/render`, `engine/physics`, `engine/netcode`, `engine/assets`, `tools/cook`. In `engine/app`, add only the `look_scale` hook.

Other briefs are running at the same time:
- `imagine-menus` is adding a `take_travel` hook to `Presenter` and menu screens;
- `render-assets` is changing the viewmodel in `present.rs`;
- `physics-shapes` is migrating query callers to `QueryFilter`.

Keep your `present.rs` and `hud.html` edits focused. Run `git merge main` into your own branch before you finish, and resolve conflicts by keeping both sides' intent.

**Docs in the same change:**
- `games/imagine/docs/PLAN.md` §3.6 (falloff, aim slowdown rules, engagement range target) and §4 (touch layout);
- `games/imagine/docs/LOOK.md` (accent rule);
- `docs/ARCHITECTURE.md` §1 (kit-fps and engine-app rows);
- a new decision in `docs/README.md` §7 (aim slowdown only, no magnetism; falloff in the kit; engagement range drives arena design);
- `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
