//! Integration tests for the device profile.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use platform::PlatformError;
use platform::gpu::Gpu;
use platform::profile::{
    CpuProfile, DeviceClass, DeviceProfile, GpuProfile, KnownBadGpu, estimate_performance_cores,
    memory_from_total, worker_count,
};
use proptest::prelude::*;
use std::num::NonZeroUsize;

fn cpu(logical: usize, performance: usize) -> CpuProfile {
    CpuProfile {
        logical_cores: NonZeroUsize::new(logical).expect("logical"),
        performance_cores: NonZeroUsize::new(performance).expect("performance"),
    }
}

#[test]
fn worker_count_desktop_and_server() {
    let cases = [(1, 1), (2, 1), (3, 1), (8, 6)];
    for (logical, want) in cases {
        let c = cpu(logical, logical);
        assert_eq!(worker_count(DeviceClass::Desktop, &c), want);
        assert_eq!(worker_count(DeviceClass::Server, &c), want);
    }
}

#[test]
fn worker_count_handheld() {
    let cases = [(1, 1), (2, 1), (4, 3)];
    for (performance, want) in cases {
        let c = cpu(8, performance);
        assert_eq!(worker_count(DeviceClass::Handheld, &c), want);
    }
}

#[test]
fn worker_count_web_is_zero() {
    let c = cpu(16, 8);
    assert_eq!(worker_count(DeviceClass::Web, &c), 0);
}

proptest! {
    #[test]
    fn worker_count_bounds(class in prop_oneof![
        Just(DeviceClass::Desktop),
        Just(DeviceClass::Server),
        Just(DeviceClass::Handheld),
        Just(DeviceClass::Web),
    ], logical in 1usize..=64, performance in 1usize..=64) {
        let performance = performance.min(logical);
        let c = cpu(logical, performance);
        let count = worker_count(class, &c);
        if class == DeviceClass::Web {
            prop_assert_eq!(count, 0);
        } else {
            prop_assert!(count >= 1);
            prop_assert!(count <= logical);
        }
    }

    #[test]
    fn worker_count_desktop_monotonic_in_logical(logical in 2usize..=64) {
        let mut prev = worker_count(DeviceClass::Desktop, &cpu(1, 1));
        for n in 2..=logical {
            let count = worker_count(DeviceClass::Desktop, &cpu(n, n));
            prop_assert!(count >= prev, "logical {n}: {count} < {prev}");
            prev = count;
        }
    }

    #[test]
    fn estimate_performance_cores_clamped(frequencies in prop::collection::vec(any::<u64>(), 0..32), logical in 1usize..=64) {
        let logical = NonZeroUsize::new(logical).unwrap();
        let perf = estimate_performance_cores(&frequencies, logical);
        prop_assert!(perf.get() >= 1);
        prop_assert!(perf.get() <= logical.get());
    }
}

#[test]
fn estimate_performance_cores_homogeneous() {
    let logical = NonZeroUsize::new(6).unwrap();
    for frequencies in [&[][..], &[0_u64; 6], &[3000, 3000, 3000, 3000, 3000, 3000]] {
        assert_eq!(estimate_performance_cores(frequencies, logical), logical);
    }
}

#[test]
fn estimate_performance_cores_mixed_cluster() {
    let logical = NonZeroUsize::new(6).unwrap();
    let frequencies = [3000, 3000, 2000, 2000, 2000, 2000];
    assert_eq!(estimate_performance_cores(&frequencies, logical).get(), 2,);
}

#[test]
fn estimate_performance_cores_seventy_five_percent_boundary() {
    let logical = NonZeroUsize::new(2).unwrap();
    assert_eq!(estimate_performance_cores(&[1000, 750], logical).get(), 2,);
    assert_eq!(estimate_performance_cores(&[1000, 749], logical).get(), 1,);
}

#[test]
fn estimate_performance_cores_u64_max_stays_in_range() {
    let logical = NonZeroUsize::new(8).unwrap();
    let frequencies = [u64::MAX, u64::MAX, u64::MAX / 2, 1];
    let perf = estimate_performance_cores(&frequencies, logical);
    assert!(perf.get() >= 1);
    assert!(perf.get() <= logical.get());
}

fn adapter_info(name: &str, vendor: u32) -> wgpu::AdapterInfo {
    wgpu::AdapterInfo {
        name: name.to_owned(),
        vendor,
        device: 1,
        device_type: wgpu::DeviceType::DiscreteGpu,
        device_pci_bus_id: String::new(),
        driver: String::new(),
        driver_info: String::new(),
        backend: wgpu::Backend::Metal,
        subgroup_min_size: 1,
        subgroup_max_size: 1,
        transient_saves_memory: None,
        limit_bucket: None,
    }
}

#[test]
fn matches_known_bad_rules() {
    let list = [
        KnownBadGpu {
            vendor: 0x10de,
            name_contains: "GTX 750".to_owned(),
            reason: "too old".to_owned(),
        },
        KnownBadGpu {
            vendor: 0x10de,
            name_contains: "GTX 760".to_owned(),
            reason: "also old".to_owned(),
        },
    ];
    let info = adapter_info("NVIDIA GeForce GTX 750 Ti", 0x10de);
    assert_eq!(
        GpuProfile::matches_known_bad(&info, &list),
        Some("too old".to_owned()),
    );
    let other_vendor = adapter_info("NVIDIA GeForce GTX 750 Ti", 0x1002);
    assert_eq!(GpuProfile::matches_known_bad(&other_vendor, &list), None);
    let case_sensitive = adapter_info("nvidia geforce gtx 750", 0x10de);
    assert_eq!(GpuProfile::matches_known_bad(&case_sensitive, &list), None);
    assert_eq!(GpuProfile::matches_known_bad(&info, &[]), None);
}

#[test]
fn memory_from_total_maps_zero() {
    assert_eq!(memory_from_total(0), None);
    assert_eq!(memory_from_total(42), Some(42));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn detect_on_host() -> Result<(), PlatformError> {
    let profile = DeviceProfile::detect(DeviceClass::for_target(), None, false)?;
    assert!(profile.cpu.logical_cores.get() >= 1);
    assert!(profile.cpu.performance_cores.get() <= profile.cpu.logical_cores.get());
    let memory = profile.memory_bytes.expect("host should report RAM");
    assert!(memory > 0);
    assert_eq!(
        profile.job_workers(),
        worker_count(profile.class, &profile.cpu),
    );
    Ok(())
}

#[test]
fn server_profile() -> Result<(), PlatformError> {
    let profile = DeviceProfile::server()?;
    assert_eq!(profile.class, DeviceClass::Server);
    assert!(profile.gpu.is_none());
    assert_eq!(
        profile.job_workers(),
        worker_count(DeviceClass::Server, &profile.cpu),
    );
    Ok(())
}

#[test]
fn gpu_profile_from_headless_adapter() {
    let instance = wgpu::Instance::default();
    let Ok(gpu) = Gpu::new(instance, None) else {
        println!("skipped: no GPU adapter");
        return;
    };
    let profile = gpu.profile(&[]);
    assert!(!profile.name.is_empty());
    assert!(profile.limits.max_texture_dimension_2d > 0);
}
