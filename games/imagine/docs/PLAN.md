# IMAGINE: the plan

This is **the one plan** for IMAGINE: what we build, how the game works inside, how we make it look and feel AAA, how we work fast, and in what order. If another file disagrees with this one about *what* or *when*, this file wins and the other is fixed. Topic files keep the detail:
- *Why* is in [decision index](../../../docs/README.md).
- *How the renderer works* is in [RENDERING.md](../../../docs/RENDERING.md).
- *How content is made* is in [CONTENT.md](../../../docs/CONTENT.md).
- *What is wrong with the engine today, measured*, is in [2026-11-11-engine-audit.md](../../../docs/research/2026-11-11-engine-audit.md).

Updated 2026-11-11 for D51 (the game is a dynamic arena shooter run by an Arena Director), D52 (destruction returns as a tool of players and the Director, inside an indestructible shell) and D53 (what is compiled and what is data, and the build profiles that make every change fast). **[D]** marks a decision, **[Q]** an open question for the owner.

---

## 0. What changed in this version, and why

| # | Change | Why |
|---|---|---|
| 1 | The game is **arenas that change during the match** (Arena Director); since D52 its buildings also break | The owner's direction (D51, D52). The indestructible shell still allows baked lighting, which is the cheapest big jump in looks. |
| 2 | **Fun is proven in a greybox before art.** The Director, movers and Survival Protocol come before lighting and animation. | The Director is what makes this game different. If it is not fun with boxes, beautiful boxes will not save it, and art made for a layout that later changes is wasted. |
| 3 | **Online comes early** (milestone M2, before the art pass) | Multiplayer is the product. Network rules (who decides what, prediction, lag) change how gameplay code is written, so finding problems late means rewriting it. |
| 4 | **Sound ships with the guns**, not as a later step | A gun without sound cannot be judged for feel. |
| 5 | **Everything that is not a rule is data**, hot-reloaded and validated before it is swapped in | Today the map, weapons and bots are Rust code, so every change is a recompile. That is the main thing slowing us down. |
| 6 | **One path for every change to the match:** commands in, events out | Players, bots, the network, replays and tests all drive the game the same way, so each is tested by the others. |
| 7 | **Safety rules for the Director are checked by tools**, not trusted to designers | A moving wall that traps a player or kills without warning is a game-breaking bug. It must be impossible to ship one. |
| 8 | **Work runs in three lanes** (gameplay, content and look, tools and product) **with milestones** | Lanes touch different crates, so they can be built in parallel by delegated agents without colliding. |
| 9 | **Destruction is back** (D52): buildings inside the arena break, by players and by the Director; the shell around them never does | The owner wants destruction graphics at the level of the reference video ([REFERENCE.md](REFERENCE.md) §1a). Keeping the shell whole keeps baked light, bounded network cost and the Director's safety checks. |
| 10 | **Every change has a measured speed** and a home (D53): data hot-reloads, Rust rebuilds in about 1 s in a `dev` build that still runs fast | Measured: 1.1 s for a game-crate change in dev, 10–12 s in release. Content in Rust and a slow-running dev build are what make iteration slow, so both go. |

---

## 1. The game on one page

**A multiplayer first-person arena shooter in arenas that transform during the match.** An Arena Director moves walls, retracts bridges, locks doors, rearranges mazes, raises water and sends in robots, turrets, trains and giant machines, always announced first. Players fight each other and the arena.

**Pillars.** Each has a test that says whether we hit it.

| Pillar | Means | Test |
|---|---|---|
| **Feels great** | tight movement and guns; every action has sound, effect and feedback | testers pick it over their usual shooter for a 20-minute session; PRINCIPLES §5.1 G1–G8 pass |
| **The arena is a player** | the map changes the fight every 30–90 s, and players can use it (ride, hack, lock) | in a bot match, at least 4 Director events per round, and each moves where people fight (heat-map test) |
| **Fair surprises** | nothing kills or traps you without a warning you could see and hear | tool checks in §3.4 pass for every arena; no "unfair death" reports in playtests |
| **Things break** | walls, floors, glass and whole buildings break, change routes and stay broken for the round | a breach charge can drop a 2-storey front in the test arena within budget (REFERENCE R3); a bot match shows routes opened by destruction |
| **Looks premium** | stylized, clean, rich lighting and animation; reads clearly in a fight | matches the reference images per tier; enemies visible against every background |
| **Runs everywhere** | 60 fps on mid phones, 120+ on desktop | budgets in §8.3 hold for 10 minutes on the reference devices |

**Not making:**
- fully destructible maps: the shell (ground, outer walls, skyline, main paths) never breaks (D52);
- photoreal graphics;
- an open world;
- a story campaign;
- pay-to-win.

**Visual target:** stylized realism with heavy, physical destruction and dense effects, at the bar of the reference destruction shooter studied in [REFERENCE.md](REFERENCE.md) §1a: real-world proportions, physically based materials, one strong colour per area, big layered explosions and dust. All art is original; reference games set the bar, never the content (D23).

**Owner's priority weights:**
- combat, movement and feel: 30 %;
- graphics and animation: 25 %;
- Director and environment: 20 %;
- multiplayer: 15 %;
- AI and machinery: 10 %.

When two tasks compete, the higher weight wins unless it is blocked.

---

## 2. Modes and seasons

