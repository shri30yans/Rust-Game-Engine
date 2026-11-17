# ui-kit-3: clipping and scroll views, font faces, safe areas (the second half of ui-kit-2's goal; audit M6 and M7)

`ui-kit-2` was narrowed on 2026-11-12 to images, nine-slice and gradients, end to end. This brief does the rest of that goal from the same design: sections 3, 4 and 5 of `docs/briefs/ui-kit-2.md`, which are its source and are not repeated here. It follows the AGENTS.md core tenets, and these rules apply in particular:

**Correct by construction.**
- `SafeArea` is made only by `engine-platform`, from the OS (display cutout and gesture insets on Android, zero on desktop). Its constructor rejects negative or non-finite insets, and insets that leave less than half the screen. There is no public field and no test fixture in production code. Tests build it through a test-only helper in the test crate.
- A layer's root lays out inside the safe area unless the document marks it `full-bleed`. A document cannot read the insets in any other way than `env(safe-area-inset-*)`, which the cooker resolves to a typed reference.
- The scroll offset is node state owned by the scroll view. Layout never reads it, and the draw offset applies it, so scrolling never re-runs layout.
- The chosen font face is resolved at cook time: a nearest-weight match against the declared faces. A document that names an undeclared family is a cook error.

**Each feature works end to end, from HTML to pixels, or it is absent.**

Done means:
1. `overflow: hidden` clips, through a scissor stack.
2. Scroll views work with touch drag, momentum, the mouse wheel, and focus scrolling a row into view.
3. Font faces work: `@font-face`, `font-weight`, `letter-spacing`, `text-transform: uppercase`, and `text-shadow` drawn from the SDF. IMAGINE ships its OFL fonts with the licences.
4. Safe areas work on Android, checked on both test phones (`adb -s 95LFUSQKDAFQNNFU` and `adb -s 464783f9`): content stays clear of the cutout.
5. The tests listed in ui-kit-2 §Tests pass for these features, and the gallery goldens show them.
6. The four gates pass.

Starts after `ui-kit-2` merges.

## Design
As in `docs/briefs/ui-kit-2.md` §3, §4 and §5, with the rules above.

**Docs:**
- `docs/ARCHITECTURE.md` §1;
- a decision in `docs/README.md` §7;
- `docs/research/2026-11-11-engine-audit.md` M6 and M7;
- the ui-cook CSS subset;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
