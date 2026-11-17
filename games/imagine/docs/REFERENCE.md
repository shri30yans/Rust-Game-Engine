# Reference: the quality bar from a THE FINALS gameplay video

What a top destruction shooter looks and plays like, measured from one video, and what we take from it. We take **the bar** (how dense, how fast, how good), never the content: no maps, names, characters, HUD layout, gadgets, branding or game-show setting are copied ([decision index](../../../docs/README.md) D1, D23).

Source: "The FInals Is The BEST FPS Out Now" by LevelCapGaming, 9:20, YouTube `FdxIsD0lafo`. The local copy (1080p60, 209 MB) and frames live in `reference/video/`, which is git-ignored and is for study only:

```bash
yt-dlp --no-playlist -f "bv*[height<=1080][ext=mp4]+ba[ext=m4a]" --merge-output-format mp4 -P reference/video -o "finals_levelcap.%(ext)s" "https://www.youtube.com/watch?v=FdxIsD0lafo"
```

The first minute was studied at one frame per second (`reference/video/first60/`). It is an edited montage: 15 cuts in 60 s, so each clip is 3–5 s of the best moments. Real matches have quieter stretches.

## 1. What the frames show

| Area | What we see | Why it works |
|---|---|---|
| **Places** | Six very different looks in one minute: a neon-purple tiled plaza at night, a rainy Mediterranean street with palms, a huge covered stadium with a crowd and cranes, a white-and-blue modern office, a red-and-white interior, a stone villa, a dark vault room, tiled rooftops | each map area has one strong colour identity, so you always know where you are |
| **Lighting** | soft overcast sky light that fills everything; wet floors with sharp reflections; saturated neon and signs bouncing colour onto walls; strong bloom on fire; dust hanging in the air catching light | light bounce (GI) and reflections do most of the "expensive" look; the base materials are fairly simple |
| **Materials** | clean, slightly stylised real-world surfaces: tile, plaster, concrete with rebar, glass, roof tiles, painted metal | readable at speed; detail comes from wear, decals and broken edges, not from noisy textures |
| **Explosions and fire** | big rolling fireballs with bright cores and dark smoke edges; flames that spread on the ground; sparks with streaks | layered: flash + fireball + smoke + sparks + light + shake, all at once |
| **Smoke and gas** | thick round billowing smoke that blocks sight; a coloured gas cloud | smoke has volume and is lit by the scene, not flat cards |
| **Destruction** | a whole building front falls after a breach charge; floors break and players drop through; holes punched in walls show rebar; glass shatters into many shards; rubble piles stay; small debris fades out as square "pixel" bits | destruction is the main event: big, physical, and it changes the route |
| **Movement** | always moving: sprinting, sliding, jumping through windows, dropping through holes, a zipline/grapple between rooftops, climbing onto ledges | the map is a playground in 3D, not a flat corridor |
| **Fights** | eliminations every few seconds in the montage; time to kill around a second at close range; third teams join fights; team-wipe bonuses | short fights, constant interruptions, always something to react to |
| **First-person view** | gloved hands, big weapon with a red-dot sight that fills the lower centre; throwing and placing gadgets has its own hand animation; the gun sways and kicks | the hands make every action feel physical |

## 1a. The first minute at 0.5 s steps (2026-11-11)

All 120 frames from 0 to 59.5 s, two per second, are in `reference/video/first60_half/` (`f_001.jpg` … `f_120.jpg`, plus ten 3×4 contact sheets `s_01` … `s_10`; sheet *k* covers seconds 6(*k*−1) to 6*k* − 0.5). Each row below groups the frames of one shot. The columns say what moves (animation), how it is drawn (graphics) and how it is built (design), and then **what our version of it is** (always original, D23).