| Mode | Players | What you do | Main blocks (§3.2) |
|---|---|---|---|
| **Survival Protocol** (flagship) | 12–24 | survive while the Director transforms the arena round by round; last player or team standing wins | Lives (limited), Director, AI roster (turrets, robots) |
| **Control Shift** | 2 teams | hold zones that the Director keeps moving and reshaping | Teams, Zones, Director, Score |
| **Maze Break** | teams or solo | the arena is a maze whose walls rearrange; reach and hold the exit | Director (layouts), Zones |
| **Robot Uprising** | co-op vs bots | hold out against robot waves that grow and change tactics | AI roster (waves), Director, Lives |
| **Titan Hunt** | squads | bring down a giant machine by hitting weak points through phases | AI roster (boss), Interaction, Director |
| **Rising Tide** | 12–24 | water rises in stages; fight for high ground | Director (water), Lives |
| **Convoy Heist** | 2 teams | stop or escort a supply train | Carry item, movers (train), Score |
| **Predator Protocol** | 1 vs many | one hunter with special abilities stalks the others | Roles, Lives |
| **Deathrun Arena** | solo | race through moving traps | movers (lethal), Zones (checkpoints) |
| **Containment Breach** | teams | contain creatures escaping from labs | AI roster (creatures), Interaction, Zones |
| **Practice** | 1 + bots | try weapons, movement and Director events | everything, with a Director control panel |

| Season | Theme | Modes | New systems it brings |
|---|---|---|---|
| **1** | The Trials | Survival Protocol, Control Shift, Practice | Director, movers, interaction, turrets and robots |
| **2** | Labyrinth | Maze Break | maze layouts, occlusion by rooms and portals |
| **3** | Flood | Rising Tide | water, swimming |
| **4** | Machine War | Robot Uprising, Titan Hunt | robot waves, boss AI, procedural titan animation |
| **5** | Outbreak | Containment Breach, Predator Protocol | creatures, asymmetric roles |

**[Q]** Convoy Heist and Deathrun Arena are not yet placed in a season. Good modes stay in the main list; others return as limited-time events. **[Q]** The game title (working title IMAGINE, D21).

---

## 3. How the game works inside (game logic)

This section is the core design. Code follows it; a brief that disagrees with it must change it here first.

### 3.1 One tick, in order

The simulation runs at a fixed 60 ticks per second, is deterministic (same inputs give the same result, checked by a hash every tick) and is decided by the server. Each tick runs these steps in this order:

| # | Step | Reads | Writes |
|---|---|---|---|
| 1 | **Commands in**: player inputs (local, network or replay) and bot commands decided last tick | command queue | each actor's intent |
| 2 | **Director**: check event conditions; advance each event's stage (§3.4) | match facts | event stages, mover orders, spawn orders |
| 3 | **Movers**: set each mover's pose for this tick from its curve | mover orders, tick | kinematic bodies |
| 4 | **Movement**: characters move, riding the mover they stand on | intents, movers | character bodies |
| 5 | **Physics step** | bodies | contacts, poses |
| 6 | **Resolve overlaps**: push characters out of movers; apply lethal-mover rules (§3.5) | poses | positions, damage events |
| 7 | **Combat**: fire, hits (rewound for lag on the server), damage | intents, poses | damage events |
| 8 | **Interaction**: hold-to-use progress | intents | interaction events |
| 9 | **AI think**, within a budget per tick (§3.8) | world | bot commands for the next tick |
| 10 | **Rules**: score, lives, zones, round state | events | match state |
| 11 | **Events out**: HUD, sound, effects, network | event log | presentation |
| 12 | **Hash** | world | determinism check |

**One event log per tick.** Every system writes what happened (a hit, a kill, an event warning, a door locked) into one typed event log, **cleared at the start of the tick**. Any later system and the presentation read it.

This replaces today's arrangement, where `feedback.capture` must run between `fps.weapon` and `fps.damage` because the kit clears its own damage log ([sim.rs](../game/src/sim.rs)). Order-dependent clearing like that is a bug waiting to happen.

### 3.2 Modes are data made of blocks

A mode is a file `content/modes/<name>.ron` that picks blocks and sets their numbers. Optional code goes in a mode crate, but only for rules no block can express.

**Block contract.** A block is added only when it has all four parts:
1. a data section with validated fields;
2. a system in the tick;
3. its HUD piece;
4. a test.

A mode that needs something new adds a block, so the next mode gets it free.

| Block | Data | Used by |
|---|---|---|
| Teams | count, size, solo or squads, friendly fire | all |
| Lives | respawn time, lives per player, or one life | all |
| Score | what earns points, what wins, time limit | all |
| Rounds | round length, rounds to win, side swap | Survival Protocol, Control Shift |
| Zones | named zones, capture time, who can hold | Control Shift, Maze Break, Containment Breach, Deathrun |
| Director | which timeline file, intensity curve | all except Practice by default |
| AI roster | which bots, robots, turrets, creatures; counts and difficulty | Survival Protocol, Robot Uprising, Titan Hunt, Containment Breach |
| Interaction | which interactables are active | Titan Hunt, Containment Breach, Survival Protocol |
| Carry item | item, carrier rules, delivery zone | Convoy Heist |
| Roles | asymmetric players and their abilities | Predator Protocol |
| Loadout | fixed, chosen or found | all |
| HUD pieces | which pieces show | all |

**Match flow** is one state machine shared by every mode: `Lobby → Warmup → RoundLive → RoundEnd → (next round | MatchEnd) → Results`. Blocks hook into its transitions; no mode writes its own flow.

**Actors.** Actor slots come from a pool sized by the mode's data (up to 24 players plus the AI roster). This replaces today's fixed `ACTOR_COUNT`, so players can join, leave and reconnect.

### 3.3 The arena is a set of layouts

An arena does not have dozens of independent switches; that would make the safety checks impossible.

