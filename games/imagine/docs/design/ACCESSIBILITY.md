# Accessibility

Target: meet the **Game Accessibility Guidelines** "basic" tier plus selected "intermediate" items, and use **WCAG 2.2 AA contrast** for all text ([gameaccessibilityguidelines.com](https://gameaccessibilityguidelines.com), [w3.org/TR/WCAG22](https://www.w3.org/TR/WCAG22/), both checked 2026-11-10).

| Area | Requirement | How | Build step (old numbers, mapped in [PLAN.md](../PLAN.md) §9) |
|---|---|---|---|
| Contrast | text ≥ 4.5:1, large text and graphics ≥ 3:1 | token table in DESIGN_SYSTEM §3 (computed); HUD over the world uses a 75 % scrim or an outline; a test in `engine-ui` recomputes ratios for every pair in theme.ron's `contrast_rules` | 7 |
| Colour vision | never colour alone | team glyphs ▲●■◆; damage = colour + direction arc shape; stat deltas carry ▲▼; filters: protan/deutan/tritan (colour-lookup swap in post-processing, RENDERING §14) | 7 |
| Text size | 100–150 % text scale, layouts reflow (taffy) | golden shots at 100 % and 150 % for every screen | 7 |
| Subtitles | on by default for VO; speaker name, background box 75 %, size S/M/L | — | 8 |
| Motion | Reduce motion: fades only, no shake, no HUD bob, no FOV kick; motion blur off by default | MOTION_SOUND | 3, 7 |
| Flash | no full-screen flashes > 3 per second; flashbang effect capped at 80 % white | VFX review | 3 |
| Input | full remapping (KB / pad); hold ↔ toggle for ADS, crouch, sprint; adjustable stick dead zone; touch layout editor; aim assist slider; auto-fire option | TOUCH_SPEC | 7 |
| Look curve | sensitivity X/Y separately, acceleration 0–100 %, gyro on/off | — | 3 |
| Audio | separate volume sliders per bus; mono audio; visualised sound (direction indicators for footsteps and gunfire, optional) | kira buses | 8 |
| Cognitive | consistent back behaviour; every icon has a text label on menus; a tutorial match vs bots | MENUS_SPEC | 8 |
| Screen readers | menus expose an accessibility tree via **AccessKit** (after launch) | — | Later |

Contrast rule of thumb for designers: on `ink-800`/`ink-900`, any token in DESIGN_SYSTEM §3 marked ≥ 4.5 is free to use for text. `ink-400` and `danger` on `ink-700` need ≥ 24 px or bold ≥ 18.7 px.
