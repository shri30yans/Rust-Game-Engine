# IMAGINE: start here

**IMAGINE** (working title) is a multiplayer first-person arena shooter whose arenas change during the match, run by an Arena Director, with destruction inside an indestructible shell and many game modes added over seasons (§1 below). The one plan, with the game logic and build order, is [PLAN.md](PLAN.md). It is one game on our engine platform: it uses the engine and the shooter kits, and nothing in the engine depends on it ([ARCHITECTURE.md](../../../docs/ARCHITECTURE.md) §0). Platform docs, workflow and code rules start at [docs/README.md](../../../docs/README.md).

## Docs for this game

| Topic | File | Answers |
|---|---|---|
| The plan (single source) | [PLAN.md](PLAN.md) | the game, modes, seasons, game logic (tick, Director, movers, AI, data), HUD, look, workflow, quality gates, build order (§9) |
| The reference bar | [REFERENCE.md](REFERENCE.md) | what a top arena shooter looks and plays like, measured from video, and what we never copy |
| How the world looks | [LOOK.md](LOOK.md) | setting, style, colour, light, grade, arena-change look, readability |
| Placeholders and asset list | [ASSETS.md](ASSETS.md) | placeholder characters and guns, everything needed for launch |
| UI design system | [design/README.md](design/README.md) | tokens (`theme.ron`), HUD, menus, components, motion, accessibility, mockups |

Shared with every game: the principles and quality bar ([PRINCIPLES.md](../../../docs/PRINCIPLES.md)), the code ([ARCHITECTURE.md](../../../docs/ARCHITECTURE.md)), the renderer ([RENDERING.md](../../../docs/RENDERING.md)), the content pipeline ([CONTENT.md](../../../docs/CONTENT.md)) and budgets ([PERFORMANCE.md](../../../docs/PERFORMANCE.md)).

## 1. Design choices

IMAGINE's own decisions, with reasons. Engine decisions live in the platform docs. A changed choice is edited here in place; git keeps the history. Proposed choices wait for the owner.

