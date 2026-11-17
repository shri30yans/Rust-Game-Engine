# Architecture: how the engine is built

The single source of truth for how the code is organised and how each system works, with the reasons beside each design. Rendering is in [RENDERING.md](RENDERING.md), the content pipeline in [CONTENT.md](CONTENT.md), budgets and measurements in [PERFORMANCE.md](PERFORMANCE.md), the reasoning behind all of it in [PRINCIPLES.md](PRINCIPLES.md).

The engine is ours. Common jobs use well-made Rust libraries (most of them the ones Bevy is built on), each behind one wrapper crate (§13). It is **a platform**: it knows nothing about any game, so many games can be built on it. Its standout systems are a deterministic, server-authoritative simulation and a world that takes part in play: structures carry load, break and leave rubble.

## 0. Platform: engine, kits, games

```text
engine/            the platform: game-agnostic crates (layers 0–4)
kits/              optional gameplay packs for a genre (layer 5), e.g. first-person shooting
games/<name>/      one folder per game
  game/            the game crate: client and dedicated-server binaries (package = the game's name)
  modes/<mode>/    optional rule crates for that game's modes
  content/         that game's Blender files, data, sounds, UI screens and theme
  docs/            that game's plan, design and reference
tools/             check-layers, cook, texture-encode, ui-cook, bench-gate, Blender scripts, the new-game template
docs/              platform docs
```

| Game | Folder | Uses | Purpose |
|---|---|---|---|
| IMAGINE | `games/imagine/` | engine + kits | an arena shooter ([its docs](../games/imagine/docs/README.md)) |
| Sandbox | `games/sandbox/` | engine only | proves the engine stands alone; holds the engine test scenes and benches |

**How a game plugs in.** A game implements `app::GameEntry`; its `CONTENT` names its one pack. The engine starts the platform, detects the device profile, opens and validates the pack, and only then calls `GameEntry::build(&Start)`, which receives mounted content, device capabilities and the settings store. The engine owns fixed ticks, input, presentation, audio and network hosts; the game owns its rules.

**Decoupling rules.** `check-layers` enforces the first five; review enforces the rest.
1. Dependencies point down only: games → kits → engine.
2. `engine/*` depends only on `engine/*`.
3. `kits/*` depends only on `engine/*` and other kits.
4. A crate under `games/X/` depends only on `engine/*`, `kits/*` and `games/X/`. Games never depend on each other.
5. `tools/*` depends only on `engine/*`.
6. No game words in `engine/` or `kits/`: no game, mode, map or weapon names, no paths into `games/`.
7. Every game has its own network protocol id; the handshake refuses a client from another game or version.
8. The sandbox builds and passes its tests in every gate run. When an engine change would need a game-specific hook, it becomes a general engine feature, or it stays in the game. A feature only one game uses lives in that game.

**Why.** More games will be built on this engine; if the boundaries are not drawn now, game code leaks into the engine and every new game becomes a fork. **Rejected:** one game crate with feature flags for other games (couples them); one repository per game now (too much overhead while the engine changes daily; the folder rules keep a later split free).

**A new game:** `scripts/new-game.sh NAME`, add kits as needed, add its `games/NAME/*` lines to `tools/check-layers/layers.json`, write `games/NAME/docs/README.md`.

## 1. Crates and layers

A crate may depend only on crates in **lower** layers. `cargo run -p check-layers` enforces this and the §0 rules from `layers.json`; every new crate adds its line there. Same-layer pairs allowed: `cook` → `texture-encode` (one shared Basis encoder) and `engine-gpu` → `engine-core` (generation-checked `Slots<T>` is not duplicated).