| Time (s) | Animation | Graphics | Design | Our version |
|---|---|---|---|---|
| 0.0–2.0 | camera shake per blast; debris chunks fly on arcs and tumble; small debris breaks into square bits that shrink and vanish | night, saturated purple and blue; huge white-hot fireballs that roll into soft grey smoke; bloom around every flame | an open plaza with tiled walls; explosions chain one after another | layered explosion (flash, fireball, smoke, sparks, debris, light, decal, shake) per PRINCIPLES §5.3; our debris fades with its own dissolve, not square bits |
| 2.5–5.0 | holographic wireframe cubes spin and dissolve; smoke expands then slowly thins | emissive cyan lines over dark scenery; smoke lit from inside by fire | a respawn or teleport effect is shown as a light-construct | emissive "build-in / build-out" shader for spawns and deaths, our own shape language |
| 5.5–10.5 | gun bob while sprinting; ADS move to a red dot; enemy hit reactions | daylight stadium: soft sky light, sharp sun shadows, palm trees with wind sway, a burning roof with heat and smoke | rooftops connected at several heights; huge scale cues (crowd, cranes, signs) around the arena | sky light + baked bounce for the shell (PLAN §5.1); wind sway on foliage in the vertex shader; our arena gets its own skyline set-dressing |
| 11.0–15.5 | coins and confetti thrown up in arcs; a player falls through a broken floor | strong red and white room grade; glossy floor reflections | a reward moment is celebrated in the world, not only in the HUD | reward bursts as GPU particles (M3.6); one dominant colour per area (R6) |
| 16.0–24.0 | a wall blows outward into chunks; glass panes shatter into many shards that bounce; sparks fly in streaks | clean white and teal office; warm fire light spilling across cool walls; dust hanging in shafts of light | interiors full of breakable walls and glass, so every room has new routes | breakable walls and glass (D52); volumetric dust on High, cheap particles on Low |
| 24.5–31.5 | a gas cloud rolls and fills a room; foam blobs swell and harden; gray dummy figures fall | blue concrete hall; green gas glows; soft volumetric look | gadgets that change space: block a door, fill a room, hide | our own gadget set (PLAN §3.6, later season); never foam or the dome |
| 32.0–40.0 | **whole floors break**: slabs tilt, slide and drop, leaving rebar; dust rolls out of the hole and hangs for seconds | warm interior light; rubble piles with many pieces; dust lit by the sun | a building collapses as a planned set of large pieces, then becomes new terrain | structural collapse from `engine-destruction` (D52): server-decided large pieces, client-only small debris and dust |
| 40.5–45.0 | a breach charge sticks to a facade; the blast makes the **whole house tilt and fall in one piece**, chunks spray, dust plume rises 6–10 m | grey overcast daylight; dark dust against bright sky; rubble stays on the street | a player tool removes the support of a building; the result changes the map for the rest of the round | support-based collapse: when a building loses its supports it falls as linked chunks (R3) |
| 45.5–49.5 | a green gas cloud billows; an orange **dome** and a hex-grid **tunnel** shield flicker; an explosion throws a shower of sparks; coins spray; a player runs carrying a box | night, neon signs, orange energy shield with fresnel edges; hundreds of spark streaks with motion blur | a carried objective under fire; "steal" interaction prompt; assist callout `+100` | carry item and interaction (PLAN §3.2, §3.7); shield effect as fresnel + scrolling pattern shader in our own style |
| 50.0–53.5 | zipline travel with the gun lowered, one hand on the line; aim-down-sights with a red dot; enemy on a rope | bright stadium: giant painted wall, crane, tiled roof; clear long sightlines | vertical traversal between rooftops; objective diamond with distance in metres | grapple and zipline (M4.3); objective marker with distance (HUD_SPEC) |
| 54.0–55.0 | recoil kick per shot; floating damage numbers (31, 115, 220) at the target | hit sparks at the target; green crosshair hit flash | damage numbers + hit marker + elimination banner, all on the same frame | damage numbers (settings toggle), hit marker, elimination callout on the next frame (R9) |
| 55.5–57.0 | **weapon inspect** after the kill: the gun turns in the hands showing its camo | gold-and-black camo, clear-coat reflections | a reward pause between fights; cosmetic shown off in first person | viewmodel inspect animation (`engine-anim`, M3.7); cosmetics later |
| 57.5–59.5 | the hands reach and grab a zipline; the body swings; "Team wipe +500" | soft daylight, trees sway | a revive marker (skull) where teammates fell; interaction prompt `ATTACH` | revive marker and prompts (PLAN §3.7) |

