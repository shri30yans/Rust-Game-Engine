# asset-source: games load content by logical path from one asset source; phone builds ship their content (2026-11-12)

**What went wrong.**
- On the Android emulator the pistol viewmodel is not drawn.
- The APK (`target/android/imagine-debug.apk`) contains 6 files and no game content.
- IMAGINE loads cooked assets from `concat!(env!("CARGO_MANIFEST_DIR"), "/../content/cooked/weapons")` (`games/imagine/game/src/present.rs:1024`), an absolute path on the build machine. It does not exist on a phone, so the load fails. Presentation errors are reported and survived, as designed, so the game runs without its gun.

**Why it was possible.**
1. Asset locations are compile-time file-system paths chosen by each game.
2. The build has no list of the content a game needs, so nothing packages it and nothing checks that it shipped.
3. A missing required asset is reported once in a log line, not by any test.

## How other engines do it
- **Unreal:** content is addressed by package path (`/Game/Weapons/Pistol`), cooked per platform, and staged into the build ("pak" files). A missing referenced asset fails the cook or the staging step.
- **Unity:** assets are addressed by GUID or Addressables key, and the build includes everything referenced.
- **Bevy:** `AssetServer` loads by path relative to an asset root. On Android the root is the APK's `assets/` through the `AssetManager`.

## Design (one path for phone and PC; owner, 2026-11-12: "the flow and engine path should be the same for phone and PC", no special asset logic)

1. **One content pack, read the same way everywhere.**
   - The cook writes the game's cooked content into one pack file, `content/cooked/<game>.pak`: a header, a table of `AssetPath`, then offset, size and content hash per entry, then the bytes.
   - The PC build, the phone build and the headless tests all load from that same pack through the same `PackReader` in `engine-assets`. There are no loose-file loads anywhere, on any platform.
   - Use the existing versioned record envelope (`light-cache-1`) for the header, if it has merged; otherwise the same magic, version and hash scheme.
2. **Logical asset paths.**
   - An `AssetPath` newtype in `engine-assets`, validated on construction: relative, no `..`, forward slashes.
   - Every `load` takes an `AssetPath`.
   - A game names its pack once in `AppConfig` and never builds a file-system path. Delete the `CARGO_MANIFEST_DIR` paths in IMAGINE and Sandbox.
3. **The one platform difference is opening the pack's bytes**, and it lives inside `engine-platform` as `open_content(name) -> impl Read + Seek`, exactly like an OS file API:
   - a file next to the executable, or under the game folder in dev, on PC;
   - the APK `assets/` entry through `AAssetManager` on Android.
   Nothing above `engine-platform` knows which one ran. No `cfg(target_os)` outside `engine-platform`.
4. **Hot reload uses the same path.** In dev, the cook rewrites the pack when content changes, and the reader reloads the entries whose hash changed. The game sees the same reload events as today.
5. **Packaging is checked, not trusted.**
   - `scripts/build-android.sh` copies the pack into the APK's `assets/`.
   - A test opens the built pack and checks every entry's hash.
   - A load of a path missing from the pack is a typed error, the same on every platform.
   - A test runs the IMAGINE match flow headless from the pack and asserts the viewmodel mesh is resident.
6. **Device check.** On the Android emulator (`$HOME/.local/trove-android/sdk/emulator/emulator -avd imagine_phone -gpu host -no-snapshot -no-audio` in the background, then `adb -s emulator-5554`):
   - install, start a match, and take a screenshot showing the pistol viewmodel;
   - report the APK size.

Done means:
- items 1 to 5 complete end to end, for IMAGINE and Sandbox;
- the four gates pass;
- the APK builds and contains its content.

Read first:
- `AGENTS.md`;
- `docs/CONTENT.md`;
- `engine/assets/src/` (`store.rs`, `load.rs`, `handle.rs`);
- `engine/app/src/asset_uploader/`;
- `engine/platform/src/window.rs` (the Android entry);
- `scripts/build-android.sh`;
- `tools/cook`;
- `games/imagine/game/src/present.rs`.

`light-cache-1` (not merged yet) also edits `engine/assets/src/store.rs` and `format/`. Keep your `store.rs` change to the source abstraction. Build on main as it is: Do not merge other branches; the reviewer integrates overlapping briefs at merge. `world-pipeline-2` changes how levels are found (the game reads the cooked level); load levels by `AssetPath` from the pack as for every other content type.

Do not touch `engine/render` or `engine/ui`.

Docs in the same change:
- `docs/CONTENT.md` (logical paths, the manifest, packaging);
- `docs/ARCHITECTURE.md` §1;
- a decision in `docs/README.md` §7: **D72**, one content pack read the same way on every platform;
- `games/imagine/docs/README.md` (Android notes);
- `docs/briefs/README.md` status `review`.

## Answers to the stop on 2026-11-12 (Codex asked how the Android pack reaches game setup; these are decisions)

Codex was right: today each game builds its whole client (and reads content) before the platform exists, and Android has its own entry (`run_client_android`, a per-game `android.rs` with `android_main`). That is a second start-up path, and it is why there was nowhere to hand the pack over. The fix follows Unreal's launch order, where the engine owns start-up on every platform: the platform layer starts, the content paks are mounted, and only then is the game module loaded and the game instance built (`FEngineLoop::PreInit`, then `Init`). Games never see which platform they run on.

