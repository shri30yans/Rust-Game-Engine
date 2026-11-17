# HUD spec

Units: logical px on the 640×360 reference canvas (see DESIGN_SYSTEM §6). At 1280×720 multiply by 2. All anchors are offset from the **safe area + 16 px gutter**. Mockup: [mockups/hud.png](mockups/hud.png), touch player view [mockups/touch-play.png](mockups/touch-play.png), touch spec sheet [mockups/touch.png](mockups/touch.png).

**Look (Work Site).** The HUD uses the Work Site kit at low weight ([UI_STUDY](UI_STUDY.md) §3): a **score stack** (status-lamp header + team rows on a left-to-right gradient fade, no closed boxes), **callout strips** (tab + slab + leader) for events and banners, **corner marks** for objectives and the selected target, **hazard stripes** only on collapse warnings, and big **outlined numerals** (72 px at 1280×720 = 36 logical) for health and ammo. Data glyphs and numbers are never skewed or textured (P-legibility).

These zones are the jobs every shooter HUD does (see [REFERENCE.md](../REFERENCE.md) §2). Our versions use our own shapes, colours and type (DESIGN_SYSTEM intro). Each mode turns pieces on or off ([PLAN.md](../PLAN.md) §7).

## Layout

| Zone | Element | Size (logical px) | Content | Tokens | Update rule |
|---|---|---|---|---|---|
| TL | **Score stack** | 118 × (10 + 3 × 15) | `MODE · RD n` header on a `signal` tab with status lamp; one row per team: glyph ▲●■◆, team name `label`, score `num` right-aligned; own team row has `signal` 2 px left rule + "YOU" chip | team-*, score, gradient fade ink-950 80 → 0 % | animate value changes with count-up 400 ms (`m-count`) |
| TC | **Round timer** | 64 × 28 | `MM:SS` in Barlow Cond 700 23 px (46 px @720p), 2 px outline, grid reference `micro` below | paper; warn < 30 s; danger + pulse < 10 s | 1 Hz |
| TC below | **Objective strip** | 200 × 18 | up to 2 objective chips: solid letter chip (Barlow Cond, never display font) + outlined distance `128 m` | ice | 4 Hz |
| TR | **Kill feed** | 180 × (5 × 16) | `killer ▸ weapon icon ▸ victim`, team-colour names, own name `signal` chip | team-*, label | entries live 5 s, max 5, fade 200 ms |
| TR below | **Minimap** (optional, off by default on low) | 88 × 88 | rotated, own arrow `signal` | ink-950 75 % | 10 Hz |
| TC | **Collapse warning** | icon 24 + edge arrow | shaking hazard-striped icon on a building about to fall, clamped to the screen edge with an arrow when off screen; countdown `micro` | warn, ink-950 | from first creak until the collapse ends |
| C | **Crosshair** | 16 × 16 default | 4 ticks + 1 px dot, 1 px ink outline; spread = weapon cone | signal default (user colour) | per frame |
| C + 0, 0 | **Hit marker** | 20 × 20 | 4 diagonal ticks; white = hit, `danger` = crit/head, × = kill | paper / danger | 120 ms in, 180 ms out (`m-hit`) |
| C + 64 below | **Event line** | ≤ 220 × 18 | callout strip: `signal` tab + paper slab `ELIMINATED` + ink leader `+100`; max 3 stacked | paper slab, ink-900 text, score numeral | 1.6 s life, slide-up 160 ms |
| ML/MR | **Damage direction** | arc 90° at 120 px radius | red arc wedge pointing to source | danger 70 % | 600 ms fade |
| BL | **Squad** | 140 × (2 × 18) | teammates: name, small health bar, status icon (down / revive) | paper, heal, danger | 4 Hz |
| BL | **Health block** | 160 × 40 | `175` in `num-xl` outlined (no scrim box), segmented bar (1 seg = 25 HP), shield segs `shield` on top | paper, shield, danger < 30 % | per frame |
| BC | **Gadget / ability row** | 3 × 32 × 32 (touch: 44 × 44) | notched tile (notch-s) + icon + cooldown sweep + seconds numeral + key chip `micro` | ink-700, paper, signal when ready | per frame |
| BR | **Weapon block** | 160 × 40 | weapon name `label`, ammo `24` `num-xl` / reserve `/ 96` `body`, slot pips 1–3 | paper; warn when ≤ 25 % mag; danger 0 | per frame |
| full | **Low-health vignette** | full | screen-edge `danger` 0→30 % with heartbeat | danger | ≤ 30 % HP |
| full | **Down / respawn** | centred 260 × 64 | `REVIVE IN 0:07` bar or `RESPAWN 12` + "hold to give up" | heal / paper | 10 Hz |
| TC banner | **Match event banner** | callout strip 280 × 36 | `ZONE B OPEN` slab + `HOLD ◇B` leader; map events (`FLOOD IN 30s`) use a `warn` slab with hazard stripes | ice slab + ink-900 text; warn for map events | 3 s, wipe in `m-wipe` 180 ms, out 240 ms |

Rules:
1. The central 40 % width × 40 % height box contains only crosshair, hit marker, event lines and transient banners (P1).
2. Every text over the world: 2 px `ink-950` outline (default for numerals and labels) or a 75 % `ink-950` scrim / gradient fade (≥ 4.5:1 measured over white, sRGB-space blend, DESIGN_SYSTEM §3). No closed boxes around HUD blocks.
3. Draw order: world → viewmodel → world-space markers → HUD → menus. HUD is one draw list, target ≤ 4 draws (1 quad batch + 1 text batch + 1 icon batch + vignette).
4. HUD scale slider 80–120 %; each zone independently hideable in settings (streamer mode).
5. Colour-blind: every team/state colour is paired with a glyph or pattern (ACCESSIBILITY).

## World-space markers
| Marker | Shape | Clamp | Behaviour |
|---|---|---|---|
| Objective | ◇ outline 18 px + letter + distance | clamps to screen edge with arrow | fades to 40 % when within 10 m |
| Teammate | team glyph 10 px above head + name ≤ 40 m | not clamped | through walls: outline only |
| Ping | ice diamond, 6 s | clamps | sound `ui_ping` |
| Enemy spotted | danger ▼ 2 s | no | only after a "spot" event |

## HUD states
`alive` · `aiming` (crosshair → weapon scope, team stack collapses to own row) · `downed` (everything dims to 40 % except revive bar) · `spectating` (squad cam label) · `respawn` · `round_end` (HUD fades 300 ms, results banner).
