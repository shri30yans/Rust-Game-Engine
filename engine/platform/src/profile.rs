//! The one place that reads OS and hardware for capability sizing.
//! Every other crate takes a [`DeviceProfile`] built here at start-up.

use crate::PlatformError;
use std::num::NonZeroUsize;

/// What kind of device this is. Fixed at compile time from the target; nothing else names the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    /// macOS, Windows, or Linux.
    Desktop,
    /// iOS or Android.
    Handheld,
    /// `wasm32` in the browser.
    Web,
    /// Dedicated server (headless, no GPU); never auto-detected.
    Server,
}

impl DeviceClass {
    /// Desktop for macOS/Windows/Linux, Handheld for iOS/Android, Web for wasm32.
    ///
    /// [`Server`] is never detected; callers use [`DeviceProfile::server`].
    ///
    /// [`Server`]: DeviceClass::Server
    #[must_use]
    pub const fn for_target() -> Self {
        if cfg!(target_arch = "wasm32") {
            Self::Web
        } else if cfg!(any(target_os = "ios", target_os = "android")) {
            Self::Handheld
        } else {
            Self::Desktop
        }
    }
}

/// CPU layout used to size the job pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuProfile {
    /// Logical cores reported by the OS.
    pub logical_cores: NonZeroUsize,
    /// Estimate from per-core frequency; always `1..=logical_cores`.
    pub performance_cores: NonZeroUsize,
}

/// Minimum share of peak MHz (as a percent) for a core to count as performance.
const PERFORMANCE_FREQUENCY_PERCENT: u128 = 75;

/// Estimates performance cores from per-core MHz samples (pure).
///
/// If `frequencies_mhz` is empty, all zero, or every core reports the same frequency, every
/// logical core is treated as a performance core (homogeneous CPU, or the OS does not report MHz).
/// Otherwise a core counts when `mhz * 100 >= max * 75` (`u128` arithmetic, no floats). The
/// result is clamped to `1..=logical`.
#[must_use]
pub fn estimate_performance_cores(frequencies_mhz: &[u64], logical: NonZeroUsize) -> NonZeroUsize {
    if frequencies_mhz.is_empty() || frequencies_mhz.iter().all(|&mhz| mhz == 0) {
        return logical;
    }
    if let Some(&first) = frequencies_mhz.first()
        && frequencies_mhz.iter().all(|&mhz| mhz == first)
    {
        return logical;
    }
    let Some(max_mhz) = frequencies_mhz.iter().copied().max() else {
        return logical;
    };
    let max_u128 = u128::from(max_mhz);
    let threshold = max_u128 * PERFORMANCE_FREQUENCY_PERCENT;
    let count = frequencies_mhz
        .iter()
        .filter(|&&mhz| u128::from(mhz) * 100 >= threshold)
        .count();
    // At least one core matches (the maximum); `count` is never zero here.
    NonZeroUsize::new(count).map_or(logical, |n| n.min(logical))
}

/// GPU identity, capabilities, and optional known-bad match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuProfile {
    /// Adapter name from wgpu.
    pub name: String,
    /// PCI-style vendor id.
    pub vendor: u32,
    /// PCI-style device id.
    pub device: u32,
    /// Integrated vs discrete vs CPU, from wgpu.
    pub device_type: wgpu::DeviceType,
    /// Metal, Vulkan, DX12, etc.
    pub backend: wgpu::Backend,
    /// Features supported by the adapter.
    pub features: wgpu::Features,
    /// Limits enforced on this adapter.
    pub limits: wgpu::Limits,
    /// Reason string when the adapter matched `known_bad`, else `None`.
    pub known_bad: Option<String>,
}

/// One entry in a known-bad GPU list supplied by the app or a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBadGpu {
    /// Vendor id to match.
    pub vendor: u32,
    /// Case-sensitive substring of the adapter name.
    pub name_contains: String,
    /// Human-readable reason for logs and UI.
    pub reason: String,
}

impl GpuProfile {
    /// Builds a profile from a wgpu adapter and an optional blocklist.
    pub fn from_adapter(adapter: &wgpu::Adapter, known_bad: &[KnownBadGpu]) -> Self {
        let info = adapter.get_info();
        let known_bad_reason = Self::matches_known_bad(&info, known_bad);
        Self {
            name: info.name,
            vendor: info.vendor,
            device: info.device,
            device_type: info.device_type,
            backend: info.backend,
            features: adapter.features(),
            limits: adapter.limits(),
            known_bad: known_bad_reason,
        }
    }

