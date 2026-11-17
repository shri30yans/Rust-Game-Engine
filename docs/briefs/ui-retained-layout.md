# ui-retained-layout: UI layout runs only when something changes, with no allocation per frame (audit H11, D54)

Goal: a HUD that does not change shape costs almost nothing per frame. Done means: (1) the taffy tree is built once when a document loads (and again on hot reload); (2) a frame whose bindings, screen size and text widths did not change returns the previous draw list without running layout; (3) a binding change re-lays out only when it can change geometry (text content, visibility, a length), and a change that only affects paint (a colour, a bar fill fraction drawn as a scaled rect if the document already treats it as paint) updates the draw list without layout; (4) a warm frame allocates nothing, proven by the counting-allocator test, including frames that change a text binding; (5) the output is byte-identical to today's for every fixture.

Read first: `AGENTS.md`; `docs/research/2026-11-11-engine-audit.md` H11; `docs/README.md` §7 D46, D48, D54; `engine/ui/src/{lib,doc,layout,bindings,template}.rs`; `engine/ui/tests/{layout,no_alloc}.rs` and `fixtures/`; `engine/app/src/ui_present.rs` (the only caller; read it, do not change its public use unless needed).

## Design

**Scope:** `engine/ui`; `engine/app/src/ui_present.rs` only if the `UiLayout` call shape must change (keep it minimal: another brief is editing `engine/app/src/client.rs`, do not touch that file).

- **Kept tree.** `UiLayout` owns the `TaffyTree` and a stable node map from document node index to taffy `NodeId`, built on `load(document)`. Hot reload calls `load` again.
- **Dirty tracking.** `Bindings` records a generation counter per key (no allocation: keys are interned to indices when the document loads; a binding the document never reads is ignored). Each document node knows which binding indices it reads and whether each read affects **layout** (text, `show_if`, lengths) or only **paint** (colour, opacity, fill). On a frame:
  - nothing changed (bindings, screen size, scale) → return the cached `DrawList`;
  - only paint-affecting bindings changed → rewrite those primitives in place in the cached list;
  - any layout-affecting binding changed, or the screen size changed → mark those taffy nodes dirty (`mark_dirty`), run `compute_layout`, rebuild the draw list into the reused buffers.
- **Text measurement** results are cached per (node, text generation, font size), so a re-layout does not re-measure unchanged text.
- **No allocation in steady state:** pre-size every buffer at `load` from the document (node count, max text length known from the template or grown once and kept). The existing `no_alloc` test is extended: 1,000 frames that change the score text, a health fill and a visibility flag allocate 0 bytes after the first 2 frames. If `taffy`'s `compute_layout` itself allocates on a dirty frame, measure it, report it, and keep layout frames rare (they happen only on change); state the number in the module header honestly.
- Update the module header invariants: the tree is kept; layout runs only on change.

**Tests:**
- Every existing fixture produces the same `DrawList` as before (snapshot equality against the old path; keep the comparison test only until the old path is deleted in this same change, then keep golden snapshots).
- Property test: for random sequences of binding changes and screen sizes, the retained path's output equals a fresh full layout of the same state.
- Counting-allocator test as above.
- A micro-bench (or timed test printed, not asserted) of a steady HUD frame before and after, in the report.

**Do not touch:** `engine/render`, `engine/app/src/client.rs`, `engine/platform`, `games/`, `tools/ui-cook` (unless a document format field is needed; then explain).

**Docs in the same change:** `docs/research/2026-11-11-engine-audit.md` H11 fixed; `docs/ARCHITECTURE.md` §1 `engine/ui` row; `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
