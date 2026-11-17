# vk-0-phone-baseline: one bench path on desktop and phones, and the wgpu numbers Vulkan must beat (D82, D83)

Goal: measured numbers from both test phones for today's wgpu renderer. Done means:
1. The render bench lives in `games/sandbox` as engine test scenes and runs the same way on desktop and Android.
2. `scripts/phone-bench.sh` installs the sandbox APK on a connected phone, runs every case cold and after a 30-minute run, samples the phone's thermal state, and pulls the JSON back.
3. Results for both test phones (RMX5085 and RMX3853) are committed under `bench/phones/` and summarised in PERFORMANCE §1.
4. `bench-gate` runs the sandbox bench; `engine/render/examples/bench.rs` is deleted (one bench path).
5. `scripts/gate.sh` passes.

This brief does not change the renderer. Its numbers are the baseline the Vulkan port (vk-3) is judged against, so the scene, sizes and settings must stay fixed after this brief merges.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D45 (frame telemetry), D72 and D73 (content and start-up), D82, D83;
- `docs/research/2026-11-11-engine-audit.md` §E and §I;
- `engine/render/examples/bench.rs`, `tools/bench-gate/src/{lib.rs, main.rs}`, `bench/baseline/apple-m5.json`;
- `games/sandbox/game/src/lib.rs`, `scripts/build-android.sh`;
- `engine/platform/src/{config.rs, content.rs, lib.rs}`.

## Design

- **Layering.** No new crate. The bench scene and case list move into `games/sandbox` (the engine's test-scene home, D24, D83). `engine-platform` gains one small API (below). `tools/bench-gate` keeps comparing.
- **Files.**
  - `games/sandbox/game/src/bench/mod.rs`: declares and documents the module.
  - `games/sandbox/game/src/bench/cases.rs`: `pub struct BenchCase { pub name: &'static str, pub grid: u32, pub tier: RenderTier, pub msaa: u32, pub taa: bool, pub textured: bool, pub frames: u32 }` and `pub const CASES: &[BenchCase]`. Cases: the four in `bench/baseline/apple-m5.json` (High tier, same arguments as today), plus `grid40-low` (Low tier, MSAA 4×, no TAA) and `grid80-medium` (Medium, MSAA 4×, no TAA), which are the phone-relevant settings.
  - `games/sandbox/game/src/bench/scene.rs`: the block-grid scene from today's `bench.rs`, moved, not copied.
  - `games/sandbox/game/src/bench/report.rs`: `pub struct BenchReport` (serde) with `case`, `device` (GPU name, driver, OS), `size`, `tier`, `msaa_active`, `frames`, `wall_ms: Percentiles { p50, p95, p99, max }`, `gpu_frame_ms: Option<Percentiles>` (`None` when the device has no timestamp queries), `draws`, `upload_bytes`, `allocations_per_frame`, `phase: Cold | Sustained { minutes }`. One JSON line per case.
  - `games/sandbox/game/src/bench/run.rs`: runs a list of cases in one process, 60 warm-up frames each, writes the report lines.
  - Sandbox start-up: `--bench <case|all> [--sustain-minutes N]` on desktop; on Android, a request file `bench-request.ron` in the exchange folder (below) holds the same fields. No request means the normal sandbox.
- **Exchange folder.** `engine-platform` gains `pub fn exchange_dir(&self) -> Result<PathBuf, PlatformError>` on the start-up context: Android `external_data_path()` from android-activity (readable by `adb pull` and writable by `adb push`, no `run-as`, no debuggable build); desktop `target/bench-exchange/` under the working directory. It is the only new platform API. No JNI.
- **Sustained run.** `--sustain-minutes 30` loops the heaviest case (`grid80-medium`) for 30 minutes without writing, then runs every case again with `phase: Sustained { minutes: 30 }`.
- **Thermal.** `scripts/phone-bench.sh` samples `adb shell dumpsys thermalservice` (status and temperatures) every 10 s during the whole run into `thermal.jsonl` next to the results. This keeps thermal reading out of the engine until a JNI decision exists.
- **The script.** `scripts/phone-bench.sh [--serial S] [--sustain-minutes 30]`: builds with `scripts/build-android.sh sandbox`, installs, pushes the request, launches the activity with `adb shell am start`, waits for `bench-done` in the exchange folder (polls `adb shell ls` every 5 s, times out after the expected run time plus 10 minutes), pulls results into `bench/phones/<model>/<yyyy-mm-dd>/`, and prints a summary table. Keep the screen on with `adb shell svc power stayon true` during the run and restore it afterwards.
- **bench-gate.** `CASES` now come from the sandbox (`cargo run --release -p sandbox -- --bench all`). Baseline numbers for the four existing cases must be re-recorded on a quiet M5 (`--record "bench moved into sandbox"`) because the host process changed; add the two new cases.
- **What gets deleted.** `engine/render/examples/bench.rs` and its section in README §4.

Tests:
- `Percentiles` from a known sample list (nearest rank, same rule as D45).
- `BenchReport` JSON round-trip, and bench-gate parsing of the new line.
- Request file parse: valid, unknown case (error naming it), missing file (normal start).
- The bench run allocates nothing per frame after warm-up for one case (existing counting-allocator pattern).
- GPU test (skipped with a printed reason when there is no adapter): `--bench grid40-low --frames 30` writes one valid line.

Do not touch: `engine/render` internals, `engine/app` client logic beyond passing the start-up option through, `games/imagine`, `kits/`.

When a detail is not specified: pick the simplest correct design that keeps one code path, state it in the report, and continue. Stop only if the goal itself cannot be met.

Device runs: the owner connects each phone over USB; run the script once per phone and commit the results. If a phone is not connected, report it and leave its folder absent.

Docs in the same change: this brief's status in `docs/briefs/README.md`; README §4 commands; PERFORMANCE §1 phone table (cold and 30-minute numbers per phone and case); docs/research/2026-11-11-engine-audit.md C2 marked measured.

## Gates
scripts/gate.sh
