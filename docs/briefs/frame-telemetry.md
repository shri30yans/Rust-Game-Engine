# frame-telemetry: an always-available performance overlay

Owner request: "add fps telemetrics or something more, I want to see how the game is performing, up to 120 fps". Today only a smoothed fps number exists, hidden behind F3 (desktop only). Build real telemetry in the engine and show it in the games, on touch too.

Read first: `AGENTS.md`, `engine/app/src/client.rs` (`Presenter`, `frame`, governor), `engine/render/src/gfx/mod.rs` (`gpu_times`, `render_scale`), `engine/render/src/timer.rs`, `engine/platform/src/presentation.rs` and `engine/platform/src/android/frame_rate.rs`, `engine/platform/src/report.rs`, `games/imagine/game/src/{present,hud}.rs`, `games/sandbox/game/src/present.rs`.

## Design
**`engine/core` or `engine/app` `Telemetry`** (pure maths, no clocks: the caller passes each frame's wall time and the tick cost): a ring of the last N = 240 frame times, pre-sized in `new`, no allocation in `record`. `snapshot()` returns a `Copy` struct `FrameReport { fps, frame_ms_avg, frame_ms_p99 (the 1% low as frame time), frame_ms_max, refresh_hz_estimate, sim_ms_avg }`. `refresh_hz_estimate` = 1000 / median frame time rounded to the nearest of 30, 45, 60, 72, 90, 120, 144 (document why: under vsync frame time equals the refresh interval). Percentile by selecting from a scratch array held in the struct (no allocation). Validation: non-finite or negative times are errors (`AppError`). Unit tests incl. known sequences, percentile edge cases, 120 Hz and 60 Hz sequences estimate correctly, proptest: fields finite and `max >= p99 >= 0`; counting-allocator test: `record` and `snapshot` allocate nothing.

**Client wiring**: `ClientApp` records each frame's `dt` and the measured simulation time (`Instant` around `App::advance`+extract is allowed ONLY inside `engine-app`'s client runner, not in simulation crates; check the layering/lint rules and keep clock reads out of the sim) and exposes `ClientApp::report() -> FrameReport` plus gives the presenter access through a new defaulted `Presenter::observe(&mut self, report: &FrameReport, render_scale: f32, gpu: Option<&GpuTimes>)` called once per frame before `draw`. Add the governor's current render scale to what the presenter sees.

**Overlay**: a small function `draw_overlay(ui, font, report, render_scale, gpu)` in `engine-app` (engine code, game-agnostic) that draws a compact panel top-left (below the safe area): `120 fps  8.3 ms`, `1% low 9.1 ms  max 12.0 ms`, `display ~120 Hz  scale 100%`, `sim 0.4 ms`, and GPU frame ms when `gpu` is Some. Text colour changes when frame_ms_p99 exceeds the display interval (amber) or twice it (red). IMAGINE and the sandbox show it: desktop F3 toggles it (keep), and on touch it is ON by default and toggled by tapping the panel (add one invisible touch button via the existing touch-button layout, a small circle over the panel; do not add a hidden gesture). Remove the old smoothed `fps` field and `fps` line in imagine's HUD (one canonical path).

**High refresh honesty**: `presentation::bind_after_swapchain_configure` declares 120 Hz once per surface session via `android::frame_rate::declare_present_rate`; failures become `RefreshError` and log through `platform::log_error`. Measured fps in telemetry stays separate from declared present rate (`PresentationBinding` on the overlay).

## Tests
Unit and property tests above; an `engine/app` test (no GPU) that a presenter receives `observe` once per simulated frame with a report whose fps matches the scripted dt; a render-free test that `draw_overlay` queues text for the three severity colours (use the existing `Ui` test patterns, or assert on the queued vertex count). The headless GPU test must keep passing.

## Do not
No `#[allow]` (except the already-existing unsafe one), `unwrap`, `expect`, `unwrap_or*`, `.ok()`, `let _ =`, panicking indexing, truncating `as` casts in non-test code. No claim about phone performance or that 120 fps is reached on any device: you can only state the Mac numbers you measured, and you must measure one: run the sandbox or imagine headless/desktop for a few seconds and report the overlay's reported fps and p99 in your final report as a measurement, labelled as Apple M-series desktop.

Docs: briefs README row (new row `frame-telemetry`, set to `review`), ARCHITECTURE §1 app row, `games/imagine/docs/README.md` controls/telemetry paragraph, D38 third exception, README §7 entry D45 for telemetry (why percentile-from-ring, why refresh estimate is derived, rejected: GPU-timestamp-only (missing on many phones), smoothed fps only (hides stutter)).

## Gates
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
