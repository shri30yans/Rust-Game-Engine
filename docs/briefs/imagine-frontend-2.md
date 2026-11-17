# imagine-frontend-2: a professional front end (home, modes, lobby, loading, results), mockups first (PLAN §4, D23, D48)

Goal: IMAGINE's menus look and feel like a shipped mobile shooter: a lit 3D showcase behind the menu, a clear hierarchy, crisp angled panels, icons, readable type, and motion and sound on every press. Today's screens are flat boxes with text that does not fit. Done means:
1. Each screen below is mocked up first.
2. It is then built as a UI document on `ui-kit-2` that matches its mockup in a golden at phone and desktop sizes.
3. It passes the overflow check and the e2e suite from `imagine-device-fixes`.
4. It runs on the APK.
5. The four gates pass.

Starts after `ui-css-layout`, `ui-css-paint`, `ui-fonts`, `ui-kit-2` images and `menu-stage` merge (re-ordered 2026-11-12: the look comes first; `ui-viewmodel`, `ui-actions`, `ui-motion` and `ui-preview` follow as engineering polish).

**How the screens are made (2026-11-12).** The approved mockups in `games/imagine/docs/design/mockups/` are HTML and CSS. The UI engine now draws the CSS subset they use, and that subset is proven against Chrome by `ui-css-layout`, `ui-css-paint` and `ui-fonts`. So each screen is:
1. written as HTML and CSS from its mockup, adapted to real data and the rules below;
2. iterated in a browser;
3. cooked;
4. checked headless in the engine against the mockup PNG at 2400×1080 and 1920×1080, comparing `side_by_side.png` and `diff.png`.
Review each round against the checklist below and fix until the last review finds nothing material. Section 3's "mock up first" step is already done for home, HUD, loadout, results and touch; mock up only the screens that have no mockup (modes, lobby, loading, settings, in-match menu) in the same style, and report those before building them.

