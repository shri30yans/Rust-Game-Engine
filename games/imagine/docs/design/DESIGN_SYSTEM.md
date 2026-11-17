# Design system

The UI rules for **IMAGINE** (working title, [decision index](../../../../docs/README.md) D21): a first-person shooter with destructible buildings and many modes ([PLAN.md](../PLAN.md)). We borrow the *genre* of destructible-city team shooters, not any game's look: upright condensed type (not italic), notched corners, orange and ice accents, brutalist concrete with warm work lights.

**Visual system: "Work Site"** (D20). The UI is built from construction-site and survey parts: callout strips, corner marks, a status lamp, map grid references, a score stack, a news strip, hazard stripes, blueprint-grid and halftone texture, and wipes. **Menus are loud** (art, 5 layers, motion); **the HUD is quiet but branded** (the same shapes at low weight). Each season can re-colour the kit without changing its shapes. The research and the kit are in [UI_STUDY.md](UI_STUDY.md).

Supporting specs: [HUD](HUD_SPEC.md) · [Menus & flow](MENUS_SPEC.md) · [Components](COMPONENTS.md) · [Motion & sound](MOTION_SOUND.md) · [Accessibility](ACCESSIBILITY.md) · [Touch](TOUCH_SPEC.md) · tokens as data: [theme.ron](../../ui/theme.ron) · [mockups/](mockups/)

## 1. Principles

| # | Principle | Applying it | Violating it |
|---|---|---|---|
| P1 | **The world is the star; the UI frames it.** The HUD hugs the edges, uses thin rules and small type, and never covers the central 40 % of the screen except for short feedback. | Elimination confirmation appears 64 px below the crosshair for 1.6 s, then slides away. | A permanent objective panel in the top-middle. |
| P2 | **One glance, one number.** Every HUD element answers one question with one dominant numeral. | Health shows `175`; the bar is secondary. | Health shown as bar + percent + numeral at equal weight. |
| P3 | **Colour means team or state, never decoration.** Orange `signal` = *you / act now*. Team colours only identify teams. Red only means damage. Hazard stripes only mean "this will collapse". | The PLAY button is the only orange thing on the home screen. | Orange section headers. |
| P4 | **Every input is first class.** Mouse and keyboard, gamepad and (later) touch all work on every screen; controls are big enough to hit. | Loadout slots are 72 px tall. | Tiny "x" close buttons in a corner. |
| P5 | **Feel beats fidelity.** Every hit, pickup and button has motion + sound within 50 ms ([PRINCIPLES.md](../../../../docs/PRINCIPLES.md) §5.1). | Hit marker on the same frame as the predicted hit. | A beautiful marker that appears 120 ms late. |

## 2. The 3D world

The world's art direction (palette, materials, light, grade, destruction look, character readability) is in [LOOK.md](../LOOK.md). The UI's job is to stay readable over it.

## 3. Colour tokens

Contrast ratios computed with the WCAG 2.x relative-luminance formula (the `ui` crate recomputes them from theme.ron in a test). AA body text needs 4.5:1, large text/graphics 3:1.

| Token | Hex | On `ink-900` | Use |
|---|---|---:|---|
| `ink-950` | #07090C | — | scrims, text outline |
| `ink-900` | #0C0F14 | — | screen background |
| `ink-800` | #141922 | — | panels |
| `ink-700` | #1E2530 | — | raised panels, inputs |
| `ink-600` | #2C3543 | 1.55 | dividers, idle borders |
| `ink-500` | #46505F | 2.35 | disabled fills (never text) |
| `ink-400` | #6B7686 | 4.17 | large hint text only (≥ 24 px) |
| `ink-300` | #98A2B1 | 7.44 | secondary text |
| `ink-200` | #C3CAD4 | 11.63 | body text |
| `paper` | #F3F5F8 | 17.57 | primary text, health |
| `signal` | #FF6A1A | 6.70 | primary action, "you", focus ring. Text on it: `ink-900` (6.70) |
| `ice` | #7FE3FF | 13.10 | info, objective markers, links |
| `score` | #FFD447 | 13.49 | points and score values only |
| `shield` | #5CC8FF | 10.20 | armour segment |
| `danger` | #FF3D52 | 5.52 | damage, low health (on `ink-700` only ≥ 24 px: 4.43) |
| `heal` | #3DF5A0 | 13.50 | healing, revive |
| `warn` | #FFB52E | 10.88 | timers < 10 s, overheat |
| `team-ember` | #FF7A3D | 7.41 | team A + ▲ glyph |
| `team-tide` | #3D8CFF | 5.84 | team B + ● glyph |
| `team-volt` | #C9F23A | 14.85 | team C + ■ glyph |
| `team-orchid` | #C37BFF | 6.94 | team D + ◆ glyph |

**Your own team is always drawn with an outline + filled chip and the word YOU**, regardless of colour (CVD-safe; see ACCESSIBILITY).

