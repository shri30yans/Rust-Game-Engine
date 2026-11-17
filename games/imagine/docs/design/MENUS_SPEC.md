# Menus and game flow

Architecture: the **layer stack** ([decision index](../../../../docs/README.md) D9), with layers ordered `Game < GameMenu < Menu < Modal < Toast`. Each layer is a stack of screens. Only the top screen of the top layer that accepts input receives focus. Back pops the top screen; Home has no quit action because the operating system owns app exit. Screens are cooked from `games/imagine/ui/*.html` into `*.ui.ron` and bound to view-models in Rust (D10, D46).

## Implemented flow

`games/imagine/game/src/flow.rs` owns the major phases and returns typed stack effects. `menus.rs` loads the cooked screens and routes row-aware widget events. `session.rs` owns the local `engine-sessions::Room<ImagineSettings>` for Practice: one local host, up to eleven bots, ready gating, a 180-tick countdown, settings-driven travel, and a 600-tick practice round. The client applies presenter travel requests at the safe frame boundary (D60). Results Continue and confirmed standalone Leave match return through menu-backdrop settings; leaving skips Results. Online Leave match will disconnect once online session teardown is implemented and is only shown in standalone matches.

Only Practice is selectable in the season 1 list. The other two entries are data-backed disabled rows. Results copy actor kills, deaths, teams and the winner from the finished world; the live scoreboard is refreshed from the score resource. The result score counts up over fixed ticks. Debug desktop builds watch cooked menu documents, validate edits before applying them at the next menu frame, and keep the previous document when parsing or layout validation fails. Release builds use embedded documents. Match travel is synchronous: a new Loading document is laid out and drawn before the next frame applies travel. Streamed load progress will come with `AssetUploader`.

## Flow

```mermaid
flowchart LR
  Boot[Boot / splash 1.5s] --> Consent[First run: EULA + privacy, language, controls preset]
  Consent --> Home
  Boot -->|returning| Home
  Home -->|PLAY| Modes[Mode select]
  Modes --> Queue[Finding match modal]
  Home --> Party[Party / private room]
  Queue --> Load[Loading: map card + tips]
  Load --> Match[In match: HUD]
  Match -->|pause / menu| GameMenu[In-match menu: resume, settings, scoreboard, leave]
  Match --> Results[Results: placement, score, XP, MVP]
  Results --> Home
  Home --> Loadout
  Home --> Settings
  Home --> Career[Career: stats, history]
  Home --> Store[Store (later)]
  Settings --> Controls[Controls + touch layout editor]
  Settings --> Graphics[Graphics: tier, fps cap, scale]
  Settings --> Audio
  Settings --> Access[Accessibility]
```

## Screens

| Screen | Layer | Primary action | Contents | Mockup |
|---|---|---|---|---|
| Home | Menu | **PLAY** (only `signal` element) | featured mode card (Team Fight 5v5 / Breach / Practice), party slots, season banner, player character in 3D on the left 50 %, top bar: player name, level, settings icon | [home.png](mockups/home.png) |
| Loadout | Menu | EQUIP | class tabs (LIGHT / MEDIUM / HEAVY), 3 slots (weapon, gadget ×2), item grid with stat bars, 3D preview | [loadout.png](mockups/loadout.png) |
| Mode select | Menu | SELECT | modes grouped as Shooter / Big Battles / Party & Mystery; each card: players, length, one-line rule | — |
| Finding match | Modal | CANCEL | elapsed time, players found 7/10, tip | — |
| Loading | Menu (full) | — | map name `display-l`, mode, objective summary, progress bar, rotating tip | — |
| In-match menu | GameMenu | RESUME | Resume / Settings / Scoreboard / Leave match (confirm modal) — game keeps running (online) | — |
| Scoreboard | GameMenu | — | live per actor: team, kills and deaths from the score resource | — |
| Results | Menu | CONTINUE | winner/placement, own kills and deaths, rule score count-up | [results.png](mockups/results.png) |
| Settings | Menu | APPLY (auto-applies; reverts graphics after 10 s unless confirmed) | tabs: Graphics, Controls, Audio, Accessibility, Account | — |
| Touch layout editor (later) | Menu (full, transparent over a live HUD) | SAVE | drag/resize controls, presets, reset | [touch.png](mockups/touch.png) |
| Toast | Toast | — | friend joined, reward, network warning; 3 s; max 2 visible | — |

## Navigation rules
1. **Every screen is fully usable with touch, mouse + keyboard, and gamepad.** Focus starts on the primary action. D-pad/arrow navigation follows spatial nearest-neighbour with explicit overrides in RON. Gamepad: A confirm, B back, LB/RB tabs, Start = in-match menu.
2. The focus ring is 3 px `signal` with a 2 px offset and is always visible on keyboard/gamepad. On touch, the pressed state replaces the focus ring.
3. Opening a screen takes ≤ 1 frame of layout plus ≤ 16 ms; heavy content (3D preview) streams in after the first frame.
4. Every destructive action (leave match, reset layout, sell) gets a Modal with the destructive button on the right in `danger` outline, never filled.
5. The operating system owns app exit; menus do not offer Quit. Leave match is confirmed and returns to Home.
6. Settings have a search field (desktop) and every setting has a one-line description.
7. **Graphics settings** expose: Quality tier (Auto/Low/Medium/High/Ultra, [PERFORMANCE.md](../../../../docs/PERFORMANCE.md) §1), FPS cap (60/120/144/uncapped), Render scale (50–100 %), Upscaling, Shadows, Effects density, Bloom, Motion blur (off by default), FOV (70–110). Auto runs a 3-second benchmark on first launch (D19).