    /// Pure match on adapter metadata: vendor equality and case-sensitive name substring.
    ///
    /// The first matching entry wins; an empty list returns `None`.
    #[must_use]
    pub fn matches_known_bad(info: &wgpu::AdapterInfo, list: &[KnownBadGpu]) -> Option<String> {
        list.iter()
            .find(|entry| entry.vendor == info.vendor && info.name.contains(&entry.name_contains))
            .map(|entry| entry.reason.clone())
    }
}

/// Device capability snapshot at start-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceProfile {
    /// Compile-time device class (or [`DeviceClass::Server`] for headless servers).
    pub class: DeviceClass,
    /// Whether the platform should present touch controls.
    pub touch: bool,
    /// Logical and estimated performance core counts.
    pub cpu: CpuProfile,
    /// Total physical RAM in bytes; `None` when the platform does not report it (web).
    pub memory_bytes: Option<u64>,
    /// `None` for servers and runs that never create a GPU.
    pub gpu: Option<GpuProfile>,
}

impl DeviceProfile {
    /// Probes CPU and memory on this machine and attaches `gpu` when provided.
    pub fn detect(
        class: DeviceClass,
        gpu: Option<GpuProfile>,
        touch: bool,
    ) -> Result<Self, PlatformError> {
        let logical = std::thread::available_parallelism()
            .map_err(PlatformError::CpuCount)?
            .get();
        let logical_cores = NonZeroUsize::new(logical).ok_or_else(|| {
            PlatformError::CpuCount(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "reported zero logical CPUs",
            ))
        })?;
        let (frequencies_mhz, memory_bytes) = probe_host_memory_and_frequencies()?;
        let performance_cores = estimate_performance_cores(&frequencies_mhz, logical_cores);
        Ok(Self {
            class,
            touch,
            cpu: CpuProfile {
                logical_cores,
                performance_cores,
            },
            memory_bytes,
            gpu,
        })
    }

    /// Server profile: [`DeviceClass::Server`] with no GPU.
    pub fn server() -> Result<Self, PlatformError> {
        Self::detect(DeviceClass::Server, None, false)
    }

    /// Job-pool worker count for this profile.
    #[must_use]
    pub fn job_workers(&self) -> usize {
        worker_count(self.class, &self.cpu)
    }
}

/// Job-pool size from device class and CPU layout; pure.
#[must_use]
pub fn worker_count(class: DeviceClass, cpu: &CpuProfile) -> usize {
    match class {
        DeviceClass::Desktop | DeviceClass::Server => {
            cpu.logical_cores.get().saturating_sub(2).max(1)
        }
        DeviceClass::Handheld => cpu.performance_cores.get().saturating_sub(1).max(1),
        DeviceClass::Web => 0,
    }
}

/// Maps sysinfo total RAM to `Option`; zero means unknown or unsupported.
#[must_use]
pub fn memory_from_total(total: u64) -> Option<u64> {
    (total != 0).then_some(total)
}

#[cfg(not(target_arch = "wasm32"))]
fn probe_host_memory_and_frequencies() -> Result<(Vec<u64>, Option<u64>), PlatformError> {
    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

    let refresh = RefreshKind::nothing()
        .with_memory(MemoryRefreshKind::nothing().with_ram())
        .with_cpu(CpuRefreshKind::nothing().with_frequency());
    let system = System::new_with_specifics(refresh).map_err(map_sysinfo_error)?;
    let frequencies_mhz = system.cpus().iter().map(sysinfo::Cpu::frequency).collect();
    let memory_bytes = memory_from_total(system.total_memory());
    Ok((frequencies_mhz, memory_bytes))
}

#[cfg(not(target_arch = "wasm32"))]
fn map_sysinfo_error(err: sysinfo::Error) -> PlatformError {
    match err {
        sysinfo::Error::Io(io) => PlatformError::Io(io),
        sysinfo::Error::Unsupported | sysinfo::Error::Other(_) => {
            PlatformError::SystemInfo(err.to_string())
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn probe_host_memory_and_frequencies() -> Result<(Vec<u64>, Option<u64>), PlatformError> {
    Ok((Vec::new(), None))
}
