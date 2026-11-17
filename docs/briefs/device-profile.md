# device-profile: the device profile in engine-platform (build step 2, D25/D33)

Goal: `engine-platform` builds a `DeviceProfile` at start-up that every other crate can read: device class, CPU core counts, memory, and the GPU's name, features and limits, with a known-bad GPU check. Done means: the profile builds on this Mac with and without a GPU, its pure rules are property-tested, and all four gates pass.

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §3 and §2.4, `docs/README.md` §7 D25 and D33, `engine/platform/src/{lib,gpu,error,headless}.rs`.

## Design

**Layering.** `engine/platform` stays layer 1. Add `sysinfo` (newest stable; owner approved this dependency) to `[workspace.dependencies]` and to `engine/platform/Cargo.toml` with `default-features = false` and only the features needed for `system` (CPU and memory). `sysinfo` must not be pulled into `wasm32` builds: put it under `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`.

**New file `engine/platform/src/profile.rs`** (and add `pub mod profile;` and re-exports `DeviceProfile, DeviceClass, CpuProfile, GpuProfile, KnownBadGpu` in `lib.rs`). Header states: this is the one place that looks at the OS and hardware; every other crate reads the profile (D33).

```rust
/// What kind of device this is. Decided at compile time from the target, never by OS name elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass { Desktop, Handheld, Web, Server }

impl DeviceClass {
    /// Desktop for macOS/Windows/Linux, Handheld for iOS/Android, Web for wasm32.
    /// `Server` is never detected; callers ask for it with `DeviceProfile::server`.
    pub const fn for_target() -> Self;   // cfg!(target_os = "ios" | "android"), cfg!(target_arch = "wasm32")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuProfile {
    pub logical_cores: NonZeroUsize,
    /// Estimate, see `estimate_performance_cores`. Always 1..=logical_cores.
    pub performance_cores: NonZeroUsize,
}

pub fn estimate_performance_cores(frequencies_mhz: &[u64], logical: NonZeroUsize) -> NonZeroUsize;
```
`estimate_performance_cores` is pure and documented: if the frequency list is empty, all zero, or every core reports the same frequency, every core counts as a performance core (homogeneous, or the OS does not say). Otherwise performance cores are those whose frequency is at least 75 % of the maximum (integer arithmetic: `f * 4 >= max * 3`, use `u64`, no float). The result is clamped to `1..=logical`. (The frequency source is `sysinfo`; Apple silicon reports 0, so it counts as "OS does not say". That limit is recorded as D35, below.)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuProfile {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub device_type: wgpu::DeviceType,
    pub backend: wgpu::Backend,
    pub features: wgpu::Features,   // adapter.features()
    pub limits: wgpu::Limits,       // adapter.limits()
    pub known_bad: Option<String>,  // reason if matched, else None
}

/// One entry of a known-bad GPU list. A game or the engine app hands the list in; this crate ships none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBadGpu { pub vendor: u32, pub name_contains: String, pub reason: String }

