# frame-errors-and-settings: a bad frame no longer quits the game, and settings survive a restart (M0.3 + M0.4)

Goal: a playtest never ends because one frame failed, and the player's settings are kept between runs. Done means: (1) a forced error inside a frame (presenter draw, UI, camera, telemetry, input preparation) is logged, shown in the error banner, and the next frame draws normally; only errors at start-up, or a run of consecutive frame failures, stop the client; (2) IMAGINE's settings (per-axis look sensitivity and fire mode) are saved to the platform's config folder when they change and loaded at start, on desktop and Android; a missing, unreadable, invalid or newer-version file falls back to defaults with one log line, never a crash.

Read first: `AGENTS.md`; `docs/README.md` §7 D24, D33, D49; `docs/research/2026-11-11-engine-audit.md` finding C4; `games/imagine/docs/PLAN.md` §6.6; `engine/app/src/{client,error,error_banner,lib}.rs`; `engine/platform/src/{lib,error,window}.rs`; `games/imagine/game/src/{settings,present,android}.rs`.

## Design

**Part 1, frame errors (`engine-app`, `client.rs`).**
- Split `AppError` into what can be survived and what cannot, by type, not by a flag: add `AppError::is_fatal(&self) -> bool` implemented with an exhaustive `match` (no `_ =>` arm), so a new variant forces a decision. Fatal: renderer creation failures, `ClientNotReady` at init, exchange closed, tick overflow. Everything raised while drawing or presenting one frame is recoverable unless listed.
- Restructure `run_frame` so the renderer is never lost on a recoverable error: borrow `gfx` (`as_mut`) instead of `take`/put-back, or put it back on every path. Simulation errors and frame errors are kept apart: a failed simulation step is still fatal (the game state may be half-updated), a failed draw is not.
- A recoverable error: log it once per distinct message per second (no log flood), store it for the `ErrorBanner`, and continue. Count consecutive failed frames; after `MAX_CONSECUTIVE_FRAME_ERRORS` (a documented `const`, 120 = 2 s at 60 fps) the client stops with the last error, as today. One good frame resets the count.
- The banner shows the newest error and clears after 5 s without a new one.
- No allocation per frame on the success path (extend the existing no-allocation test if the client has one; otherwise state how you checked).

**Part 2, settings storage (`engine-platform`, game-agnostic).**
- New module `engine/platform/src/config.rs`: `ConfigStore`, built from an app id (validated: ASCII letters, digits, `-`, `_`, 1–64 chars; newtype `AppId`).
  - `fn read(&self, name: &ConfigName) -> Result<Option<String>, PlatformError>` (Ok(None) when the file does not exist).
  - `fn write(&self, name: &ConfigName, text: &str) -> Result<(), PlatformError>`: atomic (write `name.tmp`, then rename), creates the folder.
  - `ConfigName` newtype: a plain file name, no separators, no `..`.
  - Folder: macOS `~/Library/Application Support/<app_id>`, Linux `$XDG_CONFIG_HOME/<app_id>` or `~/.config/<app_id>`, Windows `%APPDATA%\<app_id>`, Android the activity's internal data path (pass it in from the Android entry point; the platform crate already receives the `AndroidApp`). Read environment variables once at construction; no global state. Prefer `std` only; if you add a crate (for example `dirs`), say why in the report and add it to `[workspace.dependencies]`.
  - A constructor that takes an explicit root folder, used by tests (temp dirs) and by headless runs.
- The engine knows nothing about which settings exist: it stores text.

**Part 3, IMAGINE settings (`games/imagine/game/src/settings.rs`).**
- `Settings` gets `serde` on a raw form with a `version: u32` field; `Settings::from_ron(text) -> Settings` validates (sensitivity steps clamped into `SENS_MIN..=SENS_MAX`, unknown version or parse failure ⇒ defaults + a returned warning the caller logs). `to_ron()` writes the current version.
- Load once at start in `present.rs` where `Settings::default()` is used today; save when a menu action changes a value (not every frame). Update the module header: settings are now written to disk.
- Keep the edit to `present.rs` to the settings load and save lines: another brief (`imagine-data`) is editing the same file at the same time.

**Decisions already made (do not change):** RON as the file format; settings are a game concern, storage is an engine concern; defaults on any bad file, never a crash.

**Tests:**
- `AppError::is_fatal` covers every variant (the exhaustive match is the test; add a unit test per fatal/recoverable group).
- Client: a test presenter that fails `draw` on frames 3 and 4 keeps the client running, draws frame 5, and reports the error; one that fails 120 frames in a row stops it. Use the headless or test-GPU path the crate already has; skip with a printed reason when no adapter.
- `ConfigStore`: write then read round trip in a temp dir; atomic write leaves no `.tmp`; bad `AppId` and `ConfigName` values are rejected (property test on names with `/`, `\`, `..`, empty, too long).
- `Settings`: round trip; a property test that any text gives valid settings (never panics, always in range); version mismatch gives defaults.

**Do not touch:** `games/imagine/game/src/{data,scene,sim,rules,feedback,hud,hearing,autofire}.rs`, `games/imagine/data/`, the root `Cargo.toml` profiles, `engine/scene` (the `imagine-data` brief owns them); `games/imagine/content/`, `tools/blender/`, `.agents/`, `*.apk`, `scripts/codex-delegate.sh`.

**Docs in the same change:** `docs/briefs/README.md` (status `review`), `docs/ARCHITECTURE.md` §1 (`engine-app` frame errors, `engine-platform` config store), `docs/research/2026-11-11-engine-audit.md` C4 marked fixed, `games/imagine/docs/PLAN.md` §6.6 and §7 rows, `games/imagine/docs/README.md` (where settings are saved).

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
