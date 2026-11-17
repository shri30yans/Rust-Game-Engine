# imagine-device-fixes: the first phone test's bugs, and end-to-end tests that would have caught them

The first run of `imagine-debug.apk` on a real Android phone (2026-11-12) found:
1. **Starting a match crashes the app.**
2. **Adding bots in the lobby shows an error** that the configured bot names are fewer than the bots added.
3. **Button text does not fit inside its button.**

Each one is a bug a test should have caught. Done means:
- all three are fixed at their cause;
- an end-to-end suite drives the real client through every screen at phone and desktop sizes, with every lobby option, and fails on any of these;
- the four gates pass;
- a new APK builds.

Read first:
- `AGENTS.md`;
- `games/imagine/docs/` (README, PLAN §4, `design/MENUS_SPEC.md`);
- `games/imagine/game/src/{lib,flow,menus,session,present,settings,data,android}.rs`;
- `games/imagine/data/{tuning.ron,modes.ron}`;
- `engine/app/src/client.rs`;
- `engine/ui/src/{layout,doc,menus}.rs`;
- `scripts/build-android.sh`;
- `games/imagine/game/tests/`.

## Design

**1. The crash on match start.**
- Reproduce it first. No Android device or emulator is attached to this machine.
  - Run the desktop debug build and the headless flow test with the exact phone path: debug build, phone screen size 2400×1080, touch input, every lobby option.
  - If an Android device is attached (`adb devices`), install the APK and capture `adb logcat` around the crash.
- Likely suspects, to check first: anything that reads from the file system on Android in a debug build, or panics or errors at travel. That includes:
  - the dev menu hot-reload watcher and the HUD watcher (paths that do not exist on the device);
  - settings persistence (a desktop path on Android);
  - level or tuning loading by path instead of the embedded copy;
  - an error during travel that ends the client.
- **Rules:**
  - Dev hot reload on Android is off. It is gated by platform, not only by build type. Debug Android builds use the embedded content.
  - Settings persist to the platform's app data directory, given by `engine-platform`; add it if missing. Never use a relative path.
  - An error during travel shows an error banner and returns to Home. It never ends the client.
  - Write the actual cause in the report with the evidence.
- **Test:** an engine-app test where travel fails (a game whose setup returns an error) returns to the previous world, with the error reported, and the client keeps running.

**2. Bot names, made correct by construction.**
- Today `actors.bot_count` and `actors.bot_names` must be equal, while the lobby adds bots up to the room's capacity. Two numbers describe one thing.
- Replace them with a **bot roster**: a list of at least N unique names, where N is the largest bot count any mode in `modes.ron` allows (capacity − 1). `Tuning::validate` checks the roster against every mode and names the mode that does not fit.
- The lobby cannot add a bot past the mode's capacity. The add-bot control is disabled when the room is full, and the room code returns a typed error that is never shown as a raw message.
- Bot names are taken from the roster in order, deterministically for a given seed.
- Remove `bot_count`. The number of bots in a match is the room's bots. Grow the roster to fit the largest mode, using original call-sign style names.
- **Tests:**
  - every mode filled to capacity with bots starts a match;
  - adding one more is refused;
  - a roster shorter than a mode needs is a validation error naming the mode.

**3. Text that fits.**
- Find why text overflows at phone size. Check:
  - fixed button sizes against text measured at the font size;
  - `vmin` font units on a 20:9 screen;
  - missing text measurement in layout.
- **Rule, in `engine-ui`:**
  - A text node's intrinsic size is its measured text, so flexbox sizes buttons from their content unless the document fixes a size.
  - When a size is fixed and the text is too wide, the document chooses `text-overflow: ellipsis` or `fit: shrink` (down to a stated minimum size).
  - **Default:** `shrink` with a minimum of 75% of the size, then ellipsis.
  - Support the two properties in `tools/ui-cook`.
- **Test, the one that matters:** a layout check that runs over every IMAGINE document (all screens and the HUD) at 1920×1080, 2400×1080, 2340×1080 and 1600×720, with the longest real strings (the longest player and bot names, every mode name, and every setting label). It asserts that no text run's box extends outside its parent's box. Make the check an engine-ui function, so every game can run it.

**4. End-to-end suite** (`games/imagine/game/tests/e2e/`, one test binary):
- A scripted client at phone size with touch input, and at desktop size with mouse and keyboard. It drives:
  - boot → home → modes → each mode → lobby;
  - add bots to capacity, remove one, start;
  - loading → match: play 600 ticks with fire, move, jump, reload, ADS and crouch;
  - pause → settings (change each setting and confirm it applies) → resume;
  - leave match → home;
  - a second match → results → home.
- After every step it asserts:
  - the screen;
  - no error banner;
  - no text overflow (the check from §3);
  - the client is running.
- Covered as well:
  - travel failure recovery (§1);
  - a resize mid-match: phone rotation, so 2400×1080 → 1080×2400 → back;
  - suspend and resume of the GPU surface (Android pause): the renderer is dropped and rebuilt, and the match continues.
- Every step's screen is captured to a golden draw list at phone size, so layout regressions show as a diff.

**5. APK.**
- Run `scripts/build-android.sh imagine` at the end. Report that it builds, and list anything that differs on Android: features gated by platform, paths.

**Do not touch:** `engine/netcode`, `engine/physics`, `engine/render` (except a compile fix).

Other briefs are running at the same time:
- `imagine-combat-feel` edits IMAGINE's HUD, touch, settings and data;
- `render-assets-2` adds an asset uploader in engine-app;
- `world-pipeline` edits `engine/scene`.

Keep edits focused, and run `git merge main` before you finish.

**Docs in the same change:**
- `games/imagine/docs/README.md` (Android notes, how to run the e2e suite);
- `docs/ARCHITECTURE.md` §1 (ui and app rows);
- a new decision in `docs/README.md` §7:
  - travel errors return Home and never end the client;
  - a bot roster validated against modes;
  - text fit rules and the overflow check;
  - dev hot reload is desktop only;
- `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
