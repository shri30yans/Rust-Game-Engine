# imagine-menus: IMAGINE's screens from boot to results, with a working lobby (PLAN §4, MENUS_SPEC, D9, D46, D57)

Goal: IMAGINE starts like a finished game, not straight into the arena. Done means:
1. Boot → Home → Mode select → Lobby → Loading → Match (HUD) → In-match menu → Results → Home all work with mouse and keyboard, and with touch on Android.
2. The lobby is a real `engine-sessions` room run locally. The player hosts, adds and removes bots, picks teams, readies, and starts; the countdown leads into loading and the match. The same screens will show an online room once net-play-1 lands, with no change to them.
3. Settings (graphics, controls, audio placeholder, accessibility) read and write the saved settings file.
4. Every screen follows its mockup and the design system.
5. A steady menu frame allocates nothing, and the four gates pass.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D9, D10, D23 (never copy another game's content), D46, D48, D49, D55, D57;
- `docs/briefs/ui-menus-engine.md` (the widgets and `Menus` runtime you build on);
- `games/imagine/docs/design/{MENUS_SPEC,COMPONENTS,DESIGN_SYSTEM,MOTION_SOUND,ACCESSIBILITY,TOUCH_SPEC}.md`, `theme.ron`, and the mockups in `games/imagine/docs/design/mockups/` (`home`, `loadout`, `results`, `touch` HTML and PNG; `tokens.css`);
- `games/imagine/docs/PLAN.md` §2 (modes) and §4 (screens);
- `games/imagine/ui/hud.html`;
- `games/imagine/game/src/{lib,present,hud,data}.rs`;
- `engine/sessions/src/`.

## Design

**Flow as one state machine (`games/imagine/game/src/flow.rs`).**
- `enum Flow { Boot, Home, ModeSelect, Lobby, Loading, Match, Results }`. Transitions are typed methods that return the screens to push or pop; the presenter applies them to `Menus`. The flow never draws and never reads input itself.
- In-match menu and scoreboard are `GameMenu`-layer screens over `Match`.
- Confirmations ("Leave match?", "Quit game?") are `Modal` screens with the destructive action outlined in `danger` (MENUS_SPEC rule 4).
- Back from Home opens "Quit game?" (rule 5), using the document's `back` action.

**Screens (`games/imagine/ui/*.html`, cooked by `ui-cook`, hot-reloaded in dev like `hud.html`).** Lay each one out after its mockup, using only the engine's HTML subset and tokens from `theme.ron`:
- `boot.html`: logo and build line, 1.5 s, or until assets are ready.
- `home.html`: top bar (player name, level placeholder, settings button), the featured mode card, PLAY as the one `signal` element and the default focus, then Loadout, Settings and Quit.
- `modes.html`: the season 1 modes from PLAN §2 (Survival Protocol, Control Shift, Practice), each with players, length and a one-line rule. Only modes the game can run are selectable; the others show "coming soon" disabled. The mode list is data (`games/imagine/data/modes.ron`), not markup.
- `lobby.html`: room code, mode and arena (host can change them through `set_settings`), a seat list (`Repeat` over seats: name, team colour, ready mark, bot tag, host crown), and host buttons (add bot, remove bot, kick, start). Everyone has Ready. A countdown banner shows during `Phase::Countdown`.
- `loading.html`: arena name in `display-l`, mode, objective summary, a progress bar bound to real load progress (scene and assets), and a rotating tip from data.
- `match_menu.html`: Resume (default), Settings, Scoreboard, Leave match (opens the confirm modal). The match keeps running.
- `scoreboard.html`: the existing score data per actor.
- `results.html`: placement banner, podium, own stats, score count-up (`m-count`), Continue.
- `settings.html`: tabs (Graphics, Controls, Audio, Accessibility).
  - Graphics: quality tier, render scale, FOV.
  - Controls: sensitivity X/Y and fire mode, moved here from the old in-HUD menu, which is deleted.
  - Audio: volume sliders, stored now and used once `engine-audio` lands.
  - Accessibility: reduce motion (wired to `Menus::set_reduce_motion`), colour-blind preset placeholder value only if the theme defines the presets; otherwise leave it out.

  Each setting has its one-line description (rule 6). Graphics changes revert after 10 s unless confirmed (MENUS_SPEC table).
- `loadout.html` (after the mockup): class tabs and slots bound to the weapon data that exists. If the game has only one weapon, show only what exists; no fake items.

**Lobby in the game.** A `LocalSession` owns `Room<ImagineSettings>` (seats 12, teams from the mode, admission Open, countdown 3 s at the tick rate) and drives it from the screen's `UiEvent`s, with the local player as the first human seat.
- When the room's `MatchStarted` event fires, the flow goes to Loading. The arena is built from `ImagineSettings` and the seated bots' count, and the match starts when loading completes.
- When the match ends, `end_match` and then Results; Continue calls `return_to_lobby` (or goes Home from a quick play).
- Lobby actions are commands to the room; refusals show as a toast with the `SessionError` text.

**Look.** Use the design system exactly:
- the angular slab and callout-strip components (COMPONENTS);
- `m-screen`, `m-modal`, `m-press` and `m-count` motion;
- the focus ring from rule 2, styled with `:focus-visible`;
- pressed states for touch;
- the safe areas and minimum touch size from TOUCH_SPEC.

Text sizes and colours come only from theme tokens. Where the engine format cannot express a mockup detail, use the closest supported form and list each such gap in the change notes; do not hard-code a workaround.

**Input.** On Android the menus take touch through the engine's raw touch list; in the match the existing touch buttons return. The mouse is released on every menu screen and grabbed again in the match.

**Tests:**
- Flow transitions as a table: every screen's actions lead where MENUS_SPEC says, Back works everywhere, quit and leave confirm.
- Lobby: add bots to fill, team caps, ready gating, countdown cancelled by unready, start leads to Loading, results return to the lobby with humans unready.
- Every screen document cooks and validates.
- Settings round trip through the settings file, including revert after 10 s without confirmation (tick-driven, no wall clock in tests).
- Golden draw lists for Home, Lobby and Results at 1920×1080 and at a phone size.
- A headless run: boot → home → modes → lobby (2 bots) → start → loading → 600 match ticks → end → results → home, with no error and a stable state hash across two runs.
- Counting allocator: 1,000 frames on the lobby screen with focus moves allocate 0 bytes after warm-up.

**Do not touch:** engine crates (if an engine gap blocks a screen, stop and report it with the exact missing capability), `kits/`, other games.

**Docs in the same change:**
- `games/imagine/docs/design/MENUS_SPEC.md` (mode names from PLAN §2, the built flow);
- `games/imagine/docs/PLAN.md` §4 status;
- `docs/briefs/README.md` status `review`.

**Unblocked: net-play-1 merged on 2026-11-12.** Previously blocked: Codex stopped correctly. Building the match from the room needs match travel (`App::travel`, `Game::setup` taking settings, `Presenter::on_travel`), which is now part of net-play-1 (see its brief, "Decision: match travel"). When this resumes, `LocalSession`'s `MatchStarted` calls the client's travel with the room's `ImagineSettings`, and the front end is IMAGINE's menu-backdrop settings.

**Running alongside (2026-11-12):** `net-play-1` (it changes `Presenter`: `apply_input` becomes `command` and `before_ticks`, and the fire flag becomes `Trigger`) and `render-scene-retained` (it replaces `Gfx::draw*` with `RenderScene` primitives) both edit `games/imagine/game/src/present.rs` and `engine/app/src/client.rs`. Put the flow, the screens' presenter code and `LocalSession` in new modules (`flow.rs`, `menus.rs`, `session.rs`). Keep `present.rs` edits to the hook-up and to deleting the old in-HUD settings menu, and merge `main` again before you finish. `engine/ui`'s `Menus` is merged: `Repeat` rows report `RowIndex` in their events, so the lobby's per-seat buttons use it.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