impl GpuProfile {
    pub fn from_adapter(adapter: &wgpu::Adapter, known_bad: &[KnownBadGpu]) -> Self;
    pub fn matches_known_bad(info: &wgpu::AdapterInfo, list: &[KnownBadGpu]) -> Option<String>;  // pure on AdapterInfo
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceProfile {
    pub class: DeviceClass,
    pub cpu: CpuProfile,
    /// Total physical memory in bytes; `None` where the platform does not report it (web).
    pub memory_bytes: Option<u64>,
    /// `None` for servers and headless runs that never create a GPU.
    pub gpu: Option<GpuProfile>,
}

impl DeviceProfile {
    /// Probe CPU and memory for this machine; attach the GPU if given.
    pub fn detect(class: DeviceClass, gpu: Option<GpuProfile>) -> Result<Self, PlatformError>;
    /// A server profile (`DeviceClass::Server`, no GPU).
    pub fn server() -> Result<Self, PlatformError>;
    /// Worker count for the job pool (D25), see `worker_count`.
    pub fn job_workers(&self) -> usize;
}

pub fn worker_count(class: DeviceClass, cpu: &CpuProfile) -> usize;
```
`worker_count` (pure) is exactly D25: Desktop and Server: `logical_cores - 2`, at least 1; Handheld: `performance_cores - 1`, at least 1 (use `saturating_sub` then `max(1)`); Web: 0. `detect` uses `std::thread::available_parallelism()` for `logical_cores` on every target (on wasm32 too: map a failure to `PlatformError::CpuCount(std::io::Error)`); memory and frequencies come from `sysinfo` on non-wasm32 (refresh only what is needed: `System::new()` then `refresh_cpu_frequency()` / `refresh_memory()`, `RefreshKind` with `MemoryRefreshKind::nothing().with_ram()` and `CpuRefreshKind::nothing().with_frequency()`, adjusting to the API of the version you add) and `None` / empty frequencies on wasm32 via `#[cfg]`, not at runtime. `sysinfo` reports memory 0 on some platforms: treat 0 as `None` through a small pure helper `memory_from_total(total: u64) -> Option<u64>` (tested).

Add to `error.rs`: `CpuCount(#[source] std::io::Error)` with `#[error("cannot determine the CPU count: {0}")]`. Do not use `unwrap_or` to hide it.

**Wiring.** `Gpu` in `gpu.rs` gets `pub fn profile(&self, known_bad: &[KnownBadGpu]) -> GpuProfile` (calls `GpuProfile::from_adapter(&self.adapter, known_bad)`). Do not change the `PlatformApp` trait or any existing signature.

**Docs (same change).**
- `docs/README.md` §7: add `**D35: The performance-core count is an estimate from CPU frequencies** (2026-11-11, decided)` after D34. Text: sysinfo does not report performance versus efficiency cores; we estimate from per-core frequency (≥75 % of the maximum), and where the OS reports nothing (Apple silicon) every core counts. On a phone this can size the pool one or two workers too high until the reference phones are measured in build step 2 (PERFORMANCE §1); the pool size can then be set by a per-device override in the app settings, which `engine-app` provides. Why: `unsafe` sysctl or JNI calls are forbidden by AGENTS rule 4 and a per-model table does not scale. Rejected: libc `sysctl` with `unsafe`; assuming half the cores are efficiency cores.
- `docs/ARCHITECTURE.md` §1 `platform` row State: `built (no gamepad yet; device profile built)`; `docs/README.md` §2: mention the device profile.
- Leave `docs/briefs/README.md` statuses to the reviewer.

## Tests

`engine/platform/tests/profile.rs` (no allocator; no GPU needed):
- `worker_count`: Desktop with logical 1, 2, 3, 8 gives 1, 1, 1, 6; Server same; Handheld with performance 1, 2, 4 gives 1, 1, 3; Web is 0 whatever the cores. Proptest: result is never 0 except for Web, never exceeds `logical_cores`, and is monotonic non-decreasing in `logical_cores` for Desktop.
- `estimate_performance_cores`: empty list, all zero, all equal give `logical`; `[3000,3000,2000,2000,2000,2000]` with logical 6 gives 2; boundary: `[1000, 750]` gives 2 and `[1000, 749]` gives 1; the result is always within `1..=logical` (proptest with arbitrary frequency lists and logical in 1..=64; the list length may differ from logical, the clamp still holds).
- `matches_known_bad`: build `wgpu::AdapterInfo` values by hand (use `wgpu::AdapterInfo { name, vendor, device, device_type, ..}` with the fields of the wgpu version in the workspace); matches on vendor and case-sensitive substring; no match for another vendor; first match wins; empty list gives `None`.
- `memory_from_total(0)` is `None`, non-zero is `Some`.
- `DeviceProfile::detect(DeviceClass::for_target(), None)` on this machine: `logical_cores >= 1`, `performance_cores <= logical_cores`, `memory_bytes` is `Some` and > 0 (this test is compiled out on `wasm32`). `DeviceProfile::server()` has class `Server` and no GPU. `job_workers()` equals `worker_count(...)`.
- GPU test: try to create a headless `Gpu` the way `headless.rs` does; when no adapter exists, print `skipped: no GPU adapter` and return (the existing tests in the crate show the pattern; follow it). With a GPU: `GpuProfile.name` is non-empty and `limits.max_texture_dimension_2d > 0`.

## Hard rules (a review fails on any of these)

No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`, no panicking indexing, no `as` numeric casts in non-test code (use `u64::from`, `try_from`, `NonZeroUsize::new` with a typed error or `.max(1)` via `NonZero::MIN`). No `#[allow(...)]` in non-test code. No new `unsafe`. No swallowed errors (`let _ =`, `.ok()`, `unwrap_or*` that hides a failure, empty `Err(_)`). No stubs, TODOs, dead code or duplicate implementations. No `sleep` in tests, no `#[ignore]`. Test files may use `unwrap`/`expect` with a module-level `#![allow(clippy::unwrap_used, clippy::expect_used)]`.

Do not touch: other crates, other files except `Cargo.toml` (workspace dependency `sysinfo`), `engine/platform/**`, and the three doc files named above.

## Gates

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```

Run `cargo test -p engine-platform` 10 times before finishing; all must pass.