- The arena has a **small set of named layouts**, for example `open`, `bridges_up`, `lockdown_north` or `flooded_1`. Each layout says where every mover rests, which doors are locked and the water height.
- The Director moves the arena **from one layout to another** along a designed transition.
- Movers that run on their own (a train, a rotating machine, a patrolling platform) are **cycles**, independent of layout, and are checked over their whole cycle.

This keeps the number of states finite, so the tools can check every one (§3.4). It also makes every state something a designer chose and lit; each layout can have its own baked light (§5.1).

### 3.4 The Arena Director

**Event file** (`content/director/<arena>_<mode>.ron`):

| Field | Meaning |
|---|---|
| `trigger` | a time, or a condition on match facts: players alive, score gap, a zone held for N s, round number |
| `warning` | at least 3 s (5 s if lethal), with a HUD line, sound cue and in-world signal (lights, sirens) |
| `actions` | go to layout X; start a cycle; spawn an AI group; lock or unlock doors; switch a light state; enable a zone |
| `claims` | movers, zones and light groups this event uses |
| `cost` | intensity points, used for pacing |
| `weight` | chance when picked at random |

**Stages:** `Waiting → Warning → Active → Done`. Once `Warning` starts, the event always happens: a warning is a promise.

**Pacing.** Each mode has an intensity curve: a budget that rises over the round.
- The Director picks among ready events whose cost fits the budget left, by weight, with a random draw from the **match seed**, so every machine picks the same.
- A minimum quiet time separates events.
- Scripted events (fixed time) take priority over random ones.

**Claims stop clashes.** An event cannot start while another holds one of its claims. A property test proves no two active events ever hold the same claim.

**Safety rules, checked by the cooker for every layout and transition, and asserted every tick in match tests:**
1. **No trap.** From every walkable spot there is a path to a respawn or safe area, in every layout and at every step of every transition. This is checked on the navigation graph.
2. **No surprise death.** Only movers marked `lethal` may kill, only after a warning of at least 5 s, and the danger area is shown in the world during the warning.
3. **No crush without a way out.** A non-lethal mover closing on a player pushes them to the nearest free spot (§3.5). The cooker rejects a mover path that sweeps a space with no free spot next to it.
4. **No spawning into danger.** Spawn points inside a claimed area are switched off from `Warning` until `Done`.
5. **Readable.** Each event names a place (the HUD shows an arrow to it) and lasts long enough to react to.

**Network.** Movers are a pure function of the tick: `pose = curve(tick − start_tick)`. The server sends only "event X entered stage S at tick T" (reliable, a few bytes). Every client computes the same poses, so movers are predicted exactly and never jitter.

**Tools.** The dev inspector shows the timeline, the budget and every event's stage, and can fire, skip or scrub events. Practice mode exposes the same panel to players.

### 3.5 Movers

- **Kinematic bodies.** Their pose is set from data curves (a Blender action or keyframes); physics never pushes them.
- **Riding.** A character standing on a mover takes its velocity before moving. Jumping off keeps that velocity, so you can launch from a moving platform.
- **Pushing.** After the physics step, any character overlapping a mover is moved out along the mover's motion, or to the nearest free spot if blocked. Being pushed is never damage unless the mover is `lethal`.
- **Bots** treat mover surfaces as nav areas that connect only in the layouts where they line up (§3.8).

### 3.6 Movement and combat

**Movement** is one state machine per character:
- states: walk, sprint, crouch, slide (from sprint plus crouch), jump, mantle (ledges up to 1.6 m), grapple, swim (season 3);
- each state's numbers are data (speeds, timings, camera kick);
- a state change emits an event, so sound, animation and HUD follow.

**Weapons** are data (`content/weapons/*.ron`): damage by range, fire rate, magazine, reload, spread, recoil pattern, aim-down-sights zoom, sounds, effects and model.
- The **fire logic is one hit-scan path** shared by players, bots and auto fire.
- On the server it rewinds other players and movers to where the shooter saw them, up to 200 ms.
- Every weapon defines a full-damage distance, a linear falloff interval and a minimum damage scale; IMAGINE's VX-9 is full damage through 25 m, then falls to 60% at 50 m and stays there through its 80 m range.
- Bot fights are designed around 8–25 m: sight range 35 m, engage range 25 m and an 8 m stop distance. Arena lanes and cover shape those encounters.
- Bot aim-error recovery is tuned to 9°/s. The fixed-seed six-bot scenario records first-sighting and first-hit distances plus both sighting-to-hit and engage-to-hit latency; this keeps aim recovery measurable independently of closing distance.
- Touch aim slowdown is on by default for touch and off for mouse; a player setting controls it. It scales look input near an enemy within a 4° cone and 40 m. There is no magnetism or bullet bending: shots follow the view and the server validates the ray the player aimed.
- IMAGINE uses a data-backed vertical FOV of 55° by default, adjustable from 45° to 75°. The projection is Hor+, so wide screens reveal more horizontally; ADS scales the selected FOV.
- Explosives are simulated bodies.
- Melee is a short swept shape.

**Damage** goes through one function: damage event → armour → health → downed or dead → events.

### 3.7 Interaction

One `Interactable` with these fields:
- duration;
- who can use it (team, role);
- whether damage interrupts it;
- the action it sends to the Director or a mover when done.

Hack, repair, disable and activate are all this one thing with different data and effects.

### 3.8 AI

**Perceive → decide → act**, the same for bots, robots, turrets and creatures:
- **Perceive:** sight cone with line of sight; hearing (shots, footsteps; `bots.hear` today); events.
- **Decide:** utility scores read from data (attack, take cover, flee, follow objective, interact). It runs on a budget: near units every tick, far units every 4–8 ticks.
- **Act:** writes the same commands a player sends, so AI cannot cheat on movement or aim.