| Layer | Crate (folder → package) | Job | Main libraries | State |
|---|---|---|---|---|
| 0 | `engine/core` → `engine-core` | ticks, seeded random numbers, state hash, quantisation, errors, screen geometry (`Viewport`, clipped `ScreenRect`, `TouchRegion`), generation-checked `Slots<T>`, dev-only file watch | — | built |
| 0 | `engine/gpu` → `engine-gpu` | the safe Vulkan layer: device and caps, memory, resources, render passes with subpasses and transient attachments, pipelines and cache, command lists, frames in flight, swapchain; the one `unsafe` crate ([RENDERING.md](RENDERING.md) §2) | ash, ash-window, gpu-allocator | planned ([vk-1](briefs/vk-1-gpu.md)) |
| 0 | `engine/jobs` → `engine-jobs` | the job pool and the background pool; ordered chunked parallel helpers (§3) | rayon | built |
| 1 | `engine/world` → `engine-world` | entity storage, network ids, transforms, component registry, resources (in the state hash), update order | hecs, glam | built |
| 1 | `engine/platform` → `engine-platform` | window, input (keyboard, mouse, gamepad, touch), GPU start-up, device profile, content stream, config store, presentation binding | winit, wgpu (until vk-2), gilrs | built: unified desktop/NativeActivity entry, device profile, touch regions, Android config, content opening, desktop pack revision |
| 1 | `engine/spatial` → `engine-spatial` | the cell grid for "what is near" (§5) | glam | planned |
| 1 | `engine/assets` → `engine-assets` | logical asset paths, validated content packs, typed handles, background loading, Basis texture preparation, hot reload | postcard, basisu | built: validated packs, logical paths, typed stores, versioned mesh packs, hash-based reload |
| 2 | `engine/physics` → `engine-physics` | convex, triangle-mesh and compound colliders; layer-filtered queries with surface ids; rigid bodies and character controller (§6) | rapier3d | built |
| 2 | `engine/destruction` → `engine-destruction` | structural graph, load solver, damage, collapse events, budgets (§7) | — | built |
| 2 | `engine/netcode` → `engine-netcode` | transports, loopback hub, validated tick inputs, handshakes, per-client replication state, prediction, interpolation, travel-epoch snapshots (§8) | postcard, quinn | built; soak measured |
| 2 | `engine/render` → `engine-render` | the renderer ([RENDERING.md](RENDERING.md)) | wgpu today; `engine-gpu` after vk-2/vk-3 | built: checked frame plan, retained scene, TAA, dynamic resolution, cooked meshes and textured PBR, LODs, per-pass timers, test-only `golden` feature |
| 2 | `engine/audio` → `engine-audio` | typed mixer buses, decoded one-shots, pooled spatial emitters, bounded voice admission (§11) | kira 0.12.6 | slice 1 built |
| 2 | `engine/animation` → `engine-animation` | skeletons, clips, blending, IK, state graph (§11) | — | planned |
| 3 | `engine/scene` → `engine-scene` | validated RON levels: greybox shapes, mesh instances, collision records, prefabs, volumes, layouts, cues; spawning and live edits ([CONTENT.md](CONTENT.md) §6) | ron, serde, glam | built |
| 3 | `engine/ui` → `engine-ui` | design tokens, screen stack, focus, documents, layout, text, widgets, motion, backgrounds, layout checks (§10) | taffy, parley, ron | built: v3 documents with CSS block/flex, retained layout, bindings, menus, gradients, browser conformance; images, clipping, scroll, font faces and safe areas open |
| 3 | `engine/devtools` → `engine-devtools` | profiler, network graph, entity inspector, destruction view (feature-gated) | egui | planned |
| 3 | `engine/sessions` → `engine-sessions` | rooms, room codes, resume tokens, session directory; pure and tick-driven (§8) | ring | built (not yet carried over the QUIC server) |
| 4 | `engine/app` → `engine-app` | game registration and start-up, fixed tick, client/server/headless runners, lobby and room lifecycle, match travel, extraction, retained scene presentation, asset uploader, telemetry, error banner | — | built (single-threaded client; threaded mode planned) |
| 5 | `kits/*` → `kit-*` | shared genre gameplay (§9) | — | `kit-fps` built |
| 5 | `tools/*` | `check-layers`, `ui-cook`, `bench-gate`, `cook`, `texture-encode` | gltf, meshopt, bevy_mikktspace, image, Basis encoder, html5ever, cssparser | built |
| 6 | `games/*/modes/*` | one crate per mode of a game | — | planned |
| 7 | `games/*/game` → game name | the game's client and server binaries | — | built: `sandbox`, `imagine` |

**Folder layouts** (each folder's `mod.rs` only declares, re-exports and documents): `tools/ui-cook/src/html/` (`dom`, `cascade`, `props`, `state`, `look`, `element`, `convert`, `containers`, `widgets`); `engine/app/src/asset_uploader/` (`uploader`, `queue`, `build`, `texture`, `mesh`, `state`, `types`, `client_assets`, `tests`).

**More rules.** Modes never depend on each other. `netcode` and `destruction` never depend on `render` or `audio`: presentation reads simulation state, it never owns it. A third-party library is used by exactly one wrapper crate and its types do not leak into other crates' public APIs (for example, Rapier's glam version is converted in one private `convert.rs`).

## 2. Simulation model

**Decision (built).** A deterministic fixed-tick simulation on hecs.

