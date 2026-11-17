# android-sandbox: touch controls and a signed Android APK of the sandbox (build step 2, D33)

Goal: `scripts/build-android.sh` produces `target/android/sandbox-debug.apk`, a signed arm64 APK of the sandbox that opens full screen and is playable by touch (left half: floating stick to move, right half: drag to look, a quick extra tap not needed: jump is a second finger on the right half, see below). Done means: the APK builds from a clean checkout with the commands in the docs, passes `apksigner verify` and `aapt2 dump badging`, all four gates pass on the host, and the Android code is cross-compiled in the build. There is no device or emulator on this machine, so on-device behaviour is NOT verified by this brief: say so plainly in the docs and the final message; do not claim it runs.

Read first: `AGENTS.md`, `docs/README.md` §7 D33, `engine/platform/src/{input,window,lib}.rs`, `engine/app/src/client.rs` and `late_camera.rs`, `games/sandbox/game/*`, `games/sandbox/docs/README.md`.

Environment (verified): `ANDROID_HOME=$HOME/.local/trove-android/sdk`; NDK at `$ANDROID_HOME/ndk/<version>/` (27.1.12297006 present; pick the highest); build-tools `aapt2`, `zipalign`, `apksigner` under `$ANDROID_HOME/build-tools/<version>/`; platforms `$ANDROID_HOME/platforms/android-36/android.jar`; JDK `java`/`keytool` on PATH; Rust target `aarch64-linux-android` installed. `cargo-ndk` is NOT installed and must not be installed: link with the NDK clang directly (`$NDK/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android26-clang`, check the real prebuilt folder name). No other downloads.

