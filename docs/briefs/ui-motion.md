# ui-motion: paint-only transitions and keyframes, and virtual lists (docs/research/2026-11-12-ui-engines.md U5, U6)

Professional menus move: panels slide in, buttons respond within 80 ms, numbers count up, lists scroll smoothly. Unreal's invalidation and the browser's compositor both teach the same rule: animate paint, never layout. Gameface and Unreal's ListView teach the second rule: long lists create only the rows on screen.

Done means:
1. Documents can use CSS `transition` and a `@keyframes` subset on paint-only properties.
2. A layout property in a transition or keyframe is a cook error.
3. Animated frames never re-run layout.
4. Repeated templates in scroll views are virtual.
5. The four gates pass.

Starts after `ui-actions` merges (all edit `engine/ui`).

Read first:
- `AGENTS.md`;
- `docs/research/2026-11-12-ui-engines.md`;
- `games/imagine/docs/design/MOTION_SOUND.md`;
- `engine/ui/src/{layout,menus,template}.rs` and the ui-kit-3 scroll view;
- `tools/ui-cook/src/css.rs`;
- the Unreal UMG optimisation guidelines (invalidation): https://dev.epicgames.com/documentation/en-us/unreal-engine/optimization-guidelines-for-umg-in-unreal-engine

## Design
**1. Paint-only properties.**
- Allowed: `opacity`, `transform` (translate, scale, rotate, about the box centre or `transform-origin`), `color`, `background-color`, gradient stop colours, `border-color`, and the image tint.
- These are stored per node as a paint delta applied at draw time. Layout never reads them.
- `transition: <prop> <ms> <easing> <delay>`, with easings from the theme's motion tokens (named curves, plus `cubic-bezier`).
- `@keyframes name { 0% {…} 100% {…} }`, and `animation: name <ms> <easing> <iterations|infinite> <direction>`. State styles (`:hover`, `:active`, `:focus`, `[selected]`) trigger transitions.

**2. Reduced motion.** The existing setting scales durations to the reduced-motion values from the theme, or snaps them.

**3. Virtual lists.**
- A `data-repeat` inside a scroll view creates only the visible rows plus one row of buffer, and reuses row nodes as the list scrolls.
- Rows have a fixed height, from the document or measured once from the first row. Variable height is out of scope; say so.

**4. Number count-up.** A text binding marked `data-count-up="<ms>"` animates its number from the old value to the new one. It is used by Results.

## Tests
- a cook error for a layout property in a transition;
- an animated frame does no layout (a counter) and allocates nothing;
- easing values at known times;
- reduced motion;
- a virtual list of 10,000 rows creates at most visible + 2 row nodes, and scrolling to the end shows the right rows;
- the goldens at the mid-transition time.

**Docs:**
- `docs/ARCHITECTURE.md` §1;
- a decision in `docs/README.md` §7 (paint-only motion, virtual lists);
- the ui-cook CSS subset list;
- `docs/research/2026-11-12-ui-engines.md` U5 and U6;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