**Navigation** uses a navmesh baked by the cooker per layout. Mover surfaces are links that switch on or off with the layout. Turrets need no navigation and come first; robots come next; drones and bosses come later.

### 3.9 Data rules (all content)

- Every file is read as a raw form and then **validated into typed values** (ranges, newtypes, references resolved to typed ids), as `engine-scene` already does for levels. Invalid data never reaches the game.
- **Hot reload swaps only valid data, at a tick boundary.** A bad file keeps the last good version and shows an on-screen error with the file and line, so the game never crashes from an edit.
- Client and server compare a hash of all content when a player joins, so mismatched data cannot desync a match.
- All player-visible text lives in data with ids, ready for translation.

---

## 4. Screens and HUD

| Screen | What it shows |
|---|---|
| Home | your character, PLAY, the featured mode, party slots, season banner |
| Mode select | modes with players, length and a one-line rule |
| Party / Private room | invite, code, teams, mode, map, bots, start |
| Finding match | timer, players found, cancel |
| Loadout | weapon, gadgets, presets |
| Loading | arena, mode rules in 3 lines, tip |
| In game | HUD (below) |
| Death / spectate | who or what killed you (a player, a robot, the arena), respawn timer |
| Scoreboard | hold Tab |
| Results | win or loss, stats, XP, season progress |
| Settings | §6.6 |

**Current implementation status (2026-11-12):** IMAGINE starts in a menu-backdrop world and has cooked Boot, Home, Mode select, local Practice lobby, Loading, in-match menu, Scoreboard, Results, Settings and Loadout documents. The local lobby uses `engine-sessions` and standalone client travel. Loading is drawn for one frame before synchronous travel; streamed progress comes with `AssetUploader`. Debug desktop builds hot-reload cooked menus at a frame boundary and keep the last valid document; release builds embed them. Results and the live scoreboard bind actor kills, deaths, teams and winner data from the rules score. Settings persist through the versioned platform settings file; render scale, FOV, sensitivity and audio buses apply live, with tick-driven graphics rollback. Quality tier selection is disabled pending an `engine-render` runtime tier API. Menus have no Quit action; confirmed standalone Leave match returns directly to Home, while online Leave match awaits disconnect support. The product flow remains in progress: online lobby wiring and device visual review are outstanding.

HUD pieces; each mode turns on the ones it needs:

| Piece | Where | Used by |
|---|---|---|
| Health and armour | bottom-left | all fighting modes |
| Weapon, ammo, slots | bottom-right | all fighting modes |
| Gadgets | bottom-centre | all fighting modes |
| Score / players left | top | all |
| Timer and round | top-centre | all |
| Kill feed | top-right | fighting modes |
| Crosshair, hit marker | centre | fighting modes |

On touch, the left joystick moves and sprints after 90% deflection for 0.25 s; the right half remains a camera drag area. A large fire button sits under the right thumb, with smaller ADS above it, jump beside it and crouch below. Reload stays beside ammo and settings use a small gear in the top corner. A second left fire button is optional and hidden by default. Buttons stay outside the centre 40% of the screen.
| **Director warning**: "BRIDGE RETRACTS IN 10s" with an arrow to the place and a countdown ring | top-centre + on screen | all Director modes |
| Interaction ring | centre | Interaction block |
| Zone status | top | Control Shift, Maze Break |
| Boss health and weak spots | top | Titan Hunt |
| Water level and next rise | top-centre | Rising Tide |

Mockups are in `design/mockups/`; UI rules are in [design/](design/README.md).

---

## 5. Look: graphics and animation

### 5.1 Lighting

| Layer | What it is | Tier |
|---|---|---|
| **Baked lightmaps** | bounce light and soft sky shadow on static surfaces, computed once and stored. One set per layout where a layout changes the light (for example a lockdown closes shutters); the game blends between sets during the transition. | all |
| **Light probes** | a grid of baked light samples so moving things (players, robots, movers, guns) match the room | all |
| **Reflection probes** | baked cube maps per area, box-projected | all; box projection Medium+ |
| Screen-space reflections | sharp on-screen reflections on floors and water | High+ |
| Sun, physical sky, height fog | outdoor light and depth | all |
| Volumetric light shafts | beams in dusty rooms | High+ (cheap fake on Medium) |

**[D]** Bake in Blender with Cycles first. The export add-on bakes lightmaps into a second UV set and renders probes at probe empties. The stored format is ours, so an engine-side baker can replace Blender later.

**[D]** Lightmaps cover **only the shell** (D52). Destructible buildings are lit by probes, the sun and local lights, so a hole never shows stale baked light. When a building breaks, the probes around it are marked dirty and refreshed a few per frame on High; on Medium and Low they keep their values. Light through a new hole comes from the sun's real-time shadow, which already sees the change.

### 5.2 Shadows that look real

| Technique | Fixes | Tier |
|---|---|---|
| 4 stable cascades (texel-snapped), culled per cascade | blocky near shadows, shimmer | 2 Low, 3 Medium, 4 High |
| Cached static shadows: static drawn once, movers and characters on top each frame | most of the shadow cost (audit H1) | all |
| Soft shadows that harden near contact (PCSS) | the main "real" cue | High+; PCF elsewhere |
| Contact shadows (short screen-space ray) | floating feet and crates | Medium+ |
| Local-light shadow atlas, updated when something nearby moves | lamps, alarms, turret spotlights | 4 lights Medium, 16 High |
| Baked shadows from static lights | free soft shadows | all |
| Ambient occlusion (GTAO) + specular occlusion | dark corners, no shine in cracks | Medium+ |

### 5.3 Materials, effects and image

