# platform-split: folder zones in check-layers, so games stay decoupled from the engine (build step 1)

Goal: `cargo run -p check-layers` enforces the platform rules in `docs/ARCHITECTURE.md` §0 (decision D24):
- the engine never depends on a kit or a game;
- kits never depend on a game;
- tools depend only on the engine;
- a game depends only on the engine, kits and its own crates, never on another game;
- engine and kit sources never contain a path into `games/`.

The folders have already moved (`engine/*`), and `layers.json` already uses the new paths. This task adds the zone rules and their tests.

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §0–1, then every file in `tools/check-layers/src/` and `tools/check-layers/layers.json`.

## Design

Layer line: `tools/check-layers` stays in `tools/*` (layer 5, zone `tools`). It depends on no engine crate.

**Config (`rules.rs`).** `LayerConfig` gains `zones: Vec<Zone>`, and keeps `deny_unknown_fields`.

```rust
pub struct ZoneName(String);          // newtype, non-empty
pub enum ZoneScope { Shared, PerGame } // serde: "shared" | "per_game"
pub struct Zone {
    pub name: ZoneName,
    pub patterns: Vec<String>,        // same glob syntax as layer rules
    pub may_use: Vec<ZoneName>,       // zones this zone's crates may depend on (own zone must be listed to allow it)
    pub scope: ZoneScope,             // PerGame: an edge inside the zone needs the same first two path segments (games/<name>)
    #[serde(default)] pub forbid_text: Vec<String>, // substrings that must not appear in this zone's source files
}
```

Add `LayerConfig::validate(&self) -> Result<(), ConfigError>`, called by `run` after parsing. It rejects:
- duplicate zone names;
- a `may_use` that names an unknown zone;
- an empty `patterns` list;
- an empty `forbid_text` entry.

`ConfigError` is a new `thiserror` enum. Wire it into `lib.rs` `Error` as `InvalidConfig`.

**Judgement (`check.rs`, still pure).** `find_violations` also checks zones. New `Violation` variants, each with a message that says how to fix it:
- `NoZone { package, dir }`: the crate matches no zone pattern.
- `AmbiguousZone { package, dir, zones }`: the crate matches several zones.
- `ZoneEdge { from, from_zone, to, to_zone, kind }`: `to_zone` is not in the `may_use` list of `from_zone`.
- `CrossGameEdge { from, from_game, to, to_game, kind }`: inside a `PerGame` zone, the first two path segments differ.
- `ForbiddenText { file, line, text, zone }`: from the text scan below.

These apply to normal, dev and build dependencies, the same as the layer rule. The existing layer violations stay unchanged.

**Text scan (`scan.rs`, new; the only new I/O).**
- `pub fn scan_zone_sources(root: &Path, crates: &[(String /*dir*/, &Zone)]) -> Result<Vec<SourceFile>, Error>` reads every `*.rs`, `*.toml`, `*.wgsl` and `*.ron` file under each crate directory, skipping `target/`.
- A pure `fn find_forbidden(files: &[SourceFile], ...) -> Vec<Violation>` reports a 1-based line number for each match. Comments count; the rule is "no path into games/", full stop.

**`layers.json` after this task:**

```json
{
  "rules": [ ...existing engine/* and tools/* rules unchanged... ],
  "zones": [
    { "name": "engine", "patterns": ["engine/*"], "may_use": ["engine"], "scope": "shared", "forbid_text": ["games/"] },
    { "name": "kits",   "patterns": ["kits/*"],   "may_use": ["engine", "kits"], "scope": "shared", "forbid_text": ["games/"] },
    { "name": "tools",  "patterns": ["tools/*"],  "may_use": ["engine", "tools"], "scope": "shared" },
    { "name": "games",  "patterns": ["games/*/game", "games/*/modes/*"], "may_use": ["engine", "kits", "games"], "scope": "per_game" }
  ],
  "same_layer_allowed": []
}
```

**Docs:**
- Update the module headers of `check.rs`, `rules.rs` and `lib.rs`; they point to ARCHITECTURE §0–1.
- Update the `docs/briefs/README.md` row to `review`.
- Do not edit other docs; they are already updated.

## Tests

All with fixture metadata and fixture files, no cargo calls, in `tools/check-layers/src/*` unit tests or `tools/check-layers/tests/`:
- An engine crate depending on a kit gives `ZoneEdge`, and so does a kit depending on a game.
- A tool depending on a kit gives `ZoneEdge`; a tool depending on the engine is clean.
- `games/a/game` → `games/a/modes/x` is clean. `games/a/game` → `games/b/modes/x` gives `CrossGameEdge`.
- A dev-dependency crossing zones is caught too, with `kind = "dev"`.
- A crate matching no zone gives `NoZone`; a crate matching two zones gives `AmbiguousZone`.
- `validate`: a duplicate name, an unknown `may_use`, empty patterns and an empty forbid text each give their own `ConfigError`.
- Scan:
  - `"games/imagine/content"` on line 3 of a fixture `.rs` gives `ForbiddenText` at line 3;
  - the same text in a `tools` crate is clean;
  - files under `target/` are skipped.
- Proptest: for random small graphs of crates spread over the four zones, the reported `ZoneEdge`/`CrossGameEdge` set equals a reference model written plainly in the test.
- The real workspace passes: `cargo run -p check-layers` exits 0.

## Gates

```
cargo fmt --check
cargo clippy -p check-layers --all-targets -- -D warnings
cargo test -p check-layers
cargo run -p check-layers
```
(The `engine-physics` crate may fail to build in your base; it is being fixed elsewhere, so don't touch it. Report whether the workspace-wide gates pass.)