## 1. Touch input (`engine/platform/src/input.rs`)
Today the left half of the screen is a floating stick (feeds `move_axis`) and the right half is "fire". Keep that, and add (with tests in the same file's test module or `tests/`):
- `pub touch_look: Vec2` (pixels moved this frame by the right-half *look* touch; zeroed in `end_frame`). The first touch down on the right half is the look touch (`look_touch: Option<(u64, Vec2)>` holding id and last position); `touch_move` adds `position - last` to `touch_look` and updates last; `touch_end` clears it. A SECOND simultaneous right-half touch is the fire/jump touch (`fire_touch`). So `fire()` is true only while a second finger is down on the right half (existing keyboard `Key::Fire` still works). Update `touch_start`'s doc comment.
- `pub fn stick(&self) -> Vec2`: only the touch stick's deflection, x right, y forward, each in [-1, 1] and clamped to unit length (same 80 px full-deflection as `move_axis`; share one private helper so there is one implementation). `move_axis` keeps its meaning.
Tests: first right touch looks, second fires; moving accumulates deltas; `end_frame` zeroes `touch_look`; stick deflection values; left touch does not look.

## 2. Engine app (`engine/app/src/client.rs`)
In `run_frame`, after the mouse-look step, also turn the view by `sensitivity.delta(input.touch_look)` (touch look needs no grab). Factor the whole look step (mouse when grabbed, plus touch) into `fn turn_view(&mut self, input: &Input) -> Result<(), AppError>` and unit-test it without a GPU (build a `ClientApp` as the existing tests do, set `input.touch_look` and `input.mouse_delta`/`grabbed`).
`Presenter::grab_mouse` stays as is.

## 3. Platform entry for Android (`engine/platform`)
`#[cfg(target_os = "android")] pub use winit::platform::android::activity::AndroidApp;` from `lib.rs`, and in `window.rs` `#[cfg(target_os = "android")] pub fn run_android<A: PlatformApp>(android: AndroidApp, title: &str, size: (u32, u32), app: A) -> Result<(), PlatformError>` building the event loop with `EventLoop::builder().with_android_app(android).build()?` (`EventLoopBuilderExtAndroid`) and calling the existing `run_on`. Do not add unsafe code here. In `engine/app/src/client.rs` add `#[cfg(target_os = "android")] pub fn run_client_android(android: AndroidApp, title, size, client: &mut ClientApp<..>) -> Result<(), AppError>` next to `run_client` (same error handling; share the tail through a private helper so there is one implementation).

## 4. Sandbox (`games/sandbox/game`)
- `Cargo.toml`: `[lib] crate-type = ["lib", "cdylib"]` (keep the desktop `[[bin]]`).
- Presenter: forward / strafe from `Input::stick()` as well as the keyboard (turn the stick into `Axis` with a dead zone constant `STICK_DEAD_ZONE = 0.3`, named and tested via a pure function), jump from `input.fire()` or Space. Touch look reaches the view through the engine (item 2).
- `src/android.rs` (`#[cfg(target_os = "android")]`): the one entry point winit needs:
  ```rust
  #[unsafe(no_mangle)]
  // SAFETY: `android_main` is the symbol the Android glue looks up by name; it is called once on the activity thread.
  pub fn android_main(android: platform::AndroidApp) { /* build the client through a shared `fn build_client() -> Result<ClientApp<..>, AppError>` in lib.rs (also used by main.rs), call `run_client_android`; on any error call `platform::log_error(&error.to_string())` and return */ }
  ```
  `platform::log_error(&str)` is new in engine-platform: on Android it calls `__android_log_write` (liblog.so) through the same dlopen/dlsym pattern `android::frame_rate` uses; on other targets it writes to stderr. It is how an error becomes visible; nothing is swallowed.
  Keep the `unsafe` surface to exactly the `no_mangle` attribute here and the one FFI call for logging in platform (annotate with `// SAFETY:` like the existing one; the `#![forbid(unsafe_code)]` in the sandbox crate becomes `#![deny(unsafe_code)]` with a single `#[allow(unsafe_code)]` on the android module, and a test (host side) that asserts `log_error` does not panic on the host).
- Document the unsafe exceptions in `docs/README.md` §7 as D38 (2026-11-11, decided): "Android entry point and logging use two narrow unsafe exceptions", with why (winit's native-activity glue requires an unmangled `android_main`; there is no safe logging API without a new dependency) and rejected (`cargo-apk`/`android-activity` macros and the `log` crates: new dependencies).
- `games/sandbox/android/AndroidManifest.xml`: package `com.imagine.sandbox`, `android:minSdkVersion="26"`, `android:targetSdkVersion="35"`, `<application android:label="Sandbox" android:hasCode="false">`, one `<activity android:name="android.app.NativeActivity" android:exported="true" android:configChanges="orientation|keyboardHidden|screenSize|smallestScreenSize|screenLayout|uiMode" android:screenOrientation="sensorLandscape" android:theme="@android:style/Theme.NoTitleBar.Fullscreen">` with `<meta-data android:name="android.app.lib_name" android:value="sandbox"/>` and the MAIN/LAUNCHER intent filter; `<uses-feature android:glEsVersion="0x00030000"/>` and `<uses-feature android:name="android.hardware.vulkan.level" android:required="false"/>`.

## 5. `scripts/build-android.sh`
`set -euo pipefail`; honours `ANDROID_HOME` (default as above) and fails with a clear message listing what is missing; selects the highest NDK and build-tools version; runs
`CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=<clang> CC_aarch64_linux_android=<clang> AR_aarch64_linux_android=<llvm-ar> cargo build --release --target aarch64-linux-android -p sandbox --lib`;
`aapt2 link --manifest ... -I android.jar --min-sdk-version 26 --target-sdk-version 35 -o target/android/base.apk`; adds `lib/arm64-v8a/libsandbox.so` with `zip` (stored paths must be exactly `lib/arm64-v8a/libsandbox.so`; use a staging directory under `target/android/stage`), `zipalign -p -f 4`, creates `target/android/debug.keystore` once with `keytool -genkeypair` (alias `androiddebugkey`, storepass/keypass `android`, RSA 2048, validity 10000 days), signs with `apksigner sign --ks ... --ks-pass pass:android`, then runs `apksigner verify --verbose` and `aapt2 dump badging` and prints the package name, SDK versions and the native-code ABI; ends by printing the APK path and size. Idempotent (re-run safe). If the Rust build for Android fails because a dependency does not support Android (for example `sysinfo` features), fix it in the right crate with a `cfg(target_os = "android")` choice that keeps a real implementation, never a stub; if you cannot, stop and report the exact error.

## 6. Docs
`games/sandbox/docs/README.md`: "Android build" section (prerequisites, `scripts/build-android.sh`, install with `adb install -r target/android/sandbox-debug.apk`, controls, and the sentence that on-device behaviour was not verified because no device was available). `docs/README.md` §2 platforms row: the sandbox builds an Android APK (not yet run on a device). `docs/briefs/README.md` row for this brief to `review`.

## Gates (host)
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
cargo clippy --target aarch64-linux-android -p sandbox -p engine-platform -p engine-app --lib -- -D warnings
bash scripts/build-android.sh
```
(The Android clippy line may need the same linker env as the script; if so, run it through the script's env block.) Report the APK path, size, `apksigner verify` output and the badging summary.

## Do not
Add dependencies (none are allowed here). No `#[allow]` other than the single android-module `unsafe_code` allowance, no `unwrap`, `expect`, `unwrap_or*`, `.ok()`, `let _ =`, panicking indexing, truncating `as` casts in non-test code.