- **Materials:** textured PBR from Blender in KTX2 (BC7 desktop, ASTC phones), with trim sheets and decals. Shader looks: standard, emissive, glass, water, hologram. A gentle rim light keeps enemies readable.
- **Effects:** GPU particles from effect files (muzzle flash with light, tracers, impacts per surface, sparks, steam, splashes, robot bursts), decals and soft particles. All are data and hot-reloaded.
- **Destruction look** (D52, [REFERENCE.md](REFERENCE.md) §1a): big pieces fall first, then dust rolls out and hangs 6–12 s, then rubble stays.
  - Pre-cut chunks per element, cut in Blender; broken faces show inner material (brick, rebar, insulation) through a second material slot.
  - Dust and smoke as lit soft particles on all tiers, plus a low-resolution volumetric pass on High.
  - Small debris and glass shards as GPU particles with cheap collision against the depth buffer; capped per tier.
  - Explosions are layered: flash, fireball with a white-hot core, rolling smoke, spark streaks, debris, a short-lived light, a scorch decal, sound and camera shake.
- **Image:** AgX tone mapping and a grading LUT per season ([LOOK.md](LOOK.md)). TAA with sharpening and temporal upscaling on High+. MSAA 4× on phones (nearly free on tile GPUs). Dynamic resolution without rebuilding render targets (audit C3).

### 5.4 Animation (new crate `engine-anim`)

| Piece | Gives |
|---|---|
| glTF skeletons and clips (Blender, Mixamo, mocap packs), cooked | characters and robots |
| GPU skinning | many animated units cheaply |
| Blend trees + state machine with cross-fades, as data | walk, run, sprint, slide, mantle, aim |
| Layers and masks | reload while running |
| IK (two-bone, look-at) | hands on the gun, feet on stairs, robots aiming |
| First-person viewmodel: procedural sway, bob and recoil, plus authored reloads | the most-seen animation in the game |
| Ragdoll on Rapier, blended from animation | natural falls |
| Procedural rigs (titan legs, turret barrels, mechanical arms) | machines without hand-made clips |
| Animation LOD (far units at 15–30 Hz, no IK) | 24 players plus robots |
| Clip events | footsteps, effects, reload clicks on the right frame |

The network sends animation *inputs* (speed, aim, action), never poses. Motion matching is not needed for this style.

---

## 6. How we build fast (workflow)

### 6.1 Where every change lives, and how fast it lands (D53)

Modelled on Unreal (C++ modules + Blueprints + Data Assets + Live Coding) and Unity (assemblies + ScriptableObjects + prefabs): the engine is rebuilt rarely, rules live in small Rust crates, and everything a designer tweaks is data. The full comparison is in README §7 D53.

| Change | Lives in | Today (measured) | Target | How |
|---|---|---|---|---|
| UI layout and style | HTML/CSS → UI document | edit, cook; the running client reloads the changed packed document | under 1 s, no game running | `ui-preview` live window (§6.3); runtime documents load from the game pack |
| Arena geometry, lights, spawns, movers, destructible buildings | Blender → cooked arena | Kiln Yard geometry, spawns and waypoints load from the mounted pack; pack revisions reload changed level data | save in Blender, the game updates in 1–3 s | Blender live link (§6.2) |
| Weapons, bots, modes, Director, tuning, effects, sounds | RON files | validated RON entries reload from one changed pack in desktop development | instant, while playing | data (§3.9) + inspector (§6.4) |
| Shaders | WGSL | hot reload (built) | instant | `engine-render` `hot-reload` feature |
| Game rules (Rust) | game crate | See D53 for measured `dev`, `play`, and `release` build and frame times | about 1 s, and the dev build is fast enough to play | `dev` profile with optimised dependencies (below); `subsecond` (§6.5) |
| Engine (Rust) | `engine/*` | See D53 for measured `engine-world` incremental rebuild times by profile | same | small crates; the game never forces an engine rebuild |

**Build profiles (D53):**

| Profile | Own crates | Dependencies | Use |
|---|---|---|---|
| `dev` | `opt-level = 1`, incremental | `opt-level = 3` | every day: compiles in about 1 s and runs the game at playable speed |
| `play` | release, no LTO, `codegen-units = 256`, incremental | release | playtests and phone builds during development |
| `release` | thin LTO | release | shipping, benchmarks, budgets (§8.3) |

Android and release builds mount the same checked pack through the same loader, so there is one code path.

### 6.2 Blender as the editor (D15)

World pipeline status: Blender IDs, export validation, the cooked Level format, typed prefabs,
watch mode, arena selection and layout cues are wired through IMAGINE. The generated arena's
600-tick deterministic cue test passes. In desktop debug builds, `LiveScene` compares
entity-producing records by stable ID and applies transform-only moves, replacements, additions
and removals at the frame boundary without travel. It keeps gameplay-destroyed and
transform-changed entities until travel, then IMAGINE rebuilds its derived scene data from the
accepted level. Sample render goldens and automated Blender byte-reproducibility remain under
review.

Current status (2026-11-12): `tools/blender/export.py` validates scene rules, assigns persistent
object and source-file IDs, and exports GLB extras from the CLI or Blender panel. `tools/cook`
indexes cooked nodes and source files, writes the shared Level records, prefab data and layout
cues, and watches source content hashes. Sandbox loads the sample arena; IMAGINE selects its
cooked level by arena ID while Kiln Yard remains the default. Cooked instances use the shared
`AssetUploader`; each entity-producing record has a unique stable ID, and the sample prefab is
named `turret_base`.

