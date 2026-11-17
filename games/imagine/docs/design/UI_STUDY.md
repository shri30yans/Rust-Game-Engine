# UI study: what professional shooter UIs do, and what we take

Goal from the owner (2026-11-10): "a professional stylised UI, not just a basic HUD". Learn from many games and copy none. The motif changed on 2026-11-11 from a TV-broadcast look to **Work Site** when the TV premise was dropped ([decision index](../../../../docs/README.md) D1, D20).

**Sources.** Web sources are linked inline. Rows marked **[PK]** come from my prior knowledge of the shipped games and were not re-checked in this session, so check them against [Game UI Database](https://gameuidatabase.com) (60k+ screens, per [gamefromscratch](https://gamefromscratch.com/the-game-ui-database/)) before relying on them.

## 1. What each game teaches

| Game | What its UI does well | Lesson for us | Don't take |
|---|---|---|---|
| **Persona 5** | Menus are branding as well as information. One accent colour (red) plus strong lines that lead the eye. Every menu has a different layout (list, grid, dial). Uses 3D character poses ([mechanicsofmagic](https://mechanicsofmagic.com/2022/04/23/visual-design-of-games-persona-5/), [opencritic/Hashino](https://opencritic.com/news/6949/persona-director-says-designing-stylish-menus-is-actually-really-annoying-)) | **One concept drives every element.** Restrict yourself to a single accent. Characters do the posing. | Its readability cost: the director said early menus were "impossible to read at first", and Tactica moved to calmer layouts ([inverse](https://www.inverse.com/gaming/persona-5-tactica-user-interface)). Style belongs in menus, not in the combat HUD. |
| **Marvel Rivals** | A named visual system ("Dynamic Comic Style") applied to UI, video, web and merch. The UI aims to be "modern and adaptable… simple and clear" with comic and geometric influence ([official dev diary](https://www.marvelrivals.com/devdiaries/20250421/40954_1228989.html)). The early, darker concepts looked cooler but were harder to use ([PC Gamer](https://www.pcgamer.com/games/fps/marvel-rivals-hero-gallery-ui-used-to-look-a-lot-cooler-but-also-a-lot-harder-to-use)) | **Name the system and write it down** (this file and DESIGN_SYSTEM). Keep a plain, clear core and put the stylised flourishes on top. | Comic-ink rendering style. |
| **Overwatch 2** | The 2022 HUD was pared down and colourless. Reviewers wished it had "more style", and fans said the icons lost flair ([GamesRadar](https://www.gamesradar.com/the-overwatch-2-scoreboard-is-as-at-odds-with-the-series-spirit), [Windows Central](https://windowscentral.com/overwatch-2-beta-impressions)). Blizzard announced the franchise's biggest UI overhaul in 2026 ([press-start](https://press-start.com.au/news/2026/02/05/overwatch-spotlight-2026/)) | **Minimal ≠ good.** Stripping out identity is a failure too, and it is the trap my first mockup fell into. | — |
| **Valorant** [PK] | Flat geometric HUD. Ability row bottom-centre with charge pips. Top bar shows both teams' portraits and the round score. Very low clutter ([case study](https://app.notion.com/p/Independent-UI-UX-Case-Study-Valorant-21bba50a6ad480c1a4b0c0f1c5e65db1)). Menus use big agent art and thin key-lines. | The **top bar is the scoreboard**: it is glanceable and shows who is alive. Use **pips for charges**. | Its exact top-bar shape and fonts. |
| **Apex Legends** [PK] | Chunky slab panels. Rarity colours on items. The **ping wheel** and **radial item wheel** ([uxdesign.cc](https://uxdesign.cc/the-power-of-the-radial-menu-a-love-letter-to-apex-legends-from-a-ux-designer-and-perpetual-noob-1bec9b05e805)). Armour colour = tier. Legend posed in a lit 3D lobby. | Use **radials for touch and pad** (pings, gadgets). Use one colour scale for rarity or tier. Pose a **3D character in a lit lobby** for the home screen. | Its slab silhouettes. |
| **Call of Duty (MW-era)** [PK] | Tactical thin type, monochrome with a single accent, dense but tidy menus, big tile cards, an excellent **settings screen with per-option descriptions and preview images**. | Model settings UX on it (MENUS_SPEC rule 6). | Its military look. |
| **Fortnite** [PK] | Loud rarity gradients, chunky rounded type, ever-present store. Kids read it instantly. | Readability at a glance on phones; **reward moments** (results count-ups, unlock reveals). | Rounded cartoon type. |
| **Splitgate 2 / sci-fi sports** [PK] | Neon arena-sport framing with team colours everywhere. | Team colours can carry energy everywhere without a sci-fi setting. | Neon sci-fi. |
| **Deadlock** [PK] | Art-deco ornament and occult theming in a functional MOBA-shooter HUD. Busy, hard to parse at first ([sports.ru](https://cyber.sports.ru/deadlock/blogs/3274108.html)). | A **theme can live in ornament** (line work, frames) while the data stays plain. | Density. |
| **Halo Infinite** [PK] | The visor HUD has a curvature and parallax that make it feel diegetic. | Add **subtle HUD parallax/sway** on look (1–2 px), switched off with reduce-motion. | Helmet framing. |
| **Construction sites and survey maps** (real-world signage in general) | Hazard stripes, stencilled labels, callout tags on drawings, grid references (A1, B4), status lamps on machines, blueprint grids, tape and corner marks. | **Our setting is a city that gets torn down and rebuilt**, so this becomes our motif. It is a broad visual language, not one company's IP, and it can be re-coloured per season. | Any real company's signage or logo. |

## 2. Principles distilled (these replace "minimal HUD")

1. **One named motif, used everywhere.** Ours is **"Work Site"**: the city is a place that is built, broken and rebuilt. Every element is built from construction and survey parts (below).
2. **Two volumes.** *Menus are loud*: big character art, layered graphics and kinetic transitions. *The HUD is quiet but branded*: it uses the same shapes and type, at low weight, with no full boxes. (Lessons from Persona and OW2.)
3. **Layered composition, 5 layers, every menu:** ① 3D or art background, ② graphic texture (blueprint grid / halftone, 3–6 %), ③ content panels, ④ accent graphics (corner marks, rules, status lamp, registration marks), ⑤ micro-type metadata (grid reference, sheet number, codes). Layers ④ and ⑤ are what makes a screen read as "designed".
4. **Typographic contrast is the style.** Ultra-heavy condensed display at huge sizes, a clean grotesque for UI, and mono for metadata. A three-level hierarchy is visible on every screen.
5. **A single accent colour per screen** (`signal`), plus semantic colours only where they mean something.
6. **Motion is identity.** Wipes, callout slide-ins, a one-frame flash on confirm, and a count-up roll. Every transition is ≤ 250 ms and never blocks input.
7. **Data stays plain.** Numbers, names and timers are never distorted, skewed or textured. Only the frames around them are stylised.

## 3. The "Work Site" kit (original elements)

| Element | Look | Where |
|---|---|---|
| **Callout strip** | a coloured tab (team colour or `signal`) + slab with name + a thin leader rule that extends past it, like a callout on a drawing | player names, event callouts, results rows |
| **Corner marks** | 4 L-corners, 2 px, 10–16 px legs, like tape marks on a floor | focus state, selected card, objective markers, crosshair style "Corners", hit confirmation |
| **Status lamp** | 8 px `signal` dot with a slow pulse, like a machine's run lamp | the home screen's matchmaking state, your own team row, the active objective |
| **Grid reference** | `GRID C4 · SHEET 02` in mono, `ink-400` | screen corners as metadata, the round timer's sub-text |
| **Score stack** | compact block: team glyph + score, stacked, with a thin outer frame | top-left in the HUD, scoreboard |
| **News strip** | horizontally scrolling strip of messages in `micro` caps behind a `signal` tab | home screen bottom (news, challenges), results |
| **Hazard stripes** | 45° `warn`/`ink-950` stripes, 6 px wide | **only** for collapse and map-event warnings (P3) |
| **Blueprint grid** | 1 px lines every 16 px at 4 % | panels, backgrounds; never under text smaller than 16 px |
| **Halftone gradient** | dot pattern fading from an edge | character backdrops, banners |
| **Registration marks** | 8 px crosses at panel corners | menu panels only |
| **Notch** | 45° cut top-right (DESIGN_SYSTEM §5) | primary buttons, tabs, banners |
| **Wipe transition** | a `signal` bar sweeps across the screen in 180 ms and reveals the new screen | screen push, round start, results |

## 4. Typography upgrade

Barlow Condensed was too polite for display. New display face: **Big Shoulders Display 800/900** (SIL OFL; Chicago-industrial condensed, very heavy, fits the brutalist city). Fallback: Anton (OFL). Barlow stays for UI text and Barlow Condensed for HUD labels and numerals (it is more legible small). JetBrains Mono is used for grid references, codes and other metadata. **Doto** (OFL dot-matrix) is a candidate for a scoreboard LED moment on the results screen only. It is not used in the HUD because a dot-matrix face is hard to read small.

Both Google Fonts URLs return 200 (checked with curl, 2026-11-10). The game bundles the TTFs (cost: about 60–90 KB each before MSDF baking).
