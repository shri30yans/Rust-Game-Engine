# hud-fit: the HUD fits every test phone, with real text, nothing hidden and nothing overlapping (2026-11-12)

A headless phone render (1400×640, touch on) shows these faults:
- health reads "1…", ammo reads "…", and buttons read "CROU…";
- the touch buttons cover the ammo and weapon panel;
- the telemetry panel covers the scoreboard;
- the weapon label says "VX-9 CARBINE" over the pistol model.

**Why it was possible.**
1. **The checks measure text with a fake font.** `engine/app/src/ui_present.rs` `NoFont` uses 0.5 × size per character, so a check passes while real text does not fit.
2. **Ellipsis hides overflow.** Truncated text never "overflows", so `check_text_overflow` passes while a number shows "1…".
3. **Nothing checks overlap** between touch controls and the rest of the HUD.
4. **The weapon has two sources of truth.** The name is in `games/imagine/data/tuning.ron`, and the viewmodel mesh is picked in `present.rs`.

## How other engines prevent this
- **Unreal.** A DPI scale curve plus title-safe zones, previewed per device in the UMG designer.
- **Unity.** The Device Simulator shows each phone's resolution and safe area.
- **Both.** Layout and checks measure with the real font. There is one font path.

## Design
1. **One text measurer.**
   - The SDF font atlas measures on the CPU, so tests and checks use the real font.
   - Delete `NoFont`, and the test text measurers in IMAGINE tests, and use the real one.
2. **Bound values never truncate.**
   - The document marks which text may shorten: only `text-overflow: ellipsis` on name text.
   - A text node bound to a number (health, ammo, timer, score) never truncates, and the HUD lays it out at its natural width.
   - The layout check fails if any bound number would truncate.
3. **HUD fit check.** At 2800×1280, 2780×1264, 1920×1080 and 1280×720, with touch on and off, and with the telemetry panel on:
   - every text fits;
   - no touch region overlaps another visible HUD element;
   - the telemetry panel overlaps nothing, so move it to a free area.
   - The check runs in the IMAGINE e2e and HUD tests.
4. **One weapon definition.**
   - A weapon entry in IMAGINE data carries its display name, its viewmodel asset and its tuning.
   - The HUD label and the viewmodel both read that entry.
   - The current weapon is the pistol; give it a name in our own style (D23: nothing from THE FINALS).
5. **Fix `hud.html`** until the check passes everywhere. Show the HUD at all four sizes in the report.

Done means:
- items 1 to 5 complete end to end;
- the four gates pass;
- the APK builds.

Starts after `touch-from-layout` and `ui-css-layout` merge.

Read first:
- `AGENTS.md`;
- `engine/app/src/ui_present.rs`;
- `engine/ui/src/checks.rs` and `engine/ui/src/layout.rs`;
- `games/imagine/ui/hud.html`;
- `games/imagine/game/src/{hud.rs,present.rs}`;
- `games/imagine/data/tuning.ron`.

Docs in the same change:
- `docs/ARCHITECTURE.md` §1;
- `games/imagine/docs/README.md`;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