1. **Export add-on** (`tools/blender/export.py`). A side panel sets each object's tags ([CONTENT.md](../../../docs/CONTENT.md) §4.3):
   - `static`, `mover` (with its action), `breakable`, `trigger`, `interact`, `spawn`, `zone`, `probe`, `cue`;
   - layouts as Blender collections.

   It checks names and budgets, and has **Export** and **Export on save** buttons.
2. **Cooker** (`tools/cook`) writes the cooked arena:
   - glTF to our mesh format, with LODs;
   - KTX2 textures;
   - collision from `_COL` meshes;
   - lightmaps and probes;
   - the navmesh per layout;
   - the level RON.

   It also **runs the Director safety checks** (§3.4). A failing arena does not cook.
3. **`engine-assets`**: handles, background loading and hash-based replacement from the mounted pack. A changed pack swaps valid cooked entries into the running game.

### 6.3 UI preview without the game

`tools/ui-preview` (brief `ui-preview`) renders any screen with the engine's own renderer, so what you see is what the game draws. You build it once; after that, editing screens needs no compile.
- **Live:** on save it re-cooks the HTML in-process and redraws within 500 ms, in phone and desktop frames. Cook errors show as a banner, and the last good version stays on screen.
- **Mock data:** checked preview states in `ui/preview/<screen>.ron` (`default`, `long_text`, `empty`, `full`). A stale binding is an error.
- **Overlays:** layout boxes, safe area, text overflow, touch targets under 48 dp.
- **For agents:**
  - `shot` writes PNGs for each screen, state and size;
  - `compare` writes a side-by-side image, a diff image and SSIM scores against a reference;
  - `check` fails on overflow, small touch targets or stale states.
  Agents follow the review loop in `docs/WORKFLOW.md`: shoot, compare with the approved mockup, review, fix, repeat.
- `ui-cook` stays the judge of what the game can draw.

### 6.4 Dev inspector and console (desktop debug builds only)

**[D]** egui, drawn over the game. It provides:
- a free camera;
- pause, step one tick, time scale;
- an entity list with live values;
- sliders on any data field, written back to the file;
- the Director panel;
- render toggles per pass;
- the frame profiler.

The console takes commands such as `spawn robot 3`, `director fire bridge_north`, `layout lockdown_north` and `tier low`.

### 6.5 Hot patching Rust (after M0)

Dioxus `subsecond` (as used by Bevy) patches changed function bodies into the running game. It is dev-only, behind a feature flag, and will be tried in the sandbox first.

### 6.6 Settings (saved to disk)

| Group | Settings |
|---|---|
| Graphics | preset (auto by device), resolution scale, frame cap 30/60/90/120, shadows, effects, bloom, motion blur (off by default), field of view, brightness |
| Controls | per-axis sensitivity (built), aim sensitivity, invert Y, key rebinding, gamepad, touch layout editor, auto fire (built), hold or toggle for sprint, aim and crouch |
| Audio | master, effects, music, voice, mono |
| Accessibility | colour-blind filters, subtitles, HUD scale, reduced shake ([design/ACCESSIBILITY.md](design/ACCESSIBILITY.md)) |
| Gameplay | crosshair style and colour, hit sounds, damage numbers |

Sensitivity, fire mode, graphics values, audio placeholders and reduced motion are saved as versioned RON through engine-app's generic `SettingsFile<T>`. Graphics previews revert after 600 simulation ticks unless confirmed. The platform owns text storage; game settings stay in the game.

### 6.7 Brand

1. Pick the name **[Q]**.
2. Write a brand sheet: the mark (wordmark plus a symbol readable at 32 px), two colours from `tokens.css`, the display font.
3. Make the SVG logo and export app icons for every iOS and Android size, the desktop icon, the splash, the loading screen and store art.

Claude drafts SVG options for the owner to choose from; a paid designer is worth it for the final mark.

---

## 7. Engine work this needs

The engine grows only from what the game needs, and stays game-agnostic (D24): no engine or kit code names IMAGINE, a mode or an arena.

| System | Where | State |
|---|---|---|
| Event log per tick | `engine-world` | new |
| Content validation and hot reload for every kind | `engine-scene`, `engine-assets` | levels and tuning built; assets new |
| Movers (kinematic curves, riding, push-out) | `engine-physics` + `kits/arena` | kinematic bodies built; the rest new |
| Arena Director, layouts, claims, pacing, safety checks | `kits/arena` (new kit) | new |
| Interaction | `kits/fps` | new |
| Movement states (sprint, slide, mantle, grapple) | `kits/fps` | walk and jump built |
| Weapons as data, recoil, ADS | `kits/fps` | rifle built in code |
| AI: perception, utility, navmesh, budgets | `kits/ai` (new kit) | waypoint bots built |
| Spatial grid (who is near) | `engine-spatial` (D27) | decided, not built |
| Assets: glTF, KTX2, handles, streaming | `engine-assets`, `tools/cook` | not built |
| Lighting, shadows, effects | `engine-render` (GX2–GX8) | partial |
| Animation | `engine-anim` (new) | not built |
| Audio | `engine-audio` (kira) | not built |
| Online: snapshots, prediction, lag compensation, relevancy, server | `engine-netcode` | loopback slice built |
| Retained render world, job-system use | `engine-render`, `engine-app` | audit H4, H6 |
| Rooms-and-portals occlusion | `engine-render` | for Maze Break |
| Dev inspector, console, profiler | `engine-devtools` (new, dev builds only) | not built |
| Frame error recovery | `engine-app` | built: recoverable frame errors keep the renderer and banner; 120 consecutive failures stop the client (audit C4) |
| Settings storage | `engine-platform` + `games/imagine/game` | built: versioned game settings stored as text in the platform config folder |

