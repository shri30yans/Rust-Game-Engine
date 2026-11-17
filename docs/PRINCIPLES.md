# Principles: what we build, why, and how we decide

This file is the reasoning behind every other doc: the goal, the first principles that follow from the hardware, the design rules, how a decision is made and recorded, and the quality bar every feature is judged against. The engineering tenets themselves (fix the cause, correct by construction, one path, small files, measured not claimed) are canonical in [../AGENTS.md](../AGENTS.md) and are not repeated here.

## 1. The goal

**An engine that is a platform for many games, makes Unreal-quality 3D on phones at 60–120 fps, and is fast to make games on.**

- **A platform.** The engine knows nothing about any game. Games are data and their own crates on top ([ARCHITECTURE.md](ARCHITECTURE.md) §0).
- **Phones first.** The two test phones are a realme RMX5085 (Mali-G720, 2800×1280) and a realme GT 6T RMX3853 (Snapdragon 7+ Gen 3, Adreno 732). A feature is judged on them, warm, after 30 minutes. The dev Mac (Apple M5) is the desktop reference.
- **The reference workload is fast outdoor action:** aircraft, missiles and tracers, vehicles, large terrain with vegetation, explosions and smoke, many networked entities, continuous streaming. The engine's five test scenes cover these ([PERFORMANCE.md](PERFORMANCE.md) §3). One game's plan (IMAGINE's arena shooter) orders only that game's work.
- **Highly optimised end to end.** We own the GPU layer (our own Vulkan layer, [RENDERING.md](RENDERING.md) §2) so nothing between our frame and the hardware hides a cost.

## 2. First principles from the hardware

These facts decide most of the design. Evidence: [research/2026-11-17-render-mobile.md](research/2026-11-17-render-mobile.md) and [research/2026-11-17-render-deep-dive.md](research/2026-11-17-render-deep-dive.md).

1. **A phone GPU is limited by memory bandwidth and heat, not arithmetic.** Mali, Adreno and Apple GPUs render the screen tile by tile in fast on-chip memory. Every byte written to main memory and read back costs power, and a hot phone throttles within minutes. So: keep pixels on the tile, store nothing nobody reads, use compressed textures and small formats, and render at about 60–70% resolution with a temporal upscale where the tier allows.
2. **The sustained budget is about 11 ms of GPU per 16.7 ms frame.** The rest is thermal headroom. At 120 fps the whole frame is 8.3 ms, so the GPU gets about 6 ms.
3. **Decisions made once beat clever work every frame.** The largest measured phone wins in the research all came from choices made once, at cook or device bind: load and store operations derived from the frame graph (about 300× fewer memory writes in Godot), merged subpasses (55% fewer tiles), specialisation constants (about 7 ms saved). Each beats anything Nanite or Lumen measurably delivered on a phone.
4. **The famous desktop techniques cost most on a tiler.** Nanite's visibility buffer and compute shading, Lumen's per-frame tracing and two-pass occlusion culling all force data out of tile memory. We take their ideas (cluster culling and LOD, cached lighting) and leave the parts that fight the hardware.
5. **Most light does not change from frame to frame.** Visibility between surfaces is the expensive part of bounced light, and for static geometry it can be computed once at cook. Light itself stays dynamic.
6. **The CPU has idle cores; the GPU is the hot part.** Work that can move to a job thread (culling, light binning, the light-cache solve) moves there.

**What makes a frame look "Unreal"**, in order of effect per millisecond: correct light units, exposure and tone mapping; soft, stable shadows with contact shadows; bounced light and darkening in corners; reflections that match the room; real materials; atmosphere; temporal stability (no shimmer, no crawl). Ray counts do not.

## 3. Design rules

Each rule below follows from the tenets and the first principles. Where a rule is specific to a system, that system's doc states it in full.

1. **Decide once, at the boundary.** Content is classified and validated at cook; device capabilities are read once at device creation; settings are compiled once into a frame plan. A frame never re-checks or re-decides. Examples: a material's opacity class ([RENDERING.md](RENDERING.md) §7), a texture's block size by role (§9), the render graph (§3), the quality tier ([RENDERING.md](RENDERING.md) §15).
2. **One canonical path, with tiers as data.** There is one renderer, one content path, one start-up order, one input path. Quality tiers and device capabilities choose numbers and specialisation constants inside that path; they never add a branch for a platform, a build type or a game. [ARCHITECTURE.md](ARCHITECTURE.md) §14 lists the one path for each concern.
3. **When in doubt, follow Unreal.** For a design question these docs do not answer, we follow how Unreal does it, adapted to Rust and to our tenets, and we say which Unreal system we follow and what we changed. Unreal is a reference for structure, not for its phone renderer's limits.
4. **Own the layer that hides the cost.** We use well-made libraries (Rapier, taffy, parley, kira, quinn, hecs, glam, gpu-allocator, meshoptimizer, Basis Universal), each behind one wrapper crate. We own a layer only when a library hides a cost we must control: the GPU layer, netcode, the simulation model, UI documents.
5. **If a designer would tweak it, it is data.** Numbers, layouts, levels, events, effects, sounds, UI and text never live in Rust. Rust holds rules and systems, in small crates so a change rebuilds little. There is no scripting language. Data is validated on load and hot-reloaded at a frame or tick boundary, keeping the last good value ([CONTENT.md](CONTENT.md) §1).
6. **Presentation never closes the game.** A bad UI document, asset, shader or OS lifecycle event reports the problem and keeps the last good state. Only an unrecoverable simulation or GPU failure stops the client.
7. **Every claim has a number.** Performance claims come from the bench on the test phones or the M5; correctness claims from a passing test. Research findings are labelled measured, claimed or inferred, and an inferred number is a target until our bench measures it ([PERFORMANCE.md](PERFORMANCE.md)).
8. **Player experience first, then performance.** Build what the player sees and feels; optimise after a measurement shows the need. When goals conflict: how it plays, then how readable it is, then how it looks.
9. **Fast to make games, hard to get wrong.**
   - Defaults that look good: a level gets sun, sky, exposure, shadows, the light cache, reflections and anti-aliasing with no setup; the same rule holds for materials, UI themes and audio mixes.
   - One authoring path: Blender is the level editor, and saving updates the running game live.
   - Material assets with triplanar mapping, so blockout boxes look finished without UVs; later a small CC0 material library ships with the engine.
   - A new game in one command (`scripts/new-game.sh NAME`) with a playable level, HUD, menus, default lighting and the phone build.
   - UI as HTML and CSS, previewed live and validated against a real browser.
   - Built-in diagnosis: render view modes (lighting only, AO, light cache, shadows), the frame-time overlay, and a phone capture script.

## 4. How a decision is made and recorded

There is no separate decision log. **A decision lives in the doc that owns its topic**, written as the current design, with its reasons beside it:

- **Decision:** what we do, stated so a reader can build it.
- **Why:** the reason, with numbers where they exist.
- **Rejected:** the alternatives and why they lost.
- **Sources:** links, collected by topic in [SOURCES.md](SOURCES.md).

A changed decision is edited in place. The old version lives in git history, not in the doc. A decision waiting for the owner is marked **(proposed)**; everything else is decided. Research reports in [research/](research/) are dated evidence, not the source of truth: when one changes the design, the owning doc is updated and the report stays as it was.

Code comments and briefs written before 2026-11-17 cite old decision numbers (D1–D83). [README.md](README.md) §7 maps each number to the section that now holds it.

## 5. The quality bar

This checklist applies to every game on the platform. Each line is something players notice and has a way to check it. A feature is done when it meets its line, with the test or measurement attached. Each game can add measured targets of its own (IMAGINE: [REFERENCE.md](../games/imagine/docs/REFERENCE.md) §4).

**The biggest giveaways of a cheap-looking game**, in order: shimmering edges, flat lighting, plastic materials, objects floating with no contact shadow, empty and too-clean spaces.

### 5.1 How it plays

| # | Players notice | Target | How we check |
|---|---|---|---|
| G1 | Controls react instantly | input to screen ≤ 1 frame of game time plus the display; the shot fires on the frame of the click | input-latency test with a frame counter; high-speed video on desktop and phones |
| G2 | Movement is smooth and expressive | acceleration curves; sprint, slide, jump, mantle; grace windows near ledges and for early presses | movement test course with timed runs |
| G3 | The camera never jitters | fixed-rate simulation with the picture blended between ticks; no stutter at 120 Hz | frame-time graph; camera-path recording compared frame to frame |
| G4 | Weapons feel strong | kick, sound layers, flash, shell, tracer and impact on every shot; learnable recoil | the weapon effect checklist (§5.3) passes |
| G5 | Hits are always confirmed | marker, sound and reaction on the next frame (predicted); headshots differ | network test at 150 ms ping |
| G6 | Fights are fair online | server-decided hits with rewind; no rubber-banding; reconnect works | loopback tests with lag, jitter and loss |
| G7 | Damage is readable | you know where it came from and how hurt you are | playtest: "who shot you?" |
| G8 | Destruction is heavy and fair | warning before collapse (time set in data), weighty pieces, rubble that stays | destruction test scenes; the warning rule is a test |
| G9 | Time to kill is tuned | set per weapon in data and measured | numbers sheet generated from the data |
| G10 | Nothing breaks the flow | no loading pauses or hitches in play | 1% low frame time within budget during the worst scene |

### 5.2 How it looks

| # | Players notice | What it takes | How we check |
|---|---|---|---|
| V1 | Clean, stable edges | MSAA on tile or TAA with motion vectors; thin geometry handled at its cause | moving-camera reference scenes against a supersampled image |
| V2 | Light behaves like light | physical units, a bright sun, a sky that lights softly, bounced light | lighting test room against a Blender Cycles render |
| V3 | Soft, grounded shadows | sharp near, soft far, contact shadows, ambient occlusion | shadow close-up scene |
| V4 | Materials look like what they are | texture sets at real-world scale | material test board |
| V5 | Reflections match the room | relit reflection probes; never a blank sky in a closed room | reflection scene |
| V6 | Air you can see | height fog, light shafts, lingering dust, distance haze | atmosphere scene |
| V7 | Detail and wear | chipped edges, decals, props that tell a story | art review against the game's art direction |
| V8 | A consistent look | one art direction, one grade, one scale | grade check against reference frames |
| V9 | No popping or seams | distant detail fades in; lights never snap on | flythrough review |
| V10 | A good camera image | adaptive exposure, filmic tonemapper, subtle bloom | indoor-to-outdoor walk |

### 5.3 Effects

Every big effect is layered: an explosion is a light flash, fireball, smoke, sparks, debris, a scorch decal, camera shake, a low boom and lingering dust.

| # | Players notice | What it takes |
|---|---|---|
| E1 | Effects are lit by the scene | particles receive light and shadow |
| E2 | No hard edges at walls | soft particles fade near surfaces |
| E3 | Enough of them | thousands of particles without frame drops |
| E4 | They react to the world | sparks bounce, debris stays, dust drifts out of holes |
| E5 | Light from effects | flashes and fire light the surroundings briefly |
| E6 | Marks stay | decals persist under a cap, oldest fade first |
| E7 | Readable in a fight | an effect never hides an enemy for more than a moment, except smoke designed for it |

**Weapon checklist:** muzzle flash (sprite and light, 1–2 frames), smoke puff, shell, tracer, impact per material, impact decal, recoil animation, sound layers (close crack, mechanics, far boom, room tail). **Destruction checklist (per material):** crack decal stage, chunk break with weight, lit dust burst, lingering dust, small debris that stays, sound stages (creak, crack, break, rumble, settle).

### 5.4 Sound and UI

| # | Players notice | What it takes |
|---|---|---|
| S1 | You hear where things are | 3D sound, distance falloff, muffling through walls |
| S2 | Weapons sound powerful and distinct | layered shots, different tails indoors and out |
| S3 | Footsteps tell you who is near | per-surface steps, louder running, quieter crouching |
| S4 | The mix never turns to mud | buses, priorities, a voice limit, ducking |
| U1 | UI looks designed | real fonts, layered panels, one accent, motion on change |
| U2 | Text is sharp at every size | distance-field font atlases |
| U3 | It responds | motion and sound within 50 ms; nothing blocks input |
| U4 | The HUD reads at a glance | one number per element; the centre stays clear |
| U5 | Works with every input | mouse and keyboard, gamepad, touch |
| U6 | Accessible | contrast, text size, colour-blind safety, reduced motion |

### 5.5 Performance and stability

| # | Players notice | Target |
|---|---|---|
| P1 | Smooth frame rate | the tier budget holds in the worst test scene; on phones after 30 minutes warm |
| P2 | No hitches | no shader compile or file load in play; 1% low ≤ 1.5 × the mean |
| P3 | Fast loading | a map loads in under 10 s |
| P4 | It never crashes | no panics in engine or game code; fuzz tests on every decoder and file reader |
| P5 | The same result for everyone | deterministic simulation, also on many threads; a state hash catches differences |
| P6 | Fits the device | memory within the device budget; no crash after 30 minutes |

Budgets, benches and measurements are in [PERFORMANCE.md](PERFORMANCE.md).
