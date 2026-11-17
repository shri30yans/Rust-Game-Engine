# Touch spec

**Later.** The first versions are PC (mouse, keyboard, gamepad). This spec keeps a phone and tablet version possible; target devices are an open question ([PERFORMANCE.md](../../../../docs/PERFORMANCE.md) §1).

## Units
A phone in landscape has a short side of about 65–72 mm (A: 6.1–6.8" 19.5–20:9 panels). With the canvas at 360 logical px tall, **1 logical px ≈ 0.19 mm**, so **9 mm ≈ 48 px** and **14 mm ≈ 72 px**. On a 2400×1080 panel at ~400 ppi, 1 logical px = 3 device px (1080/360).

Runtime rule: `mm_per_px = (screen_height_px / dpi * 25.4) / 360`. When it drops below 0.17 (small or high-aspect phones), touch controls scale up so they never fall below their mm minimums. Tablets get larger controls automatically; their size is capped at 1.3× the phone mm values.

Minimums, from first principles and platform guidance: Apple asks for at least 44 pt (~7 mm) and Material for 48 dp (~9 mm). **Our floor is 9 mm for any touch target, 12 mm for anything pressed in combat, and 8 px (1.5 mm) of empty gap between targets.**

## Default combat layout (player view: [mockups/touch-play.png](mockups/touch-play.png); annotated sheet: [mockups/touch.png](mockups/touch.png))

| Control | Position (centre, from safe-area edges) | Size | mm | Behaviour |
|---|---|---|---|---|
| **Move stick** | floating; spawns where the thumb lands in the left 40 % × lower 70 % zone | base 112 px, knob 48 px | 21 / 9 | dead zone 12 %; pushing past 100 % for 300 ms locks sprint (chevron ▲ shows) |
| **Look** | any touch on the right 60 % not on a button | — | — | relative drag; sensitivity curve (ACCESSIBILITY); 2nd finger allowed |
| **Fire (right)** | right 96, bottom 96 | 72 | 14 | press = fire; drag on it also aims ("fire-and-look") |
| **Fire (left)** (optional) | left 64, top 120 | 56 | 11 | for claw grip; off by default |
| **ADS / scope** | right 176, bottom 72 | 56 | 11 | toggle or hold (setting) |
| **Jump** | right 40, bottom 176 | 56 | 11 | hold = mantle |
| **Crouch / slide** | right 176, bottom 152 | 48 | 9 | tap crouch; tap while sprinting = slide |
| **Reload** | right 96, bottom 176 | 48 | 9 | the ammo numeral also reloads when tapped |
| **Ability 1–2** | right 248, bottom 48 / right 248, bottom 112 | 48 | 9 | cooldown sweep; long-press shows info |
| **Weapon swap** | tap the weapon block (BR) | 160 × 40 | 30 × 7.6 → hit area padded to 48 tall | swipe left/right to cycle |
| **Ping** | top-right of the right cluster | 40 visual, 48 hit | 9 hit | tap = ping where aimed |
| **Menu / scoreboard** | top-left, inside the safe area | 40 visual, 48 hit | 9 | — |

Rules:
1. Combat buttons use `ink-950` at 45 % with a 2 px `paper` 60 % ring. On press: fill `paper` 25 %, scale 0.94 within 60 ms, then a haptic tick (Android `KEYBOARD_TAP`, iOS `UIImpactFeedbackGenerator.light`).
2. **Layout editor**: drag any control and resize it 80–140 %; positions are stored as anchors plus offsets in `touch_layout.ron`, so they survive aspect changes; three presets (Thumbs, Claw, Lefty).
3. **Gyro aim** (optional): adds to touch look while ADS; on by default only on the high tier, off elsewhere.
4. **Assists:** aim slowdown near targets (strength 0–100 %, default 40 %); auto-fire is off by default but available (helps accessibility, is a known option in mobile shooters).
5. Notches and rounded corners: no control is centred within 4 mm of a display cutout; the safe-area inset comes from `WindowInsets` (Android) and `safeAreaInsets` (iOS).

## Menus on touch
- List rows 56 px tall (10.6 mm), tabs 48 px, primary buttons 56 px, icon buttons 48 px hit area.
- Back is the OS back gesture or button plus an on-screen ‹ BACK at the top-left (48 px).
- Scrolling lists use kinetic scrolling with 0.94 friction per frame at 60 Hz and an edge bounce of 24 px or less.