**1. One start-up path, owned by the engine (scope added to this brief).**
- `engine-app` defines the one game entry:
  ```rust
  pub trait GameEntry {
      type Game: Game;
      type Presenter: Presenter<Command = <Self::Game as Game>::Command, Settings = <Self::Game as Game>::Settings>;
      const TITLE: &'static str;
      const CONTENT: ContentName;      // the game's pack, e.g. "imagine"
      fn build(start: &Start<'_>) -> Result<ClientApp<Self::Game, Self::Presenter>, AppError>;
  }
  pub struct Start<'a> { pub content: &'a ContentPack, pub device: &'a DeviceProfile, pub config: ConfigStore }
  ```
- `app::game_main!(ImagineEntry)` expands to the desktop `fn main` and, on Android, the `android_main` NativeActivity needs. It is the only `#[unsafe(no_mangle)]` in the codebase, written once in the engine with its `// SAFETY:` comment. Delete both games' `android.rs`, `run_client_android`, and every `build_client*` variant (`build_client_with`, `build_client_with_store`, `build_client_from`, the `file_data` flag, `ImagineGame::desktop()`).
- The order is the same on every platform, inside `engine-app` and `engine-platform`: start the platform (Android passes its `AndroidApp`, desktop passes nothing; this is the one place the platform handle exists), detect the `DeviceProfile` (class and touch from the platform), open the pack with `open_content(E::CONTENT)` and mount it with `PackReader`, call `E::build(&start)`, then run the window loop. The GPU still arrives later through `PlatformApp::init`, as today.
- Headless tests start the same way: `app::headless::start::<E>()` opens the same pack from disk through the same `open_content`, so tests exercise the real path.

**2. The platform reports the device; games never test the target.**
- `DeviceProfile` carries `class` and `touch: bool`, filled by `engine-platform`. Games read them from `Start`. Remove every `cfg!(target_os = ...)` and `cfg(target_os)` from `games/` and from `engine-app`. `check-layers` (or a small test in it) fails if `target_os` appears outside `engine/platform`.
- Touch controls follow `start.device.touch`. Testing the phone layout on desktop is a launch option the platform reads (`--touch`), like Unreal's "always show touch interface", not a separate constructor.

**3. All game content goes through the pack, not only meshes (scope added).** Unreal has no content compiled into the game binary. Today IMAGINE and Sandbox compile in, or read from build-machine paths, every kind of content: levels (`include_str!` of `*.level.ron`), UI documents and the theme (`menus.rs`, `present.rs`, `hud.rs`), data (`modes.ron`, `tuning.ron`, `sounds.ron`), sounds (`include_bytes!` of the wavs), fonts, and meshes. Each also has a second, desktop-debug-only file path with its own `MtimeWatch`.
- The cook puts all of it in the pack under logical `AssetPath`s, and the game loads all of it from `start.content`. Delete every `include_str!`/`include_bytes!` of content and every `CARGO_MANIFEST_DIR` path in non-test game code, and every `cfg!(debug_assertions)` that picks a content source.
- Parsers keep their unit tests on literal strings; anything that tests the game's real content reads it from the pack.
- The engine's own fallback font (used by the error banner and the telemetry panel) stays compiled into `engine-app`: it is engine data that must draw when a game's pack failed to open, which is what the banner reports.

**4. One hot-reload path.** Delete the per-type watchers (`MenuDocumentWatch`, the tuning, sound, level and HUD file watches). `PackReader` watches its source when the platform says the source can change (a file on desktop; an APK asset never does), and reports changed `AssetPath`s by content hash. Each system that loaded an asset reloads it on that one event. In dev the cook's watch mode rewrites the pack when a source changes. There is no build-type switch: the same binary hot-reloads whenever its pack changes.

**5. The pack is a checked build output.** Commit `content/cooked/<game>.pak` like the cooked meshes today, and add a test that a fresh cook produces the same pack bytes, so a stale pack fails with "run `cargo run -p cook -- <game>`". Report the pack size. If it is over 50 MB, stop and say so.

**Order with other work.** `device-tier` waits for this brief and builds the GPU part and the tier on this start-up path. `light-cache-1` (running) edits only `engine-assets/src/format/`. Do not merge other branches; the reviewer integrates overlapping briefs at merge. `world-pipeline-2` (Kiro, running) edits the live scene and level loading; the level it reads comes from the pack by `AssetPath`.

Docs in the same change, in addition to the list above: **D73**, the start-up path (engine-owned entry and launch order, as Unreal), in `docs/README.md` §7 and `docs/ARCHITECTURE.md` §1 (`engine-app`, `engine-platform`).

## Answer to the third stop on 2026-11-12 (decision)

**`GameEntry::CONTENT` is the one place a game names its pack.** Delete the earlier wording "a game names its pack once in `AppConfig`": `AppConfig` gets no pack field. The engine's start-up path reads `E::CONTENT` and mounts that pack before calling `E::build`. Nothing else names it.

**Small choices are yours.** For naming, plumbing or where a function lives, pick the simplest option that keeps one path and one source of truth, say it in the report, and continue. Stop only when the goal itself conflicts.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