**What the minute teaches, in one list:**
1. **Every second has motion in three layers:** the viewmodel (sway, recoil, inspect), the world (debris, smoke, foliage, movers) and the HUD (callouts, numbers, markers).
2. **Big events are built from many cheap layers**, not one expensive effect: flash, fireball, smoke, sparks, debris, dust, light, decal, sound and shake.
3. **Destruction is staged:** big pieces fall first, dust follows and hangs, rubble stays. The fall takes 1–2 s, the dust 6–12 s.
4. **Light carries the look:** sky light, sun, emissive signs and fire light do more than texture detail.
5. **Each area has one colour**: purple night plaza, red room, white office, blue hall, grey stone street.
6. **Feedback lands on the same frame** as the hit or kill.

## 2. The HUD in those frames (layout facts, not to copy)

| Place | Element |
|---|---|
| top-left | team ranks as a stack: rank number, coloured team icon, score |
| top-centre | match timer; a bright yellow bar when a map event is running, with its own countdown |
| top-right | kill feed |
| centre | elimination callout with the victim's name and "+points"; interaction prompts ("press key to …"); objective icon with distance and value |
| bottom-left | team name, squad list with small health bars, your own health as a large `250 / 250` numeral over a long segmented bar |
| bottom-right | weapon name, ammo `34 / 34`, gadget slots with counts and key numbers, key hints (Fire, Aim, Reload) |
| world | enemy health bars over heads after hits, teammate outlines through walls, red arrows for damage direction, a full red screen tint when badly hurt |

Our HUD has the same *jobs* (they are what every shooter needs) but its own shapes, colours and positions per [design/HUD_SPEC.md](design/HUD_SPEC.md).

## 3. What makes it interesting

1. **Destruction is a tool, not decoration.** You make your own door, drop the floor under an enemy, or bring a building down on an objective.
2. **Three or more teams.** Fights rarely stay one-on-one, so there is always a chance to swoop in or get swooped.
3. **One shared objective everyone fights over**, which pulls teams together.
4. **Map events** change the rules mid-match.
5. **Vertical movement** (grapple, zipline, ledges, holes) means threats come from every direction.
6. **Short loops of tension and payoff**: steal, defend, steal back.
7. **Every action is loud**: every hit, kill and blast has a callout, a sound and an effect.

We plan these as *ideas* ([PLAN.md](PLAN.md) §2–3): destruction as a player and Director tool (D52), Director events, zones everyone fights over, Carry item, the grapple, and the feedback rules in [PRINCIPLES.md](../../../docs/PRINCIPLES.md).

## 4. The bar we hold ourselves to (measurable)

| # | Bar | Target for our game | Check |
|---|---|---|---|
| R1 | Effects density | an explosion layers flash, fireball, smoke, sparks, debris, light, decal, sound and shake (PRINCIPLES §5.3) | effects_range test scene |
| R2 | Volumetric smoke | smoke blocks sight, is lit by the scene and lasts 10–20 s | atmosphere test scene; a line-of-sight test |
| R3 | Building collapse | a whole 2-storey front can fall in one go, with dust that hangs 6–12 s and rubble that stays | destruction_yard test scene; frame time stays in budget |
| R4 | Glass | panes shatter into many lit shards with sound | effects_range |
| R5 | Lighting | sky fill, bounce light and wet reflections good enough that simple materials look expensive (GX6, GX7) | lighting_room and reflections scenes vs. reference shots |
| R6 | Colour identity | every map area has one dominant colour theme you can name | art review |
| R7 | Pace | a fight starts within ~20 s of spawning; time to kill 0.8–1.5 s at close range | playtest timing; weapon numbers sheet |
| R8 | Movement | grapple, ledge climb, slide and drop-through all in the base | movement_course |
| R9 | Feedback | every elimination shows a callout and points on the next frame | PRINCIPLES §5.1 G5 |

## 5. What we never copy

Map layouts or places (the stadium arena, the specific towns), the game-show setting and crowd, the cash-and-vault loop, their gadget designs (the foam/goo, the dome shield), their HUD shapes and colour scheme, character designs, names, logos and sounds. If a reference shot is used in a review, it is used to compare *quality*, never traced.