| Destruction: structural elements, pre-cut chunks, rubble, cosmetic debris | `engine-destruction` + `kits/destruction` (new kit) | load solver built; the rest new |
| Probe refresh after destruction | `engine-render` | new |

`engine-destruction` is **unfrozen** (D52). The structural solver decides what breaks on the server; pre-cut chunks become simulated bodies, then frozen rubble; dust, small debris and glass are client-only effects. Small props (crates, glass panes) are a prefab swap to pieces, without the solver.

---

## 8. Quality gates

### 8.1 Every change

The AGENTS.md gates (fmt, clippy, test, check-layers), tests with the code, and docs in the same change.

### 8.2 Match tests (headless, in CI)

A seeded bot match of each mode runs for a set number of rounds. Every tick it asserts:
- no character inside a solid or mover;
- every living character can reach a safe area (§3.4 rule 1);
- every lethal event was warned for at least 5 s;
- no two active events share a claim;
- the round ends within its time limit.

Running the same seed twice gives the same hash at every tick. A replay of the recorded commands also gives the same hash.

### 8.3 Budgets (targets to measure, not claims)

| Budget | Phone (Low, 60 fps) | Desktop (High, 120 fps) |
|---|---|---|
| Whole frame | 16.6 ms | 8.3 ms |
| Simulation (24 players + 40 AI units) | ≤ 2 ms | ≤ 1 ms |
| Render CPU | ≤ 4 ms | ≤ 2 ms |
| GPU | ≤ 12 ms | ≤ 6 ms |
| Network down per client, 24 players | ≤ 40 KB/s | same |
| Memory | fits the smallest supported device (D31) | — |

Benches run in CI with a baseline (audit §I).

### 8.4 Looks

Golden images per test scene and tier ([PERFORMANCE.md](../../../docs/PERFORMANCE.md) §3). From M3, the build is played **on a reference phone every week**.

---

## 9. Build order

Three lanes run in parallel because they touch different crates. Each milestone has an exit test; a step starts only when its inputs are done.

- **Lane G (gameplay):** `kits/*`, `games/imagine`.
- **Lane C (content and look):** `engine-assets`, `engine-audio`, `engine-render`, `engine-anim`, `tools/cook`, Blender.
- **Lane T (tools and product):** `engine-devtools`, `engine-app` robustness, settings, UI, brand.

### M0. Fast iteration (the base for everything)

| Step | Lane | Done when |
|---|---|---|
| M0.1 Event log per tick; damage and feedback move onto it | G | the `feedback.capture` ordering workaround is gone; tests pass |
| M0.2 IMAGINE moves to data: arena, spawns, weapons, bots and tuning in RON through `engine-scene`, validated, hot-reloaded with the keep-last-good rule | G | change rifle damage or move a wall while playing, with no compile; a broken file shows an error and the game keeps running |
| M0.3 Frame errors no longer quit (audit C4) | T | a forced render error is logged, shown by the engine-owned banner and the next frame draws |
| M0.4 Settings saved to disk (existing settings first) | T | sensitivity and fire mode survive a restart on desktop and Android |
| M0.5 Dev inspector v1: free camera, pause and step, live sliders, console | T | tune a bot's aim from the inspector and the file updates |
| M0.6 Build profiles `dev`, `play`, `release` (§6.1); HUD and other UI documents load from file in dev builds | T | a game-crate change rebuilds in about 1 s and the dev build plays at 60+ fps; editing `hud.html` updates the running game |

**Exit:** a day of tuning needs no recompile.

### M1. Playable greybox: find the fun

| Step | Lane | Needs |
|---|---|---|
| M1.1 Movement states: sprint, crouch, slide, mantle | G | M0.2 |
| M1.2 Weapons in data: rifle, pistol, shotgun; recoil pattern, ADS spread, swap; box viewmodel with procedural sway and recoil | G | M0.2 |
| M1.3 Audio v1 (`engine-audio`, kira): shots (near, far, tail), footsteps, hits, Director warnings | C | slice 1 built: buses, spatial one-shots, voices, IMAGINE placeholders and shot/footstep/hit/kill playback; deeper sound pass remains M3.8 |
| M1.4 Movers: curves, riding, push-out, lethal flag | G | M0.1 |

| M1.5 Director v1: layouts, events, stages, claims, pacing, warnings in the HUD; Director panel in the inspector | G | M1.4, M0.5 |
| M1.6 Director safety checks as a library, used by match tests now and by the cooker in M3 | G | M1.5 |
| M1.7 Interaction v1 (hold to use) | G | M0.1 |
| M1.8 Turrets (the first AI machine) | G | M1.5 |
| M1.9 Match flow state machine; modes as data; Survival Protocol with bots in a greybox arena | G | M1.1–M1.8 |
| M1.10 Match tests in CI (§8.2) | G | M1.9 |
| M1.11 Destruction greybox: one 2-storey box building of structural elements; grenade and breach charge; walls, floors and supports break through `engine-destruction`; large pieces fall and become rubble that blocks movement; the Director can collapse a named group with a warning | G | M0.2, M1.2 |

M1.1 is implemented and under review: input levels drive one kit movement state, crouch shape changes are clearance checked, and movement tuning hot reloads at tick boundaries.

**Exit:** the owner plays 10 rounds of Survival Protocol with bots and wants more. The match tests pass.

### Lane C from M0 onward: content path (runs in parallel with M1)

| Step | Done when |
|---|---|
| C1 Blender export add-on (tags, layouts, export on save) | the add-on exports the pistol and a test room with tags |
| C2 Cooker: meshes, KTX2, collision, level RON | cooked files load in the sandbox |
| C3 `engine-assets`: handles, background load, hot swap | save in Blender and the change shows in the running game in ≤ 3 s |
| C4 Textured PBR (GX2 remainder) | the pistol `.glb` draws with its materials |
| C5 UI preview tool (lane T) | edit `hud.html` in the browser with mock values |