**Quality bar: shipped-game polish, not "good enough".** The owner judged the current menus unprofessional. Hold every screen to this list:
- clear hierarchy, with one primary action per screen;
- every element on the 8 px grid;
- consistent spacing rhythm;
- no text that is truncated, crowded or touching its frame;
- icons beside labels where they help;
- depth from layered panels, gradients and subtle shadows, not flat boxes;
- states (hover, pressed, focused, disabled, selected) that are distinct and animated;
- contrast that passes the accessibility rules;
- nothing that looks like a debug tool.
If a screen is not at that level, it is not done.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D23 (no copying of other games' content), D48 and D55;
- `games/imagine/docs/design/` (`DESIGN_SYSTEM`, `COMPONENTS`, `MENUS_SPEC`, `MOTION_SOUND`, `TOUCH_SPEC`, `ACCESSIBILITY`, `UI_STUDY`);
- `games/imagine/docs/LOOK.md`;
- `games/imagine/ui/`;
- `games/imagine/game/src/{flow,menus,session,results}.rs`;
- the ui-kit-2 gallery.

**Quality bar, not a source.** The owner compared our menus with current mobile shooters. Take only the general patterns every such game shares:
- a character or weapon showcase in a lit scene;
- the mode choice on one side;
- one large primary action in the bottom-right thumb zone;
- a player card with level and progress;
- compact icon buttons along the top;
- a podium-style results screen.

Never copy any game's layout pixel for pixel, its names, art, icons, fonts or wording (D23). IMAGINE's own identity comes from LOOK.md: the near-future city, worn concrete and steel, saturated colour only in light, signs and team colours, and the angled HUD style (D48).

## Design

**1. Design system first** (update `DESIGN_SYSTEM.md` and `COMPONENTS.md`):
- **Type:**
  - a condensed display face for titles and buttons, and a clean sans for body text, both OFL, with the licence committed;
  - a type scale (for example 12, 14, 16, 20, 28, 40, 56 at 1080 p height), uppercase with tracking for labels.
- **Colour:**
  - neutral dark panels at 70–85% opacity over the 3D scene;
  - one accent colour for the primary action and focus;
  - team colours;
  - success, warning and danger colours.
  - Check contrast with the existing contrast rules.
- **Style variants (docs/research/2026-11-12-ui-engines.md U4):** the theme defines component variants as token sets (`button.primary`, `button.secondary`, `button.ghost`, `panel.card`, `chip`, `badge`). Documents use classes only. Add a ui-cook lint that rejects raw colours, sizes, spacing and fonts outside the theme, so the design system holds by construction.
- **Shapes:** angled-corner nine-slice frames in three sizes (panel, button, chip) and an icon set, using an open-licence SVG set plus our own where needed. Remember the D23 rule.
- **Spacing:** an 8 px grid. Safe-area padding. Touch targets at least 48 dp. Primary actions in the bottom-right thumb zone.
- **Motion:**
  - screens slide or fade in 180–240 ms, with ease-out;
  - buttons give press feedback in at most 80 ms (scale 0.96 plus brighten) and play a sound;
  - focus has a visible ring.

**2. The 3D showcase behind the menus.**
- The menu-backdrop world is a small lit set: a corner of Kiln Yard at golden hour, with light haze.
- The selected weapon (the cooked tactical pistol, through the asset uploader) sits on a slowly turning display.
- The operator stands in front in an idle pose. The operator is built from the sourced CC0 base body in `games/imagine/content/characters/man_base/source/` (see `games/imagine/docs/ASSETS.md` §1.1a):
  - a Blender build script in `games/imagine/content/characters/operator/blender/` adds armour plates, a helmet, a vest and team-colour panels, all as separate meshes over the body. The script never edits the source file.
  - It exports one GLB with the rig and the clips, in a desktop LOD and a phone LOD (decimated, at most 15k triangles), and a hero render.
  - It is cooked by `tools/cook`, like the weapon pack.
  - The team colour is a material parameter, not a texture per team.
  - The same operator is used on the Results podium.
  - Keep the AI-assisted disclosure from the source licence in CONTENT.md.
  - If the rig or clips cannot be cooked by the current pipeline, stop and report the gap. Do not ship a static pose as a workaround.
- The camera has a slight idle drift.
- The showcase is game content plus the existing renderer. No new engine feature unless one is missing; if one is, stop and report it.

**3. Screens. Mock up each at 2400×1080 and 1920×1080.** Make the mockups as UI documents rendered with `ui-preview shot`, so the mockup is already the screen. Save them in `games/imagine/docs/design/mockups/frontend-2/`, and self-review each one with the loop above before reporting. Stop after the mockups and report with the images, so the owner can approve before the build.
- **Home:**
  - top-left: player card (name, level, XP bar);
  - top-right: icon buttons (settings, friends placeholder hidden until it exists, mail hidden);
  - right: a vertical mode rail (Practice, Arena modes from `modes.ron`; locked ones dimmed with a lock icon);
  - bottom-right: a large PLAY button showing the selected mode;
  - bottom-left: LOADOUT;
  - centre: the showcase.
  - No store, currency or event tiles. Those do not exist, and placeholders for absent features are not allowed.
- **Modes:** cards with a thumbnail rendered from the arena, the name, the team size and a one-line description. The selected card is highlighted, with Back.
- **Lobby:**
  - the mode and map card with its thumbnail;
  - team slots as cards (player card or bot card with its call sign and a remove ✕; an empty slot shows a "+ Add bot" card, which is disabled when the room is full);
  - a READY or START primary button bottom-right;
  - a countdown overlay;
  - the room code chip, once online lobbies exist (hidden for now).
- **Loading:**
  - full-bleed arena art (a render of the arena);
  - the mode and map name;
  - a tip line;
  - team rosters;
  - a progress bar driven by real progress once the uploader reports it (until then the bar is not shown; no fake progress).
- **In-match menu:** Resume, Settings, Leave match, as a side panel over the dimmed match.
- **Results:**
  - VICTORY or DEFEAT banner;
  - the top three as podium cards (rendered operators once they exist, until then large player cards with call sign, score, kills, deaths, MVP badge);
  - the full scoreboard below;
  - the score count-up;
  - an XP bar that fills;
  - a Continue button with an auto-continue timer.
- **Settings:** a tabbed side panel (Graphics, Controls, Audio, Accessibility) with scroll, sliders and toggles from the component set, each showing its value.

**3a. The in-match HUD.** It is held to the same quality bar.
- The match timer is clipped at the top-left screen edge in the 640×360 combat golden (seen on 2026-11-12). Fix it at the cause: safe-area padding from ui-kit-3, and the overflow check at every size.
- Restyle the HUD with the design system. Keep the touch layout from imagine-combat-feel.

**4. Build.**
- After owner approval, rebuild each document to match its mockup.
- Bind real data only: player name, level and XP once progression exists (until then level and XP are hidden, not faked), modes, room slots, results.
- Each screen gets a golden at both sizes and passes the overflow check.
- The e2e suite still drives every screen.

**Do not touch:** `engine/` and `kits/` (stop and report any engine gap instead).

**Docs in the same change:**
- `games/imagine/docs/design/` (system, components, specs, mockups);
- `games/imagine/docs/PLAN.md` §4;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