**HUD over the 3D world:** text sits on a `ink-950` scrim at **75 %**, or on no scrim with a 2 px `ink-950` 80 % outline + 0/2 px shadow. `signal` is never used for text directly over the world (1.54:1 over white).

**UI blends in sRGB (gamma) space, not linear.** Computed with the WCAG formula: `paper` on a 75 % `ink-950` scrim over a pure-white scene = **8.59:1 with sRGB-space blending but only 3.18:1 with linear blending** (55 %: 4.07 vs 1.92). wgpu blends in linear when drawing to an `*Srgb` surface view, so the UI pass draws into a **non-sRGB (`Unorm`) view of the swapchain** after the final tonemap pass has written sRGB-encoded values (D8). With that, alpha values in this document mean the same thing as in the mockups and in design tools. If a platform cannot provide the second view format, the fallback is a scrim alpha of **0.84** (linear blending reaches 4.5:1 over white).

## 4. Typography

| Role | Font (SIL OFL 1.1) | Weight | Notes |
|---|---|---|---|
| Display (menus, banners, results) | **Big Shoulders Display** | 800, 900 | heavy industrial condensed; ≥ 28 px only ([UI_STUDY §4](UI_STUDY.md)) |
| HUD numerals and labels | **Barlow Condensed** | 600, 700 | tabular figures; uppercase for labels with +4 % tracking |
| UI text | **Barlow** | 400, 500, 600 | same family = coherent; good at small sizes |
| Mono (debug, codes, cvars) | **JetBrains Mono** | 400 | console, party codes |

Type scale (px at 1× = 360 dp-tall landscape reference; scales by `ui_scale` = min(width/640, height/360) clamped 1.0–3.0, times user text scale):

| Token | Size / line | Font | Use |
|---|---|---|---|
| `display-xl` | 56/56 | Big Shoulders 900 | results "VICTORY", round banners |
| `display-l` | 40/40 | Big Shoulders 800 | screen titles |
| `num-xl` | 32/32 | Barlow Cond 700 | ammo, health numerals |
| `title` | 22/26 | Barlow Cond 600 caps | section titles, tab labels |
| `body-l` | 16/22 | Barlow 500 | buttons, list rows |
| `body` | 14/20 | Barlow 400 | descriptions |
| `label` | 12/16 | Barlow Cond 600 caps | HUD labels, chips |
| `micro` | 10/12 | Barlow Cond 600 caps | key hints, distance (min size; never below) |

## 5. Space, shape, elevation

- **Spacing** (4-pt base): `s0 2 · s1 4 · s2 8 · s3 12 · s4 16 · s5 24 · s6 32 · s7 48 · s8 64`.
- **Radii:** `r0 0` (HUD bars), `r1 2` (chips), `r2 4` (buttons, cards), `r-pill 999` (toggles). The **notch**: 45° cut on the top-right corner, `notch-s 6 px`, `notch-m 12 px` — the signature shape on primary buttons, tabs and banners.
- **Strokes:** `hair 1 px`, `rule 2 px`, `focus 3 px signal`.
- **Elevation:** `e0` flat; `e1` panel = `ink-800` + 1 px `ink-600` top highlight; `e2` popover = `ink-700` + shadow 0 8 24 #000 50 %; `e3` modal = `e2` + 60 % `ink-950` scrim + 8 px backdrop blur.
- **Iconography:** 24-px grid, 2 px stroke, square caps, 45° chamfers echoing the notch; filled variant for active state. Drawn in-house as SVG, baked to an MSDF atlas.

## 6. Layout grid

- Reference canvas 640×360 logical px (16:9). Safe area insets from the OS + **16 px** gutter. 12-column grid, 8 px gutters for menus; HUD uses 9 anchor zones (TL, TC, TR, ML, C, MR, BL, BC, BR).
- Aspect support: 4:3 to 21:9+. Wider than 16:9: menus centre at max 960 logical px; HUD anchors stretch to the safe edges.

## 7. Token coverage map

| Token group | Used in |
|---|---|
| ink-* | every screen background/panel ([MENUS](MENUS_SPEC.md)), HUD scrims ([HUD](HUD_SPEC.md)) |
| signal | primary button, focus ring, own-player marker, PLAY, crosshair default |
| ice | objective markers, info toasts, links |
| score | team score, objective value, results points |
| shield/hp/danger/heal/warn | health block, damage direction, revive, timers |
| team-* | score stack, kill feed names, scoreboard, results podium |
| type scale | all components ([COMPONENTS](COMPONENTS.md)) |
| motion tokens | [MOTION_SOUND](MOTION_SOUND.md) used by every component state |
| Work Site kit (callout strip, score stack, corner marks, status lamp, grid reference, news strip, hazard stripes, blueprint grid, halftone) | [UI_STUDY §3](UI_STUDY.md), [COMPONENTS](COMPONENTS.md), all mockups |