### M2. Online slice

| Step | Lane | Needs |
|---|---|---|
| M2.1 Snapshots with delta and relevancy; client prediction for the local player; Director stage messages | G | M1.9 |
| M2.2 Lag-compensated hits (rewind ≤ 200 ms) | G | M2.1 |
| M2.3 Dedicated server binary; real transport (quinn) | G | M2.1 |
| M2.4 Join with content-hash check; reconnect | G | M2.3 |

**Exit:** 12 players (humans and bots) at 150 ms simulated ping. No visible mover jitter, hits feel right, and the bandwidth budget holds.

### M3. Beautiful arena (vertical slice)

| Step | Lane | Needs |
|---|---|---|
| M3.1 The Season 1 arena built in Blender from the M1 greybox layout, with its layouts and movers | C | C1–C4, M1 exit |
| M3.2 Cooker runs safety checks and bakes the navmesh per layout | C | M1.6, C2 |
| M3.3 Lighting: Blender bake of lightmaps per layout, probes, reflection probes | C | C2 |
| M3.4 Shadows: 4 cached stable cascades, contact shadows, PCSS on High, local-light atlas (GX4) | C | — |
| M3.5 Post, sky and fog: AgX, grading LUT (GX5, GX8) | C | — |
| M3.6 Effects: GPU particles, decals, effect files (GX3) | C | C4 |
| M3.7 Animation: `engine-anim`, first-person hands, a placeholder character (Mixamo), turret and robot procedural rigs | C | C2 |
| M3.8 Sound pass 2: surfaces, reverb by area, mix | C | M1.3 |
| M3.9 Phones get the right tier (audit C1); first measured phone run | T | — |
| M3.10 Destruction look: Blender pre-cut chunks with inner material, layered explosions, hanging dust, spark streaks, glass shards, probe refresh after a break (§5.1, §5.3) | C | M1.11, M3.6 |

**Exit:** the arena matches its reference images per tier. The phone holds 60 fps on Low for 10 minutes. The owner says it looks like a product.

### M4. Season 1 complete

| Step | Lane | Needs |
|---|---|---|
| M4.1 Control Shift | G | M2 |
| M4.2 Robots: navmesh AI, utility decisions, budgets (`kits/ai`) | G | M3.2 |
| M4.3 Grapple | G | M1.1 |
| M4.4 24 players: retained render world and job-system use where measurements show the need (audit H4, H6) | C | M2 |
| M4.5 Menus as documents: home, mode select, party, loadout, full settings, results | T | C5 |
| M4.6 Brand: name, logo, icons, splash | T | the owner's name choice |
| M4.7 Accessibility basics ([design/ACCESSIBILITY.md](design/ACCESSIBILITY.md)) | T | M4.5 |

**Exit:** an internal online playtest where most people want another round, and the PRINCIPLES launch checklist passes for Season 1.

### M5 onward: seasons

| Milestone | Builds |
|---|---|
| M5 Season 2 | Maze Break: maze cells as layouts with generated, checked transitions; rooms-and-portals occlusion |
| M6 Season 3 | Rising Tide: water, swimming, underwater look |
| M7 Season 4 | Robot Uprising (waves), Titan Hunt (boss AI, procedural titan) |
| M8 Season 5 | Containment Breach (creatures), Predator Protocol (asymmetric roles) |

**Critical path:** M0 → M1 → M2 → M4. Lane C must reach C4 before M3.1. Performance work from the audit lands inside the step that needs it (shadows in M3.4, the retained render world in M4.4), not as a separate phase.

**Old step numbers.** Docs written before this version refer to build steps 1–10. Map them as follows:

| Old step | New |
|---|---|
| 1 | done |
| 2 | M0 + lane C |
| 3 | M1 |
| 4 | M3 |
| 5 | M2 |
| 6 | M1.9 |
| 7 | M4.5 |
| 8 | M4 |
| 9 | M4 exit |

---

## 10. Risks

| Risk | Effect | Answer |
|---|---|---|
| The Director is not fun | the core idea fails | M1 proves it in a greybox before any art; Practice mode lets testers fire events by hand |
| Moving walls feel unfair | players quit | safety rules (§3.4) checked by tools and match tests; the playtest asks "did any death feel unfair?" |
| Movers desync online | jitter, wrong hits | movers are a function of the tick; lag-compensated hits rewind movers too; the M2 exit test |
| Baked light looks wrong on movers | movers look pasted on | movers lit by probes plus real-time lights; per-layout bakes blended |
| Phones too slow | no mobile release | tiers set per device; measured from M3.9; budgets in §8.3 |
| Too much engine, too little game (as before D51) | slow progress | every step serves a milestone exit; nothing is built "for later" |
| The animation runtime is large | M3 slips | start with clips, a blend tree and the viewmodel; IK and ragdoll follow |
| Destruction costs too much on phones or online | frame or bandwidth budget broken | the shell never breaks; only structural breaks cross the network ("element E broke at tick T"); cosmetic debris is client-only and capped per tier; measured in M1.11 and M3.10 |

## 11. How we work

Claude writes a brief for each step (`docs/briefs/`, template in [WORKFLOW.md](../../../docs/WORKFLOW.md)). A delegated agent builds it in its own worktree, Claude reviews the diff, and the owner merges. Steps in different lanes can run at the same time.

**Needs the owner:**
- the game title;
- the reference phones and any Windows PC;
- free or paid;
- playtesters;
- the final logo choice;
- later, the real character and gun models.
