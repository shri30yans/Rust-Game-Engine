# ui-viewmodel: typed view models checked once at load; no binding errors at run time (docs/research/2026-11-12-ui-engines.md U1)

Today a game hands a document a `Bindings` map (name → `Value`) every frame, and a wrong name or type is found only when the document reads it, at run time. That is the same flaw as the touch crash: a mistake that tests should catch can reach a player.

Unreal's Viewmodel and Unity's runtime binding both bind a typed object, and update only what changed.

Done means:
1. Documents bind to typed view models.
2. Any mismatch between a document and its model is an error when the document loads, and tests run that load for every document.
3. Unchanged fields cost nothing per frame.
4. IMAGINE's HUD and menus use it.
5. `Bindings` as a per-frame name map is deleted.
6. The four gates pass.

Starts after `ui-kit-2` merges (`engine/ui`, `tools/ui-cook`).

Read first:
- `AGENTS.md`;
- `docs/research/2026-11-12-ui-engines.md`;
- `docs/README.md` §7 D46, D55, D65;
- `engine/ui/src/{bindings,template,layout,menus}.rs`;
- `tools/ui-cook/src/`;
- `games/imagine/game/src/{hud,menus}.rs`;
- the Unreal Viewmodel docs: https://dev.epicgames.com/documentation/unreal-engine/umg-viewmodel-for-unreal-engine

## Design
**1. The binding schema is cooked.**
- `ui-cook` writes, with each document, the fields it reads: name, kind (number, flag, text, image region, list) and, for lists, the row's field schema.
- The cooker infers kinds from use: `{x}` in text is text or a number formatted by the document; `data-show-if` is a flag; `data-repeat` is a list. Where use is ambiguous, the document states the kind.

**2. The model is a Rust type.**
- A `ViewModel` trait in `engine-ui`, implemented by a derive macro in a small proc-macro crate (`engine/ui-derive`), for a struct of fields whose types map to the kinds.
- The derive emits:
  - a static schema;
  - a per-field change flag, set by the generated setter only when the value really changes;
  - a `visit_changed` that walks only the changed fields.
- List fields are a typed `ListModel<Row>` with row ids, so inserts, removals and reorders mark only those rows.

**3. Bind once.**
- `UiScreen::bind::<M>()` checks the document schema against `M::SCHEMA` and returns a `Bound<M>`. A missing field, a wrong kind or an unused model field is a typed load error naming the document and field.
- After that, a frame takes `&mut M`, applies only changed fields to the nodes that read them, and allocates nothing.
- There is no way to set a field by string.

**4. Coverage.**
- A test in each game binds every document to its model. The `ui-preview check` (when it exists) does the same.
- Preview states in `ui/preview/*.ron` deserialise into the model type, so a stale preview state is a load error as well.

**5. Migrate IMAGINE.** Every HUD and menu document gets a model struct. Delete `Bindings`, `Value` and the per-frame map path. No shim.

## Tests
- a schema mismatch for each case (missing, wrong kind, unused) names the field;
- an unchanged frame does no binding work (a counter in tests) and allocates nothing;
- a list insert marks one row;
- every IMAGINE document binds;
- the existing goldens are unchanged.

**Do not touch:** `engine/render` (except the UI pass if node updates need it), `engine/netcode`, `engine/physics`, `engine/scene`, `kits/`.

**Docs:**
- `docs/ARCHITECTURE.md` §1 (ui, and a new ui-derive row in `layers.json`);
- a decision in `docs/README.md` §7 (typed view models, bind at load, change flags);
- the ui-cook docs (the schema);
- `docs/research/2026-11-12-ui-engines.md` U1 status;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
