# UI design system

The look and rules of every screen and the HUD. These files are the source of truth for UI layout and style; the `ui` crate implements them ([ARCHITECTURE.md](../../../../docs/ARCHITECTURE.md) §10). The look of the 3D world is in [LOOK.md](../LOOK.md).

Working title: **IMAGINE** ([decision index](../../../../docs/README.md) D21). UI motif: **Work Site** (D20).

## Files

| File | Holds |
|---|---|
| [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md) | principles, colour, type, space, shape, grid |
| [UI_STUDY.md](UI_STUDY.md) | what other shooters' UIs teach, and the Work Site kit |
| [HUD_SPEC.md](HUD_SPEC.md) | in-match HUD zones, markers, states |
| [MENUS_SPEC.md](MENUS_SPEC.md) | screen flow, screen list, navigation rules |
| [COMPONENTS.md](COMPONENTS.md) | the widget library: anatomy, sizes, states |
| [MOTION_SOUND.md](MOTION_SOUND.md) | motion tokens, feedback per event, sound mix |
| [ACCESSIBILITY.md](ACCESSIBILITY.md) | contrast, colour vision, text size, input, audio |
| [TOUCH_SPEC.md](TOUCH_SPEC.md) | touch controls, for a later phone and tablet version |
| [theme.ron](../../ui/theme.ron) | the tokens as data; the `ui` crate loads this file (D10) |
| [mockups/](mockups/) | HTML reference screens + PNG snapshots; `tokens.css` mirrors theme.ron |

## Mockups (1280×720)

```bash
open games/imagine/docs/design/mockups/hud-angled.html
```

| Mockup | Shows |
|---|---|
| `hud-angled.html` | in-match HUD over a 3D street (loads Three.js from a CDN and `three/char3d.js`) — the target in-match look |
| `hud.html` | flat HUD over `arena.svg` |
| `home.html` | home screen with the player character and PLAY |
| `loadout.html` | player type, weapon and gadget slots |
| `results.html` | match results |
| `touch-play.html`, `touch.html` | touch layout (later) |
| `player3d.html` | the placeholder player in 3D |

Re-render a PNG after changing a mockup:

```bash
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless --disable-gpu --hide-scrollbars --window-size=1280,720 --virtual-time-budget=4000 --screenshot=games/imagine/docs/design/mockups/home.png games/imagine/docs/design/mockups/home.html
```

## Porting rules

1. Tokens come from `theme.ron` only; `tokens.css` is a copy for the mockups and is kept in sync by hand.
2. HUD zones in `HUD_SPEC.md` become pieces a mode turns on or off ([PLAN.md](../PLAN.md) §7).
3. The HTML files are layout references; the game never embeds them.