- **Fixed tick:** 60 Hz by default, a per-game setting (30 or 120 allowed) that never changes during a match. The simulation is `step(&mut World, &Inputs, tick)` with an explicit system order. Snapshots go out at 20–30 Hz.
- **No hidden state:** no wall-clock reads, no thread-local random numbers. Randomness is a per-match seeded `SimRng` held in the world.
- **Resources live in the world.** Non-entity state (the physics world, a match clock, a map) is a typed resource, one per type, stored in the `World`. Each resource writes its bytes into `World::state_hash` in insertion order, so the hash covers everything that affects a tick. Systems are plain functions `fn(&mut World, &mut SystemContext)`. **Rejected:** singletons and `static` state; per-system state outside the hash.
- **Determinism:** the same seed and inputs give the same `StateHash` on every platform we ship. Float use is confined to physics (Rapier's cross-platform determinism mode) and structural maths. The hash is asserted in tests and exchanged in debug builds.
- **Entities:** hecs (an archetype store: fast iteration, which is our hot work). Components are plain serde data registered once in a `ComponentRegistry` that also sets replication: `Replicated`, `OwnerOnly`, `ServerOnly`, `Cosmetic`. Every replicated entity has a stable `NetId`; `Entity` handles never cross the wire.
- **Commands are typed and validated.** A player command is a value that cannot hold an out-of-range input: axes are enums, view angles are the engine's validated `ViewAngles` (finite, yaw wrapped, pitch clamped). The view comes from the engine's mouse look: a presenter asks for the grab, and the engine turns `ViewAngles` from mouse or touch motion. **Rejected:** a game-side turn rate (a second view source); free floats the simulation must clamp.
- **Events (built):** systems tell each other what happened through typed per-tick logs (`World::add_events`, `send`, `events`), sized once from data. The schedule empties every log before a tick; reading never consumes, so readers run in any order, and a test runs every reader order and compares the results. No system clears another's log. This follows Bevy's events and Unreal's gameplay message router.
- **Rendering between ticks:** the client draws at display rate and blends transforms between the last two ticks.

**Why.** Fair online play, replays and desync detection all need a deterministic simulation; a value outside the hash can desync silently.

**Open (owner):** whether to drop cross-platform bit-identity for Rapier's SIMD path. Clients are corrected by server snapshots, so they never need bit-identity; measurements so far were on a loaded machine and do not settle it ([PERFORMANCE.md](PERFORMANCE.md) §5).

## 3. Threads and the frame pipeline

**Decision.** Four kinds of thread, one job pool sized per device, deterministic by construction.

| Thread | Runs |
|---|---|
| Main | window and input events (winit needs it on macOS and iOS), the fixed tick, extract |
| Render | records and submits the GPU frame; owns every GPU resource (planned: threaded mode) |
| Job pool (rayon) | data-parallel loops inside a step: physics, structural solve, snapshot building, culling, light binning |
| Background pool | file loading, decompression, shader preparation; lower priority, never waited on by a frame |

- **Pool size from the device profile:** desktop logical cores − 2 (at least 1); phones performance cores − 1, because efficiency cores and heat make extra workers slower. The performance-core count is estimated from per-core frequency (≥ 75% of the maximum; every core counts where the OS reports nothing), with a per-device override in settings. Threads are never pinned; the OS places them.
- **Deterministic parallel code:** work splits into fixed-size chunks (for example 256 entities), never one piece per thread; each chunk writes its own slot and slots combine in index order on one thread; float `sum`/`reduce` across threads is banned in simulation crates by clippy `disallowed_methods`; a test runs a match with 0, 1 and all workers and compares the hash every tick. Rapier's parallel solver gives the same result at any thread count.
- **`engine-jobs` is the only crate that names rayon;** Rapier receives the same pool.
- **Pipelined frame:** the render thread draws frame N while the main thread simulates N+1. **Extract** copies what drawing needs (transforms of the last two ticks, changed instances and materials, destruction changes, effect events) into pre-sized arrays with no allocation, under 0.5 ms on desktop and 1 ms on the phone. The game thread is never more than one frame ahead.
- **Late camera:** just before recording, the render thread applies the newest mouse or touch movement to the view; the command sent to the server carries the same angles, so pipelining adds no aim delay.
- **Single-threaded mode** runs extract and render back to back on the main thread: for few-core devices, tests and debugging. Servers and headless runners have no render thread. Built today: the single-threaded client (`ClientApp`) and the frame exchange; the threaded mode is next.

**Why.** Unreal's task system, Unity's jobs and Godot's worker pool are the same shape (a pool with priorities); what we add is that determinism comes from how work is split, not from a special compiler. A separate render thread is standard. **Rejected:** fibers (need `unsafe` stack switching); a job graph with waits inside jobs (Unreal reports stalls); our own scheduler (rayon is one); tokio for frame work (built for I/O); render code reading the live world (races); a third API-translation thread like Unreal's RHI thread.

## 4. Start-up, platforms and content

**One launch order (built).** `engine-platform` starts the OS context and detects the device; `engine-app` opens and mounts the game's pack, builds the game, then hands the client to the platform loop. The `game_main!` macro emits the desktop and NativeActivity entry points. Headless start-up uses the same content source and mount sequence.

**Platforms.**

| Device class | Systems | GPU API (through `engine-gpu`) | Threads | Transport |
|---|---|---|---|---|
| Desktop | macOS, Windows, Linux | Vulkan on Metal (KosmicKrisp); Vulkan; Vulkan | pipelined | QUIC |
| Phones, tablets | Android; iOS later | Vulkan 1.1+; native Metal backend later | pipelined | QUIC |
| Dedicated server | Linux | none | no render thread | QUIC |
| Consoles | — | a future `engine-gpu` backend under NDA | — | platform |

- **The device profile** holds class, core counts, memory budget, GPU capabilities and limits, and the known-bad list match. Every crate reads the profile; nothing checks the OS by name. `target_os` appears only in `engine-platform`.
- **Only `engine-gpu` names Vulkan; only `engine-render` and `engine-platform` use `engine-gpu`; only `engine-platform` names winit.**
- **Not supported:** WebGL2 and OpenGL ES (no compute, no HDR; Godot keeps a whole separate renderer for them). The web is dropped until a game needs it.
- **Android lifecycle:** a lost or zero-size surface is a state, not an error. When Android revokes the surface, the platform drops renderer resources and rebuilds them when a new one arrives; the simulation world stays alive.
- **Android `unsafe` exceptions** (each with a `// SAFETY:` comment and a test): the exported `android_main` symbol (NativeActivity resolves it by exact name; emitted once by the entry macro); `__android_log_write` through `dlopen`/`dlsym` so an entry-point error reaches logcat; the frame-rate declaration (`ANativeWindow_setFrameRateWithChangeStrategy`, else `ANativeWindow_setFrameRate`) once per surface session, recorded as a `PresentationBinding`. **Rejected:** `cargo-apk`, `log`/`android_logger` (new dependencies for one function each).

**One content pack per game (built).** Each game declares one validated `ContentName`. `engine-assets::PackReader` validates the table and every entry hash, then mounts logical `AssetPath` entries before the game is built. Desktop files, APK assets and headless tests differ only in how `engine-platform` opens the bytes. Game code never builds file-system paths for runtime content. In development, `cook --watch <game>` rewrites the pack and the platform reports its revision; a valid replacement reloads changed entry hashes; invalid data reports an error and keeps the last good value. APK packs are immutable. The pipeline that makes the pack: [CONTENT.md](CONTENT.md).

**The one allowed exception:** the engine's fallback font is compiled into `engine-app`, because it draws the error banner when a game's pack cannot be opened.

## 5. Spatial queries

**Decision (planned: `engine-spatial`).** Physics answers collision questions; one sparse cell grid answers "what is near".

- Rays, sweeps and overlaps go to Rapier's query pipeline through `engine-physics` (one path, the physics data).
- Everything else that asks "what is near here" uses one shared sparse grid: network relevance, coarse render culling, nearby sounds, AI perception, effect limits. Cells are cubic (16 m by default, set per world); only occupied cells exist, in a fixed-seed hash map reserved from the level bounds, so small arenas and large worlds share one path. An object is stored in every cell its bounds touch; objects larger than a set number of cells (terrain, skyscrapers) go to a "large objects" list every query checks. Moving objects update only when they change cells. Results come out sorted by key, so they are deterministic.
- The same cells are the unit for cooked static geometry, HLOD and streaming.

**Why.** Unreal's Replication Graph uses this kind of grid for relevance at Fortnite scale; a grid updates in O(1) and beats trees when most objects move. **Rejected:** a second BVH or an octree (slower updates); BSP and portals (assume walls never move).

## 6. Physics

**Decision (built).** Rapier 0.36 behind `engine-physics`, with Unreal's simple/complex collision split.

- **Shapes:** moving bodies and casts use validated convex shapes (including compounds of hulls); static level geometry and precise traces may use triangle meshes. Convex decomposition (VHACD) runs deterministically in the cook, never at run time.
- **Filtering:** every collider carries reciprocal interaction masks and a surface id; every query uses one layer filter, can exclude a whole body, and opts into sensors explicitly. Games define layer bits and map material names to surface ids. Character moves exclude the moving body's own colliders (a ray or sweep from a capsule would otherwise hit itself).
- **Glam boundary:** Rapier uses glam 0.33, the workspace 0.30; one private `convert.rs` holds every conversion.
- **Features:** default `cross-platform-determinism`; opt-in `simd`; mutually exclusive.
- **Allocations accepted and bounded:** Rapier allocates a few small buffers per step in its narrow phase; at most 3 allocations / 640 bytes per warmed step on the test scene, enforced by `engine/physics/tests/no_alloc.rs`; our own physics code stays at zero. Moves that hit dynamic bodies may allocate contact buffers (measured at most 8 / 1,280 bytes per 64 moves). Reopen if a phone profile shows allocator pressure. **Rejected:** forking or vendoring Rapier.
- **Gravity** changes (low-gravity rounds, jump pads) are settings and impulses.

**Why.** Rapier is the most mature Rust physics library; triangle meshes keep authored static geometry precise while convex runtime shapes keep characters stable. **Rejected:** Avian (tied to Bevy), Jolt (C++, `unsafe` bindings, a second build system), casts against triangle meshes, runtime decomposition, separate query APIs per filter combination.

## 7. Destruction

(Built: `engine-destruction`. Server-authoritative and hierarchical.)

1. **Structure:** a building definition (materials, elements, connections, rooms) cooked from Blender tags.
2. **Graph:** elements have mass, strength and damage; edges carry load. Removing or weakening an element re-solves load only for the affected connected part, giving progressive failure. A fixed per-tick solve budget with a work queue bounds spikes.
3. **Fracture:** pre-cut chunk geometry per element; the damage impulse picks the pattern; distant objects use coarser patterns.
4. **Debris:** gameplay debris (large pieces that block, cover or crush) are capped, server-simulated, replicated rigid bodies that freeze into rubble. Cosmetic debris is client-only, seeded from the replicated event, never sent.
5. **Environment:** fire, smoke, gas and water as coarse grid fields with per-tick budgets; chain reactions are explicit rules.
6. **Consequences:** events (`ElementBroken`, `ChunkDetached`, `RoomOpened`) feed navigation, objectives, AI, audio and rendering (light-cache invalidation, shadow tiles, dust).

Replicate events and state changes (element health bands, collapse state, gameplay debris transforms), never particles. Every collapse has a warning phase (creak, dust, tilt) of at least a configured duration; this is data and is tested. `DestructionBudget` caps solves per tick, active gameplay debris, cosmetic debris per client and environment cells per tick; quality tiers change only cosmetic budgets. **Rejected:** physics-only collapse without a structural solver (not deterministic, cannot be replayed).

## 8. Online play

**Decision (built).** Our own netcode over QUIC, server-authoritative, following Unreal's replication model.

- **One command path.** `Presenter::command` turns input and view angles into the game's validated, serialisable command; it does not mutate the world. Standalone play is one session with `PlayerId::LOCAL` that joins once and applies commands through the same `Game::command` as network play. An online presenter sends the command to `NetClient`. Commands carry intent, not outcomes: a request like `Trigger::OnTarget` is evaluated against the authoritative world at the command's tick.
- **Transport:** `quinn` (QUIC) behind a `Transport` trait: reliable streams for lobby, RPCs and events; unreliable datagrams for inputs and snapshots. `ServerTransport` is implemented by `QuicServer` and by `LoopbackHub`, the in-process version that injects latency, jitter, loss and reordering, so every netcode test runs without sockets. One network thread per endpoint with a single-thread tokio runtime; the game thread only moves pooled buffers and never blocks. Clients pin the server certificate's SHA-256 (self-signed at start, handed out by the lobby); there is no accept-anything mode. The datagram cap is 1,150 bytes (QUIC guarantees 1,200-byte payloads), and a path that cannot carry it is refused at connect.
- **Prediction and interpolation:** clients send tick-stamped inputs, repeated over the last few ticks to survive loss; the client predicts its own pawn with the same `step` code and corrects on disagreement; others are shown about 2 ticks in the past.
- **Lag compensation:** the server keeps a short hit-box history and rewinds to the shooter's view tick, up to a maximum.
- **Replication (after Unreal):** per-client component shadow state; complete changed component values are sent until acknowledged; game callbacks give relevancy, priority and update frequency; dormant entities are skipped until they change; reliable ordered events have their own sequence queue. Transforms use game-bounded millimetre quantisation (up to 24 bits per axis) and smallest-three rotations. Per-entity records live in slots indexed by `NetId`, and packet size is tracked as it fills, so a write is linear in entities.
- **Wire format:** postcard with a versioned handshake; every message has a size cap checked before parsing and a fuzz test.
- **Sessions** (`engine-sessions`, after Unreal's `AGameSession`): `Room<S>` holds seats (humans and bots), teams, ready flags, the host, admission (open, approval, closed) and the lobby → countdown → match → results cycle; `SessionDirectory<L>` lists rooms by code. `S` and `L` are the game's types, so the engine never names a mode or map. Commands apply one at a time; illegal ones are typed errors; changes come out as events and clients see a token-free `RoomView`. Room codes are 6 characters from a 32-letter alphabet without I, O, 0 and 1, typed leniently. Resume tokens are 32 bytes from the OS, compared in constant time, never printed. The host is always a human while one is seated and migrates to the earliest connected seat. A drop in the lobby frees the seat; from the match on the seat waits for a resume. A countdown exists only while its start rule holds. Not built: lobby messages over the QUIC server, parties, matchmaking, bans.
- **Match travel** replaces the world as one deterministic operation: a fresh world and schedule are built and swapped in only after setup succeeds; a `u32` epoch increments and derives the world seed; snapshots carry the epoch so packets from an old world are ignored. The server applies game-derived settings (such as bot seats) to the room before starting, so clients receive exactly what the server used. Standalone travel requests from a presenter are applied at the client frame boundary; online travel is server-owned. A failed standalone travel keeps the current world, shows the error banner and calls the presenter's recovery hook.
- **Anti-cheat basics:** the server validates every input (rate, size, impossible movement), never trusts client hit claims, rate-limits RPCs.

**Why.** Prediction, snapshots and lag compensation have to fit the deterministic simulation and destruction events. **Rejected:** lightyear (built for Bevy); plain state sync without per-client state (resends unchanged data); byte deltas against a client-held baseline (fragile across lost packets); renet2 (the fallback if quinn's datagrams disappoint).

## 9. Kits and modes

Kits are optional; a game takes what it needs and the sandbox takes none. A kit holds gameplay more than one game of a genre could use, with every number in the game's data.

- **`kit-fps` (built):** locomotion (walk, sprint, crouch, slide, air, mantle as one movement state with per-tick events), health and damage with distance falloff, a hitscan weapon, pure aim slowdown (never magnetism; shots follow the view), bots on a validated waypoint graph. Players and bots drive the same `MoveCommand` through the same systems, so a bot can do anything a player can and no more. Configs decode through `serde(try_from)` so saved or networked values cannot skip checks, and can be replaced while preserving live state. Kit errors reach the schedule as `WorldError::Downstream`. The combat path allocates nothing across 600 warm ticks.
- **Bot routing:** the waypoint graph is linked by the kit from static geometry (knee and head rays across the capsule width; step and length limits); a graph that cannot reach every waypoint fails to build (`KitError::Disconnected`). A next-hop table built by breadth-first search makes routing a lookup per tick with no allocation. Bots steer to the next hop only when they can see it; they slow only near the final destination; a stuck bot picks a new waypoint from its own seeded stream. View turn rate is separate from aim-error settling. Limit: nearest waypoint by horizontal distance, so multi-storey routing is not solved. **Rejected:** a navigation mesh for greybox arenas.
- **Planned kits:** weapons (data-defined hitscan, projectile, area), combat (armour, downed and revive), gadgets, objectives, roles, AI (navigation rebuilt from the current structure).
- **Modes** belong to a game: a settings file choosing blocks and numbers, plus an optional crate with `ModeLogic` hooks through a typed event API. A new mode needs no engine change.

## 10. Presentation: UI, input and telemetry

**UI is a validated document (built).**
- **One format:** a screen is a `*.ui.ron` document: a tree of nodes with layout, style, `show_if`, and one content kind (text, bar, touch button, action, toggle, slider, choice, tabs, repeat). It is validated on load (ranges, unique widget ids, focus and back references, bindings, state styles, unknown fields).
- **Authoring in HTML and CSS:** `tools/ui-cook` compiles a subset (tags `div`, `p`, `span`, `progress`, buttons, inputs, selects, tab lists, repeated templates; tag, class, id and widget-state selectors; flex, absolute, colour, radius, border, opacity, scale, skew, linear gradients, vmin) into the document. Anything outside the subset is a named error. A shared user-agent stylesheet (`ua.css`) gives CSS block and flex defaults; documents use border-box sizing.
- **Layout is CSS's own algorithms through taffy,** and is checked against a real browser: headless Chrome through the DevTools Protocol renders every screen at 1280×720, 1920×1080 and 2800×1280, and the test compares rectangles. Logical `px` is `short_side / 720` physical pixels, so one file fits 16:9 and 20:9.
- **Data flows one way:** the game fills `Bindings` (numbers, flags, text, typed record lists) each frame; templates, widget values and `show_if` read them; a missing or mistyped variable is an error. Widgets emit typed events; the game owns all actions.
- **Retained:** the layout tree is built at load; unchanged frames return the cached draw list; paint-only changes skip layout. Steady menu frames allocate nothing.
- **Output is a draw list,** not draw calls: primitives plus clipped touch regions in painter's order from the same layout pass. Text uses measured intrinsic size; fixed text shrinks to 75% then ellipsizes; `check_text_overflow` and `check_interactive_bounds` reject text outside its box and controls outside the safe viewport.
- **Screens stack in layers** `Game < GameMenu < Menu < Modal < Toast`; only the top screen of the highest input-taking layer has focus; Back pops it; toasts never take focus. `Menus` routes pointer, touch and keyboard input, restores focus, gives each repeated row its own focus target, and runs transitions from theme motion tokens without delaying input (reduce motion: a 120 ms fade).
- **Theme as data:** each game's theme file is the single source for colours, type, spacing and motion.
- **Text:** parley shaping with MSDF atlases baked offline (planned; fontdue today).

**Rejected:** a browser engine or webview (heavy on phones); hand-written UI calls per screen; egui, iced, Slint or vello for player UI (egui is for developer tools only); CSS inheritance beyond specificity.

**Touch input hit-tests what was drawn (built).** `engine-core` owns `Viewport`, `ScreenRect` (created only by clipping a finite rectangle to a finite viewport) and `TouchRegion`. The UI emits at most 8 regions from the same layout it draws; `Input` installs them only after the draw succeeds, picks the topmost region containing a new touch, and carries a held region by id when the layout moves. A region may declare `drag_look`, so a held fire button also turns the view. The first frame has no regions, so touches on it hit nothing. UI layout, binding, reload and drawing failures are presentation errors: the client logs each distinct message once, shows the banner, and replays the last good UI and regions.

**Gameplay input** (planned: input-actions): devices map to named actions through mapping contexts in data; games read actions only (Unreal's Enhanced Input). Look sensitivity is per axis, validated, with the presenter able to scale it (for example while scoped).

**Frame telemetry (built).** `Telemetry` keeps the last 240 frame times in a ring sized once; `snapshot` returns fps, mean, the 1% low (99th percentile, nearest rank), the maximum, the refresh rate derived from the median frame time (rounded to 30, 45, 60, 72, 90, 120 or 144 Hz; the OS does not report the compositor's real rate) and the mean simulation time. It takes times as numbers, so it has no clock. `draw_overlay` draws one panel for every game, amber when the 1% low exceeds one display interval and red at two. On touch the panel starts shown.

## 11. Audio and animation

- **Audio (built, slice 1):** `engine-audio` wraps kira 0.12.6: a typed bus tree, a listener pose, attenuation-profile spatial pools, deterministic pitch jitter, a priority-aware 32-voice cap. Occlusion from a ray per sound every few ticks. Headphone 3D sound (HRTF) is tested later behind the same wrapper. **Rejected:** Firewheel as the base (younger), oddio (inactive).
- **Animation (proposed):** our own runtime, about 2,000 lines, after ozz-animation's design (sample, blend and IK as separate steps): glTF skins and clips, blending, additive layers, two-bone and aim IK, a small state graph in data, GPU skinning, animation LOD. It also drives server hit boxes. **Rejected:** ozz-animation-rs (nightly Rust and C++ tools).

## 12. Testing, gates and developer tools

| Level | What |
|---|---|
| Unit | every pure module; property tests (`proptest`) for invariants, round trips, quantisation bounds |
| Determinism | N ticks twice and across a recorded input stream; equal `StateHash`; 0, 1 and all workers; across x86-64 and ARM64 |
| Netcode | loopback with latency, jitter and loss: prediction converges, lag-compensated hits match |
| Integration | per game: a headless server and scripted bot clients play a match; the sandbox runs headless every gate |
| Fuzz | every network decoder and file reader |
| No allocation | counting-allocator tests on per-tick and per-frame paths |
| GPU | Vulkan validation and sync validation on, any message fails; device runs via `scripts/android-test.sh` on both phones |
| Visual | PNG goldens per tier: `render::golden` behind the test-only `golden` feature is the one decode, compare and record path. A frame is rejected as uniform (luminance deviation below 0.25 levels or one colour over 99.9%); every golden states at least one content expectation (a luminance range or changed-pixel share for a region); `GOLDEN_RECORD=<pattern>` is the only recording switch and records only frames that pass |
| Performance | the bench and `bench-gate` ([PERFORMANCE.md](PERFORMANCE.md)) |

**Gates.** `scripts/gate.sh` is the one gate: each change checks fmt, layers, and clippy and tests for the crates it changed; `scripts/gate.sh --full` and `scripts/build-android.sh imagine` run once per merge batch on main. Dev builds keep line tables only (`debug = "line-tables-only"`, dependencies without debug info). **Why:** five parallel runs testing the whole workspace compiled it five times; full debug info was most of a 65 GB target folder. This follows Unreal's split of local preflight and farm build.

**Developer tools (planned):** `engine-devtools` (egui, feature-gated): profiler with named CPU scopes, network graph, entity inspector, destruction view, replay scrubber (replays are inputs plus the seed). Render view modes (lighting only, AO, light cache, shadows) are built into the frame plan.

## 13. Libraries chosen

| Job | Library | Rejected |
|---|---|---|
| Engine base | our own, from Bevy's libraries | switching to Bevy (breaks compatibility every 3–4 months; not built for a server-run deterministic game); writing everything ourselves |
| GPU | `ash` + `gpu-allocator` in `engine-gpu` | wgpu (hides tile controls), VMA |
| Window, gamepad | winit, gilrs | — |
| Maths | glam | — |
| Entities | hecs | sparse-set stores (slower to iterate), our own ECS |
| Physics | Rapier | Avian, Jolt |
| Jobs | rayon | fibers, tokio for frame work |
| Network | quinn, rustls (ring), postcard | lightyear; renet2 is the fallback |
| UI | taffy, parley, MSDF atlases; html5ever and cssparser in the cooker | egui, iced, Slint, vello, a browser engine, lightningcss (a full style engine where a tokenizer is enough) |
| Audio | kira | Firewheel, oddio |
| Content | gltf, meshopt, Basis Universal, bevy_mikktspace | per-platform texture packages |

Ideas taken from Bevy (design, not code): scenes as data with prefabs, asset handles with hot reload, plugin-style registration, binned render phases, required components (features imply inputs).

## 14. One path per concern

Each concern has one canonical path, the Unreal design it follows, and a state. A second path for any row is a defect: fix it at its cause.

| Concern | The one path | Unreal | State |
|---|---|---|---|
| Start-up | `GameEntry` + `game_main!`: platform, device, pack, then the game | `FEngineLoop` PreInit/Init | built |
| Platform differences | only inside `engine-platform` | `FPlatformMisc`, `IPlatformFile` | built |
| GPU access | only inside `engine-gpu` | RHI | planned (vk-1) |
| Content | one pack per game, read by `AssetPath` everywhere | pak / IoStore | built |
| Hot reload | desktop pack revisions reopen and reload changed hashes | editor package reload | built |
| Asset encoding | one versioned record envelope, validated by the owner crate at load | one package format | in progress |
| Surface class | cooked `SurfaceClass` picks the pass family | material blend mode | planned |
| Device and quality | profile → tier table → saved choice; every change through `set_settings` | Device Profiles, scalability | planned (device-tier) |
| Render frame | frame plan compiled to a render graph once per settings change | Render Dependency Graph | plan built; graph planned |
| Opaque depth | one invariant position function and instance bindings for prepass and base pass | `MakePrecise`, near-or-equal base pass | built |
| Indirect light | the light cache | Lumen (consoles); Lightmass (mobile) | planned |
| Gameplay input | named actions from mapping contexts in data | Enhanced Input | planned |
| UI input | screen input modes and actions on gameplay actions | CommonUI | planned |
| UI layout | HTML/CSS cooked to one document, laid out by CSS algorithms, checked against a browser | UMG | built |
| Touch hit-testing | regions from what was drawn, clipped | Slate hit-test grid | built |
| Presentation errors | banner, last good state kept, game never closes | `ensure` | built |
| Standalone and online | one command path, one session code | one GameMode path | built |

## 15. Technical risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Cross-platform float determinism | High | Rapier's deterministic mode; gameplay maths in a few audited modules; hash tests across platforms; reconcile from snapshots, never lockstep |
| Parallel code breaks determinism | Medium | fixed chunks, ordered combining, banned reductions, the worker-count hash test |
| Android Vulkan driver bugs, now ours to handle | High | validation and sync validation in every GPU test; both test phones; one GPU-family workaround table |
| Phone heat and memory | High | device-profile budgets, thermal-driven resolution, texture streaming, shorter visibility ranges |
| iOS and consoles need non-Vulkan backends | Medium (later) | every API call lives in `engine-gpu`, whose API names no Vulkan type |
| Light cache stale after destruction | High | local invalidation from destruction events, built with the cache |
| Pipelining adds input delay | Medium | late camera; single-threaded setting |
| Structural solve spikes | High | per-part solving, per-tick budget with a carry-over queue, worst-case bench |
| Destruction bandwidth | High | events plus changes, client-side cosmetic debris, relevance, byte budget per snapshot |
| Dependency churn (ash, Rapier, quinn) | Medium | pinned versions; one wrapper crate per library |
| Scope | Very high | each game's plan in milestones that end playable; engine work ordered by its test scenes |
| Placeholder art hides visual problems | Medium | test scenes use final-quality materials and lighting |