**Product.**
- **One shooter, many modes, seasons.** One first-person base with modes added over seasons; modes are built from shared blocks, so a new mode is a settings file plus a little code. Rejected: a single-premise game (a TV game show, then a survival trial; the owner asked for no TV premise and no single theme; the changing-map battle royale survives as the Last Stand mode); "Cashout" as a mode name and loop (too close to THE FINALS' signature mode).
- **Plain language everywhere.** Docs, UI text and briefs use plain words and explain a term the first time it appears.
- **Placeholders first for characters and weapons.** Simple script-made Blender models ([ASSETS.md](ASSETS.md) §1); effort goes to gameplay, graphics and effects first. The owner builds the real characters later; the replacement contract (same names, bones and sockets) keeps the swap trivial.
- **Working title "IMAGINE"** (proposed), named after the engine repository, until the owner picks the real title.
- **Reference games set the bar, never the content.** We measure effects density, lighting, destruction scale and pace from gameplay video of THE FINALS ([REFERENCE.md](REFERENCE.md)) and hold the game to those numbers. Maps, characters, HUD, gadgets, names and setting are all original. Why: copies would be someone else's work and a legal risk.

**Arena and destruction.**
- **A dynamic arena run by an Arena Director.** An Arena Director runs a data timeline of announced events (walls and mazes rearrange, bridges retract, doors lock, water rises, robots, turrets and trains appear), with movers players can ride. Events are deterministic from the tick, so the network sends an event and its start tick, not mover positions. Modes and seasons: [PLAN.md](PLAN.md) §4–5.
- **Destruction is a tool of players and the Director, inside an indestructible shell.** The shell (ground, outer walls, skyline, main paths) never breaks. Destructible buildings sit inside it and are lit by light-cache probes, the sun and local lights; breaking a piece invalidates the light cache locally ([RENDERING.md](../../../docs/RENDERING.md) §13). Three sizes of destruction: structural elements in `engine-destruction` (server-decided, deterministic, replayed by clients); large pre-cut chunks simulated on the server for a few seconds, then frozen as rubble that blocks movement; cosmetic debris, dust, sparks and glass, client-only and budgeted per tier. The Director can break a named group with the same warning rules as players' explosives. The cook checks that no allowed collapse removes the last path to a safe area; falling pieces only harm after a visible warning or from a player's own explosive. Rejected: fully destructible maps including the shell (no stable light, unbounded network cost, Director safety checks impossible); purely visual destruction (changing routes is what makes it fun); physics-only collapse (not deterministic).
- **Short engagements.** Fights at 8–25 m, 35 m bot sight, 25 m engage, 8 m stop distance; lanes and cover enforce short sightlines. Characters are 1.8 m. Vertical FOV 55° by default (45–75°, Hor+); scope scales the selected FOV. At the default FOV a target fills at least 11% of screen height at 15 m. Character accents are the most saturated colours on screen. Aim slowdown is on by default only for touch, never magnetism; shots follow the view.

**Controls, HUD and menus.**
- **Touch controls follow the mobile battle-royale layout:** a left move stick; drag on the right half to aim; a big FIRE on the right that also aims while held (a drag-look region); an optional small left FIRE for claw players; a scope toggle (ADS or Q) that halves the FOV and the look scale; JUMP and RELOAD; a settings gear. All are visible, labelled buttons drawn from the HUD document and hit-tested against what was drawn; hidden gestures were removed because players could not discover them. Only the layout is copied, not the art. Rejected: hold-to-scope (no source for the original; a toggle is the simpler single path); a user-editable layout (for now).
- **The HUD is a UI document** cooked from `ui/hud.html` (the slanted `hud-angled` mockup, limited to what the game has data for): score stack, match clock, kill feed, elimination callout, health with a segmented bar, ammo with the weapon name, build line. The game's Rust only fills bindings. Skew stands in for the mockup's perspective tilt. Not built because the game has no data: objective markers, squad list, chat, teammate bars, other weapon slots, floating enemy health bars.
- **Settings:** per-axis look sensitivity in steps 1–20 (step 10 is the engine default of 0.004 rad/px, up 60% from before); manual or auto fire (auto pulls the trigger when the weapon's own ray first hits a living enemy, evaluated on the server's tick); graphics previews that revert unless confirmed; reduced motion. Saved through `SettingsFile<T>`.
- **UI motif "Work Site"** (proposed): construction-site and survey parts (callout strips, corner marks, a status lamp, grid references, hazard stripes, blueprint grid, halftone), re-colourable per season ([design/](design/README.md)). Replaced the TV "Control Room" motif with the TV premise.

**Bots and modes.**
- **Free-for-all bots that act like players:** every actor is its own team; a bot picks the nearest enemy it can see, else the nearest; idle bots hear shots within 45 m and investigate; strafe gaps vary; attacking bots jump by chance; reaction 200 ms, starting aim error 4°, aim settling 3°/s. Not built: retreating, cover, grenades, squad play.
- **Mode capacity owns bot counts:** each mode declares typed minimum and maximum players; the lobby derives its capacity from them; the bot roster and each available mode's spawn markers must cover `max_players - 1`; the match uses the bots seated in the room.


## Code and content

| Folder | Holds | State |
|---|---|---|
| `games/imagine/game/` | the `imagine` package: desktop binary, Android library, arena scene, tests | playable arena built (greybox for M1, see below); server binary planned |
| `games/imagine/android/` | Android manifest (`com.imagine.game`, label IMAGINE, `lib_name` `imagine`) | built; APK not run on a device |
| `games/imagine/modes/*` | mode data and optional rule crates for Survival Protocol, Control Shift, Practice, … | planned (M1.9) |
| `games/imagine/content/` | Blender files, guns, effects, modes, Director timelines, UI screens | started (pistol); data files from M0.2, cooked content from lane C |

Kits it uses: `fps_controller`, `weapons`, `combat`, `gadgets`, `objectives`, `ai` (later `roles`).

## Playable arena (imagine-arena)

Status: built and tested on the host; the Android APK is built and signed; **neither has been played on a device, so feel on a phone is unverified.**

Run a fast-compile desktop build with `cargo run -p imagine` (`dev`), or a full-speed playtest with `cargo run -p imagine --profile play`. Keep `--release` for shipping and benchmarks; Android release APKs use `--release`. IMAGINE starts in its menu flow over the menu-backdrop world. The local Practice lobby can add bots and start a room-backed match. In a match, WASD (or arrows) move, Space jumps, the left mouse button fires, R reloads, F3 toggles the frame telemetry panel, and the mouse looks. Settings are on the Home or match menu screens. On Android the telemetry panel starts shown and a tap on its drawn top-left panel toggles it. Touch regions are installed after the first successful UI draw, so a touch during the first frame intentionally hits nothing. `UiScreen` replays its last good draw list when a layout, binding, or draw fails, while input keeps the last successfully installed regions. The HUD test matrix runs match-start, longest score rows, full feed, warnings, left-fire and ADS states across 1920×1080, 2400×1080, 2340×1080, 2800×1280, 1600×720, and 640×360; it rejects any original interactive rectangle outside the safe viewport. At 2800×1280 match start, the FIRE control from `.b-fire` computes to `(x: 2313.6, y: 601.6, w: 332.8, h: 332.8)` and every authored control is inside the viewport. There is therefore no off-screen control or CSS rule to correct in the checked-in HUD: the prior phone crash was the removed fraction-coordinate conversion, whose weaker no-font layout could create a rejected input value before drawing. A downed player respawns after 3 s; a downed bot respawns after 5 s at a bot spawn point. The HUD shows the crosshair, score stack, match clock, kill feed, elimination callout, health, ammo with weapon name, and hit and kill markers.

**Settings:** the cooked Settings screen sets graphics preview values, X/Y look sensitivity, fire mode, master volume placeholder and reduced motion. Values are saved through engine-app's generic `SettingsFile<T>`; graphics previews revert after 600 fixed ticks unless confirmed. The presenter converts fire input and the selected mode into `kit-fps::Trigger::Held`, `Released` or `OnTarget`; it does not inspect the world. `OnTarget` is resolved by the weapon system on the authoritative tick, using the weapon's own ray and excluding the shooter's body.

**Match setup and travel:** `modes.ron` owns each mode's typed `min_players` and `max_players`; player-range text is derived from those numbers. Only available modes can start. The tuning bot roster and each available mode's arena must provide at least `max_players - 1` unique bot names and spawn markers. The lobby cannot exceed that mode's room capacity, and match settings use the bots actually seated in the room. `ImagineSettings` is the room's settings. At match start `LocalSession` copies the bot seats and teams into those settings and the presenter requests travel through `take_travel`. `ClientApp` applies it at a frame boundary before the next tick. Results Continue and confirmed standalone Leave match request a fresh menu-backdrop world through the same path. Online Leave match will disconnect when online session teardown is implemented; it is not shown in online sessions because world travel is server-owned. IMAGINE clears actor and scene identity caches after travel.

If standalone world setup fails during travel, `engine-app` preserves the current world, reports the failure in its error banner, and IMAGINE returns to Home. Desktop development reloads changed entries after the cook rewrites the pack. Android reads the same packed bytes from NativeActivity assets; settings use the activity's internal app-data directory.

**Editing the HUD:** the HUD and the touch buttons are `games/imagine/ui/hud.html` (the slanted design of the `hud-angled` mockup). Edit it, run `cargo run -p ui-cook -- games/imagine/ui/hud.html games/imagine/ui/hud.ui.ron`, then run `cargo run -p cook -- imagine`. A running desktop build reloads the changed pack entry; a bad edit shows a red bar while replaying the last good HUD. The source-to-cooked test fails if the committed file is stale.

All twelve IMAGINE screens have checked-in sample bindings and are compared with headless Chrome at 1280×720, 1920×1080 and 2800×1280. The HUD health bar keeps a full-width box: its filled area is 256 px at 100/100 and 64 px at 25/100 inside the same 258 px bar box at 1280×720.

**Layout** (`data/arenas/kiln_yard.level.ron`). A perimeter wall 4 m high and no gaps; a central two-storey building with two doorways, a stair on one side and a 1.0 m ledge on the other that must be jumped; cover clusters of crates near the four corners; a long low wall; a ramp up to a raised platform; a shallow trench. Spawns: 4 player and 6 bot points, all at least 8 m apart and clear of geometry. Shape names carry game-side roles through `floor_`, `wall_`, `crate_`, `building_`, `ramp_`, and `platform_` prefixes; spawn names start with `player_` or `bot_`.

**Bot routing.** The waypoint graph (25 markers) is built at setup from the arena's geometry by the kit (`kit-fps`). The level loader checks that every waypoint and spawn stands clear of geometry; scene tests check that the graph is connected, that every spawn can see at least one waypoint, and that line of sight between spawns is limited to at most half of the pairs (16 of 45 pairs). Bot respawns choose a point far from the player and the living bots so simultaneous respawns do not stack into one blocked body pile.

**Tuning** lives in `data/tuning.ron`, with durations in milliseconds and angles in degrees. When desktop development cooks a changed pack, valid tuning applies before the next simulation tick and valid level edits replace the arena. A broken edit shows its logical path and reason in the error banner and keeps the last good values. Changing rifle damage, bot aim, movement, respawn delays, feedback timers, or link rules needs no compile. The room determines the match's bot count; tuning provides an ordered roster of names, validated against each available mode's capacity. Desktop, Android and headless tests read the same pack entries.

**Tests** (`games/imagine/game/tests/`, headless, scripted, fixed seed):
- `scene.rs`: waypoints and spawns clear of geometry, graph connected, spawns spread apart and mostly out of each other's sight, perimeter closed, the ledge needs a jump and the stair is walkable.
- `autofire.rs`: `OnTarget` consumes ammunition in the command's tick when an enemy is first on the weapon ray and does not fire into empty air. `bots.rs`: with the player standing still, bots kill bots within two minutes.
- `bots.rs`: bots kill bots, every bot kill is credited and reaches the kill feed.
- `gameplay.rs`: a bot is killed by the player and respawns; a bot kills a stationary player and the player respawns; sixty seconds of six bots run without error, with the same state hash across runs of one seed; a downed player cannot move. The six-hundred-tick Manual and Auto state hashes are recorded for this bot-roster configuration; Auto eligibility is checked by the server's weapon ray immediately before the weapon step.
- `no_alloc.rs`: the game's own systems allocate nothing after warm-up. Physics is not claimed to be allocation-free ([PERFORMANCE.md](../../../docs/PERFORMANCE.md) §5).

`cargo test -p imagine --test e2e` runs the scripted client journey. It checks client state, error banners and text overflow while it visits menus, starts a room-backed match, plays a fixed tick sequence, reaches Results and returns Home. Menu draw-list goldens cover every menu screen at 2400×1080; layout checks cover every menu and the HUD at 1920×1080, 2400×1080, 2340×1080 and 1600×720.

**Android.** `scripts/build-android.sh imagine` builds `target/android/imagine-debug.apk`, signed with the debug key, and stages `content/cooked/imagine.pak` as `assets/imagine.pak`. The platform opens this pack through the NativeActivity asset manager; IMAGINE uses the same `PackReader` and logical paths as desktop and headless tests. Settings live under the activity's internal app-data directory. The platform drops renderer resources when Android revokes the surface and rebuilds them after resume while retaining the simulation world. `scripts/tests/build-android.sh` checks the game-name argument without the NDK.

**Frame capacity.** The game's 64-entity dynamic budget covers live actors and feedback entities.
`engine-app` adds that budget to the loaded world's entity count when sizing client extraction
storage, and recalculates after travel or a successful level edit.

**Weapon asset.** The default first-person tactical pistol is cooked from `content/weapons_batch1/export/pistol.glb` into `content/cooked/weapons/pistol.meshpack`. The presenter loads it through `AssetStore`, and `AssetUploader` makes its PBR materials and meshes visible only when the pack is ready. The viewmodel stays absent while the first upload is pending. `socket_muzzle` positions muzzle flashes and tracers. Bronze and crimson pack handles are loaded for later loadout selection but remain unused.

The headless viewmodel golden now shows the cooked silver pistol in camera space; the previous frame showed only the small dark blue procedural placeholder shape at the lower right. The cooked frame includes the slide, grip and controls, with the arena and HUD unchanged.

After merging combat-feel, the match goldens were recorded again: the combat frame now has the three-lane arena and 55° default FOV, and both match frames include the cooked pistol. The menu reference was also refreshed after a small TAA raster difference (maximum channel delta 7); its visible composition is unchanged.

**Not done or not verified.** Not played on a phone or a desktop by a person. Touch controls are on-screen buttons on the right: FIRE (held to fire), ADS, JUMP, R (reload) and the settings gear, drawn only on Android, plus the left stick and right-half look. The buttons are untested on hardware. Touch controls are hit-tested against the exact clipped regions drawn by the HUD, not a second fraction layout. The first frame intentionally has no touch regions until the HUD has drawn once. Routing picks the nearest waypoint by horizontal distance, so it is not multi-storey aware. Physics allocates inside Rapier.
