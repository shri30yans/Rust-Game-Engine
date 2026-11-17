# imagine-data: IMAGINE's arena and tuning become hot-reloaded data, and the build gets fast profiles (M0.2 + M0.6)

Goal: no content number or arena shape of IMAGINE lives in Rust, and every Rust change compiles in about a second into a build that plays at full speed. Done means: (1) while the game runs in a desktop debug build, editing `games/imagine/data/tuning.ron` (rifle damage, bot aim, respawn delay...) or `games/imagine/data/arenas/kiln_yard.level.ron` (move a wall) changes the running game within a second, with no compile; (2) a broken edit shows the error banner with file and reason and the game keeps running on the last good values; (3) release and Android builds embed the same files through the same loader; (4) the workspace has `dev`, `play` and `release` profiles as below, with measured build times in the report; (5) every existing IMAGINE test passes without changing its expected values (or the report explains exactly why a hash moved).

Read first: `AGENTS.md`; `docs/README.md` §7 D24, D41, D53; `games/imagine/docs/PLAN.md` §3.9 and §6.1; `engine/scene/src/{lib,model,source,tunables,watch,spawn}.rs`; `engine/app/src/{live_scene,error_banner}.rs`; `games/imagine/game/src/{data,scene,sim,rules,feedback,hud,hearing,autofire,present,lib}.rs` and `games/imagine/game/tests/`; `games/sandbox/game/` (the reference port to `engine-scene`, brief `docs/briefs/engine-scene.md`).

## Design

**Layering.** Only `games/imagine/game`, the root `Cargo.toml` profiles and docs change. `engine-scene` may get a small, game-agnostic addition if the port truly needs it (for example a `role` field on a shape that the game interprets as a string, like `Marker::kind`); no engine code names IMAGINE, Kiln Yard, a weapon or a mode (D24). If you add to `engine-scene`, add tests there.

**1. Tuning file.** `games/imagine/data/tuning.ron` holds every designer number now in `data.rs`: player and bot health and names, bot count, rifle (name, damage, rpm, magazine, reload, spread, range, ...), movement tuning, bot parameters (aim, reaction, hearing range), respawn delays, link rules. Shape:
- `data.rs` keeps only: `TICK_RATE_HZ`, the capsule and eye geometry the kit's scene builders must match, `ms_to_ticks`, and a `Tuning` struct (serde, `Validate` from `engine-scene`) with nested structs per group, plus conversion functions that build the kit's validated configs (`WeaponConfig`, `BotConfig`, `ControllerConfig`, ...) from it. Every conversion returns `Result`; invalid values are typed errors with the field path.
- Fixed-size arrays that depend on the bot count (`BOT_NAMES`, `BOT_SPAWNS`) become `Vec`s validated against each other (names count == bot count; enough bot spawns), checked in `Validate`.
- Code that reads constants (`rules`, `feedback`, `hud`, `hearing`, `autofire`, `sim`, `present`) reads them from the `Tuning` held in the world as a resource (D36 world resources) instead. No global, no `static`.

**2. Arena file.** `games/imagine/data/arenas/kiln_yard.level.ron`, an `engine-scene` `Level` that reproduces today's `scene::boxes()` exactly (same positions, sizes, order), with player spawns, bot spawns and the 25 waypoints as markers. `BoxRole` (what each box is for: floor, wall, cover, sight break...) is carried by the file (shape name prefix or a game-interpreted field; pick one, document it in the crate header and `games/imagine/docs/README.md`). Write a one-off test that compares the file's shapes with the old generator before deleting the generator, then delete `boxes()`, `push_*`, `PLAYER_SPAWNS`, `BOT_SPAWNS` and `WAYPOINTS`. `scene.rs` keeps only what reads the level into IMAGINE's `Scene` (entries, roles, hashing) and its tests (`stands_clear` and similar validation move into the level validation: every spawn and waypoint stands clear of solids, checked when the file loads).

**3. One loader, hot reload.** Use `LevelSource` and `Tunables` exactly as the sandbox does: `File` (path from `CARGO_MANIFEST_DIR`) when `cfg!(all(debug_assertions, not(target_os = "android")))`, else `Embedded(include_str!(...))`. Reload happens at a tick boundary in the client: tuning swaps in place (new values apply from the next tick; current health and ammo of living actors are not reset); a level change despawns and respawns the arena as `engine-app`'s `live_scene` does, and moves any actor left inside geometry to the nearest spawn. A failed reload keeps the last good value and shows `ErrorBanner` with the file and reason. Headless tests always use the embedded text so they never depend on the working directory.

**4. Build profiles** in the root `Cargo.toml` (D53):
```toml
[profile.dev]
opt-level = 1
[profile.dev.package."*"]
opt-level = 3
[profile.play]
inherits = "release"
lto = false
codegen-units = 256
incremental = true
debug-assertions = true   # keeps the hot-reload path (it is gated on debug_assertions)
overflow-checks = false
```
`release` stays as it is. Update `scripts/` run helpers and `games/imagine/docs/README.md` commands if they hard-code `--release` for playing (use `--profile play`; keep `--release` for Android release APKs and benches). Measure on this machine and put a table in the report and in README §7 D53's "Measured" sentence: clean build time per profile; incremental rebuild after a one-line change in `games/imagine/game/src/rules.rs` and in `engine/world/src/lib.rs` per profile; and the dev build's frame time in the arena (from the frame-telemetry overlay or a headless tick bench) compared with `play`.

**Decisions already made (do not change):** RON, not JSON or TOML; one loader with Embedded/File, no fallback path; no scripting language; no new watcher (reuse `engine-scene`'s `Watched`); the keep-last-good rule.

**Tests:**
- `tuning.ron` and `kiln_yard.level.ron` parse and validate (unit).
- Each validation rule rejects a bad value with the right field path (unit, one per rule group).
- Property test: any `Tuning` that validates converts into kit configs without error.
- Round trip: `Tuning` → RON → `Tuning` is equal.
- The arena file equals the old generator (then the generator is deleted; keep the test as "every spawn and waypoint stands clear").
- Hot reload: write a temp tuning file, load as `File`, change damage, poll, the new value is used on the next tick; write an invalid file, poll, the old value stays and the error is reported.
- Existing determinism and match tests pass unchanged.

**Do not touch:** `games/imagine/game/src/settings.rs`, `engine/app/src/client.rs`, `engine/platform` (another brief is changing them at the same time); `games/imagine/content/`, `tools/blender/`, `.agents/`, `*.apk`, `scripts/codex-delegate.sh`.

**Docs in the same change:** `docs/briefs/README.md` (status `review`), `games/imagine/docs/README.md` (where data lives, how to edit it live, the profiles), `games/imagine/docs/PLAN.md` §6.1 "Today" column for the rows this changes, `docs/README.md` §7 D53 measured numbers, `docs/ARCHITECTURE.md` §1 if `engine-scene` changed.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
