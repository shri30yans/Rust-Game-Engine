# android-lifecycle: the client's lifecycle is a state machine; nothing that Android does in a normal order can be an error (second phone, 2026-11-12)

On a realme GT 6T (RMX3853, Snapdragon 7+ Gen 3, Adreno 732, Android 16), logcat showed two lifecycle failures besides the touch crash, which `touch-from-layout` fixes:

```
E/imagine: the client or presenter was used before it was ready
E/imagine: imagine: platform error: event loop failed: EventLoop can't be recreated
```

**Why they were possible.**
1. **No lifecycle states.** `ClientApp` holds `gfx: Option<Gfx>`, and every callback checks the `Option` and calls `fail(ClientNotReady)` when it is `None`.
   - `PlatformApp::resize` before `init`, or while suspended (after `suspend` dropped `gfx`), is therefore fatal.
   - On Android, size and rotation events legitimately arrive before the first `Resumed`, and between `Suspended` and `Resumed`.
   - A normal event order is treated as a bug.
2. **One event loop per process, built per activity.**
   - `android_main` builds a new winit `EventLoop` every time it is called.
   - android-activity calls `android_main` again when the activity is recreated in the same process (relaunch from recents, `am start`, or a configuration change the manifest does not handle).
   - winit allows one event loop per process, so the second launch fails and shows nothing.

Done means:
1. Every legal lifecycle event order is handled with no error.
2. Illegal orders are unrepresentable.
3. A relaunch in the same process works.
4. Both are tested.
5. The four gates pass, and the APK builds.
6. The report covers both phones: RMX5085 (Mali-G720) and RMX3853 (Adreno 732).

Starts after `touch-from-layout` merges: both edit `engine/app/src/client.rs` and `engine/platform`.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D26 and D64, and the decision from `touch-from-layout`;
- `engine/platform/src/window.rs` (`Runner`, `run_android`, `run_on`, the `PlatformApp` trait);
- `engine/app/src/client.rs` (`PlatformApp for ClientApp`, `try_init`, `suspend`, `resume`, `fail`);
- `games/{imagine,sandbox}/game/src/android.rs`;
- the android-activity and winit (0.30) docs on `android_main` re-entry and `EventLoop` creation. Search the web, and cite what you used in the report.

## Design

**1. The lifecycle as a type, in `engine-app`.**
- `enum Surface { Suspended { size: (u32, u32) }, Ready(Ready) }`, where `Ready` owns `Gfx`, the error font, and anything else that needs the GPU.
- Callbacks are total:
  - `resize` while `Suspended` records the size, and while `Ready` resizes the GPU targets;
  - `frame` while `Suspended` is not called by the platform runner (it already has no surface), and if it is, it draws nothing and is not an error;
  - `suspend` moves `Ready` to `Suspended`, keeping the size;
  - `resume` and `init` move `Suspended` to `Ready` at the recorded size.
  No callback can produce `ClientNotReady`.
- Delete `ClientNotReady` from the lifecycle paths. If other uses remain (`asset_uploader`, `scene_present`), give each a specific error that says what is missing. A generic "not ready" is not allowed.
- The simulation keeps running while suspended only if it already does. Do not change match rules here.
- **Tests.** A table of event orders. Every one ends with the client running and drawing after the final resume:
  - resize before init;
  - resize while suspended;
  - suspend then resume twice;
  - rotation while suspended (2400×1080 → 1080×2400);
  - resume at a new size.

**2. One event loop per process.**
- Find the correct pattern for android-activity plus winit 0.30, and follow the library's documented guidance. Two candidates:
  - build the `EventLoop` once per process and reuse it for a re-entered `android_main`, if winit supports that on Android;
  - or end the process cleanly when the activity is destroyed, so a relaunch gets a fresh process. This is what many native Android games do.
- Pick the option the libraries document. Write the reason and the sources in a decision.
- Whichever it is, it lives in `engine-platform`. Games keep calling one function and never see it.
- **Test:** a platform test, or a documented manual check if the library cannot be driven headless. Then on the device:
  - `adb shell am start` twice;
  - background, then reopen from recents;
  - rotate.
  The game shows its menu every time.

**3. Device check (both phones are attached to this machine).** Use `adb -s <serial>`: RMX5085 is `95LFUSQKDAFQNNFU`, RMX3853 is `464783f9`. For each phone:
- install the APK;
- start the app, start a match, play 60 seconds;
- background, reopen, rotate;
- `am start` again.
Save logcat for the app's process and report the lines from `imagine`, `RustStdoutStderr` and `DEBUG`. If no device is attached when you run, say so; the owner will reconnect them.

**Do not touch:** `engine/render` internals, `engine/netcode`, `engine/physics`, `engine/scene`, `engine/ui`, `kits/`.

**Docs in the same change:**
- `docs/ARCHITECTURE.md` §1 (app and platform rows);
- a new decision in `docs/README.md` §7 (the lifecycle state machine; one event loop per process, with sources);
- `games/imagine/docs/README.md` (Android notes, both test phones);
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
