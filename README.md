# Rust Game Engine

A 3D game engine in Rust for fast multiplayer shooters on phones and desktop. Its simulation is
deterministic, and it has its own renderer, physics layer, networking, UI system and content
pipeline.

> **Status: work in progress, paused.** Most systems are built and tested and the engine runs on
> macOS. The Android build compiles but has not been tested on a device. Development has moved to
> a new codebase; the [Findings](#findings) section explains why. The code is published as a
> reference and as a record of what was tried.

**At a glance**

| | |
|---|---|
| Language | Rust (edition 2024), WGSL shaders |
| Size | about 88,000 lines of Rust and 1,700 lines of WGSL |
| Tests | 907 unit and integration tests, plus property tests in 43 files |
| Dependencies | 531 crates in the full dependency tree |
| Platforms | macOS (runs); Android (builds, untested on device) |
| License | MIT or Apache-2.0 |

## Contents

- [Goals](#goals)
- [Core tenets](#core-tenets)
- [Architecture](#architecture)
- [Features](#features)
- [Getting started](#getting-started)
- [Making a game](#making-a-game)
- [Development process](#development-process)
- [Findings](#findings)
- [Not included](#not-included)
- [License](#license)

## Goals

The target was a first-person arena shooter whose maps change during a match, and which plays
well on mid-range phones. Four goals shaped the design:

- **Text instead of an editor.** Levels, tuning, UI and content are plain files. They can be
  diffed, reviewed and validated like code.
- **Phones first.** The engine has per-device memory and frame budgets, and a renderer that
  scales down smoothly. Touch is a first-class input, and the game keeps running when the OS
  pauses the app or takes its screen surface away.
- **Ready for multiplayer.** The simulation runs at a fixed 60 Hz and is deterministic, so a
  server can be authoritative and clients can predict and replay.
- **A platform, not a single game.** The engine knows nothing about any particular game. Each
  game is its own data plus a few crates on top.

## Core tenets

Every change was reviewed against these principles. When two of them conflict, the earlier one
takes priority.

1. **Fix the cause, not the symptom.** A bug fix should make the whole class of mistake
   impossible, not just correct one bad value. For example, a touch button that ended up
   off-screen once closed the game on a phone. Moving the button would have fixed that one case.
   The actual fix made layout run once per frame, tested touches only against what was actually
   drawn, and stopped drawing errors from ever closing the game.
2. **Make invalid states unrepresentable.** Types should prevent bad data from existing at all.
   Data is checked once, when it is loaded or compiled, and never again every frame. If valid
   input can still cause a run-time error, that counts as a design bug.
3. **One way to do each thing.** There is a single path for start-up, content, input, rendering,
   UI and error reporting. There are no duplicates, compatibility shims or half-built stubs. A
   feature that is not finished is left out.
4. **Simplicity.** Use the fewest moving parts that are still correct. A module should be
   understandable from its header comment and its public items alone.
5. **Small files.** Each file has one responsibility, about 300 lines, and 500 at most.
6. **Engine and game stay separate.** Engine code never refers to a specific game, mode, map or
   weapon.
7. **Players never see a crash.** Problems in drawing, input, assets or the app lifecycle are
   reported, and the game continues from its last good state.
8. **Measure everything.** Performance claims need a benchmark or device number. Correctness
   claims need a passing test.
9. **Feel before speed.** Build what the player experiences first, and optimise only when a
   measurement shows a need.

Tooling enforces these where it can:

- The lints reject `unwrap`, `expect`, `panic!`, indexing that can panic, and lossy casts.
- `unsafe` is off by default.
- Simulation code cannot read the clock, use global state, or use unseeded randomness.
- Hot paths have tests that count memory allocations.

## Architecture

```
engine/    game-agnostic crates
kits/      optional gameplay shared between games (first-person shooter kit)
games/     one folder per game; only the sandbox demo is included
tools/     content cooker, UI compiler, texture encoder, layer checker, benchmark gate
scripts/   quality gate, Android packaging, new-game template
```

Crates are arranged in layers, and each crate may only depend on lower layers. The layer checker
(`cargo run -p check-layers`) fails the build if a crate depends upwards, if engine code refers
to a game, or if one game depends on another.

| Layer | Crates |
|---|---|
| Core | `engine-core`: fixed tick, seeded RNG, state hashing, errors |
| Foundations | `engine-jobs` (ordered parallel work), `engine-world` (entities with stable network ids, transforms, schedule) |
| Systems | `engine-physics`, `engine-netcode`, `engine-destruction`, `engine-assets`, `engine-scene`, `engine-sessions`, `engine-ui`, `engine-audio` |
| Platform and rendering | `engine-platform` (window, input, GPU setup, device detection), `engine-render` |
| Host | `engine-app`: the runtime a game plugs into |
| Kits | `kit-fps`: movement, health, weapons, bots |
| Games | `games/sandbox`: demo and template |

Main libraries:

| Area | Library |
|---|---|
| Graphics | `wgpu` |
| Windowing | `winit` |
| Physics | `rapier3d` |
| Entities | `hecs` |
| Maths | `glam` |
| Audio | `kira` |
| UI layout | `taffy` |
| Content cooker | `gltf`, `meshopt`, Basis Universal |
| Data and network formats | `serde`, `ron`, `postcard` |

## Features

| System | State | Summary |
|---|---|---|
| Core, jobs, world | Built, tested | 60 Hz fixed tick, seeded RNG, cross-platform state hashes, stable network ids |
| Physics | Built, tested | Deterministic layer over Rapier. The character controller handles stairs and slopes; a move that would end inside geometry is rejected |
| Rendering | Built | HDR forward+ renderer: tiled lights, cascaded sun shadows, ambient occlusion, bloom, auto exposure, tone mapping, TAA or MSAA by tier, dynamic resolution. The render graph is validated before GPU setup, and shaders hot reload |
| Platform | Built, tested | Window, keyboard, mouse, touch. The GPU surface can be lost and restored while the game runs. Quality tier is chosen per device |
| App host | Built | Headless and windowed runners, late camera update so aiming has no added lag, performance overlay, an error banner instead of crashes |
| Scenes | Built, tested | Levels and tuning in validated text files, with live reload; a broken edit keeps the old scene |
| Assets | First version | glTF to a binary content pack: optimised meshes with LODs, GPU-specific texture formats, deterministic incremental output |
| UI | Built, tested | Screens written in an HTML/CSS subset, compiled to data, laid out with flexbox, live reload. A typical HUD frame takes about 0.5 µs and does not allocate |
| Networking | Early, tested | Swappable transports, simulated lag and packet loss, redundant inputs, versioned message format, QUIC. No replication or client prediction yet |
| Sessions | Built, tested | Rooms, seats, teams, join codes, reconnect tokens, room listing |
| Destruction | Built, tested | Structural graph with a load solver that decides when pieces collapse. Not yet connected to rendering or physics |
| Audio | First version | Mixer buses and pooled 3D sound effects |
| Shooter kit | Built, tested | Walk, sprint, crouch, slide, mantle, buffered jumps, health, weapons with spread and falloff, bots with perception and routing |

**Not built yet:** multi-threaded frame pipeline, replication and prediction, a dedicated server,
iOS and web builds, texture streaming, and a device-tested Android build.

## Getting started

Requirements:

- A recent stable Rust toolchain (edition 2024).
- The desktop client currently runs on macOS.
- Android builds also need the Android SDK (platform 36), the NDK and Java.

The sandbox's content pack is generated rather than committed. Build it first, then run:

```bash
cargo run -p cook -- sandbox
```

```bash
cargo run -p sandbox
```

Controls: W/S or Up/Down move forward and back, A/D strafe, Space jumps, the mouse looks
around, and F3 toggles the performance overlay.

Run all checks (formatting, layering, lints, tests):

```bash
scripts/gate.sh --full
```

Build an Android debug APK (untested on a device):

```bash
scripts/build-android.sh sandbox
```

## Making a game

A game implements the `Game` trait from `engine-app`:

- `setup` registers its components and systems.
- `join` adds a player.
- A presenter turns the world into things to draw, including the HUD.

The game's levels, tuning, UI and models come from its own content pack. To start a new game from
a copy of the sandbox, run `scripts/new-game.sh NAME`.

The sandbox is the smallest complete game: boxes, ramps, stairs and a crate stack to move around
in. Its tests replay scripted input and check two things: that movement is correct, and that the
same input always produces the same state hash.

## Development process

Most of the implementation was written by AI coding agents working from design briefs. Design and
code review were done by hand. The workflow:

1. Each task got a short brief: its purpose, the files it may touch, the invariants it must keep,
   and the end-to-end definition of done.
2. Tasks whose files did not overlap ran in parallel, each in its own git worktree.
3. Every diff was reviewed against the core tenets before merging.
4. `scripts/gate.sh` checked formatting, layering, lints and tests for the changed crates. The
   full workspace was checked before each batch of merges.
5. Design decisions were written down along with the rejected alternatives, so they were not
   reopened later.

For correctness, this worked well. The codebase is consistent and heavily tested, and the lints
and layer checks caught many mistakes before review. Speed was another matter, as the next
section shows.

## Findings

### Build times limited how fast work could go

Measured on an Apple M5 with 10 cores, development profile with optimised dependencies:

| Measurement | Result |
|---|---|
| Clean build of the full game | 2 min 34 s wall time, 907 s CPU time |
| Crates compiled from source | 531 |
| Build directory per git worktree | 10–17 GB |

The work ran as a loop of change, build, test, so every task paid this cost again and again.
Parallel tasks each needed their own build directory, and that filled the disk more than once.

### Building every subsystem from scratch took most of the effort

The renderer, the UI layout engine, the content cooker and the networking were all written for
this project. Each one needed its own design, tests and performance work. The renderer alone is
about 15,000 lines and still lacks features that mature renderers have, such as image-based
lighting and texture streaming. Most of the effort went into infrastructure rather than gameplay.

### A C++ prototype built from existing libraries

To find out whether the build model was the bottleneck, the same arena was rebuilt in C++ from
mature libraries:

- a prebuilt physically based renderer;
- a physics library with a character controller;
- SDL3 for the window and input;
- an entity-component system.

The prototype reached the same gameplay within a single working session: the arena, first-person
movement, the rifle, six bots, the HUD and touch controls, plus headless tests.

| | This engine | C++ prototype |
|---|---|---|
| Project code | ~88,000 lines | ~3,200 lines |
| Renderer and physics | Written for the project, compiled from source | Prebuilt libraries |
| Compiled on a full build | 531 crates | Project code and the physics library |
| Simulation cost per tick | not compared | 0.01 ms |
| Frame rate on the M5 | not compared | 60 fps (vsync) |

The prototype compiles only project files, so the loop from edit to running game is much shorter.

### What carried over

These ideas held up and moved to the new codebase:

- determinism with state hashes;
- one path per concern;
- checking data once, at the boundary;
- one content pack on every platform;
- touch hit-testing against drawn regions;
- a runtime that never lets a drawing error end the game;
- content as plain text.

### What changed

The new direction builds the engine from mature libraries for rendering, physics and the platform
layer. Project code is limited to gameplay and the glue between libraries, and every change has
to show up in seconds.

## Not included

- **Assets.** No art, audio, images or video. The only binary files are three Roboto fonts
  (Apache 2.0) and one small texture used by a test; the code compiles both in.
- **Content packs.** They are generated by the cooker.
- **The game built on the engine,** along with its assets and design documents.

## License

Dual-licensed under MIT or Apache-2.0, at your option. See `LICENSE-MIT` and `LICENSE-APACHE`.
