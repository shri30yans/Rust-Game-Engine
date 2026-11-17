# Component library

Every component lives in `ui/src/widgets/` (crate `engine-ui`) and as a RON template in `content/ui/components/`. States: **idle · hover · pressed · focused · disabled · selected** (where it applies). Sizes are logical px (see TOUCH_SPEC for mm). Motion tokens are in MOTION_SOUND.

| Component | Anatomy | Sizes | States (deltas from idle) | Tokens |
|---|---|---|---|---|
| **Button / primary** | notched rect (notch-m top-right) + label `body-l` caps 600 + optional trailing icon | h 56 (touch) / 40 (desktop), min w 160 | hover: fill +8 % lightness, notch glows 1 px; pressed: scale 0.97, `m-press`; focused: 3 px signal ring offset 2; disabled: ink-600 fill, ink-400 text | fill `signal`, text `ink-900` (6.70:1) |
| **Button / secondary** | rect r2, 1 px `ink-500` border, label `paper` | same | hover: border `ink-300`; pressed: fill `ink-700` | ink-800, paper |
| **Button / ghost** | label only + underline on hover | h 40 | — | ink-200 |
| **Button / danger** | secondary with `danger` border + text | same | pressed: fill danger 15 % | danger (only ≥ 16 px bold on ink-800) |
| **Icon button** | 24 icon in 48 hit area | 48 | hover: ink-700 circle | paper |
| **Tab bar** | row of tabs `title` caps; active tab: notch-s underline 3 px `signal` + paper text | h 48 | inactive: ink-300 text | — |
| **Card / mode** | e1 panel, image top 60 %, title `title`, sub `body` | 240 × 160 | selected: 2 px signal border + notch; hover: lift 2 px, e2 | ink-800 |
| **List row** | leading icon 24, title `body-l`, trailing value / chevron | h 56 | pressed: ink-700; selected: 3 px signal left rule | ink-800 |
| **Slot (loadout)** | square icon 48 + name + small stat | 72 × 72 | empty: dashed ink-500; selected: signal ring | — |
| **Stat bar** | label `label` + 8-segment bar + value | h 16 | compare: green/red delta segments with ▲▼ glyphs | heal / danger + glyphs |
| **Toggle** | pill 44 × 24, knob 20 | — | on: signal fill; disabled: 40 % | — |
| **Slider** | 4 px track, 20 px thumb (hit 48) + numeric value box | w 240 | dragging: thumb 24, value tooltip | signal fill |
| **Dropdown / stepper** | `‹ value ›` stepper is preferred (gamepad + touch friendly) | h 48 | — | — |
| **Chip** | r1, `label` caps, 18 tall | — | variants: team (glyph + colour), info (ice), score (score numeral) | — |
| **Banner** | notch-m left & right, `title` caps | h 36 | slide in from top `m-banner` | ice / score / warn (hazard stripes) / danger |
| **Modal** | e3, max w 480, title `display-l`/`title`, body, actions right-aligned (primary rightmost) | — | enter: scale 0.96→1 + fade `m-modal` | ink-800 |
| **Toast** | e2, icon + text, 3 s | w 320 | slide from top-right | ink-700 |
| **Progress bar** | 6 px track, fill, optional segments | — | indeterminate: 30 % block sweeping 1.2 s | signal / heal / ice |
| **Count-up numeral** | tabular `num-xl` | — | rolls digits 400 ms, ticks `ui_tick` every ~40 ms | score / paper |
| **Tooltip / key hint** | `micro` caps in ink-700 r1 chip, glyph for input device (auto-swaps KB / pad / touch) | 18 tall | — | — |
| **HUD block** | outlined content, no box; gradient fade where a backing is needed | see HUD_SPEC | danger state = danger numeral + pulse | — |
| **Callout strip** | `tab` 6 px colour + `slab` (paper/ice, ink-900 caps `title`) + optional `leader` (ink-900, value) | h 30–36 | build-in `m-wipe`; out 240 ms | signal / ice / score |
| **Score stack** | header tab (status lamp + `MODE · RD n`) + rows: 5 px team rule, glyph, name, score | row h 15 logical | own row paper text; value change `m-count` | team-*, score |
| **Corner marks** | 4 L-corners, 2 px, 12 px arms (`.brk`) | any | selected = signal; framing = paper 30 % | — |
| **Grid reference / status lamp** | mono `micro` `GRID C4 · SHEET 02`; 7 px dot with `m-pulse` | — | menus and results only (HUD: score-stack header) | ink-300, signal |
| **News strip** | `NEWS` signal tab + scrolling `label` caps messages | h 40 | scroll 40 px/s; pauses on focus; off with Reduce motion | ink-950 |
| **Hazard stripes** | 45° `warn` / `ink-950` stripes, 6 px | any | collapse and map-event warnings only | warn |
| **Text field** | h 48, ink-700, 1 px ink-500, caret signal | — | focus: 2 px signal border | — |

## Implementation constraints ([decision index](../../../../docs/README.md) D8, D11)
- One SDF rect shader draws everything (fill, border, radii, **notch** as a per-corner cut, shadow): `rect(pos, size, radii[4], notch[4], fill, border, border_w, shadow)`.
- Text is laid out by parley and drawn from MSDF atlases baked offline (Big Shoulders 800/900, Barlow Cond 600/700, Barlow 400/500, JetBrains Mono 400: Latin + Latin-Ext + Cyrillic + Greek ≈ 900 glyphs). CJK comes later.
- Icons are an MSDF atlas of in-house SVGs (≤ 128 icons at 48 px source).
- **Target: ≤ 6 draw calls per menu screen** (rects, text, icons, one 3D preview, blur, scrim).
