//! Device handles shared by the renderer: instance, adapter, device and queue.
use crate::PlatformError;
use crate::profile::GpuProfile;
use crate::profile::KnownBadGpu;

/// Device handles shared by the renderer. Cheap to clone (wgpu handles are Arcs).
#[derive(Clone)]
pub struct Gpu {
    /// The wgpu instance the adapter was requested from.
    pub instance: wgpu::Instance,
    /// The selected adapter.
    pub adapter: wgpu::Adapter,
    /// Logical device created on `adapter`.
    pub device: wgpu::Device,
    /// Submission queue of `device`.
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// `surface` is None for headless. Limits are downlevel-friendly on purpose so
    /// anything that works here has a chance on low-end mobile GPUs.
    pub fn new(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Gpu, PlatformError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))?;
        let optional_features = wgpu::Features::TIMESTAMP_QUERY
            | wgpu::Features::TEXTURE_COMPRESSION_BC
            | wgpu::Features::TEXTURE_COMPRESSION_ASTC
            | wgpu::Features::TEXTURE_COMPRESSION_ETC2;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("engine-device"),
                required_features: adapter.features() & optional_features,
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            }))?;
        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// Human-readable adapter name and backend, for logs.
    pub fn info(&self) -> String {
        let i = self.adapter.get_info();
        format!("{} ({:?})", i.name, i.backend)
    }

    /// Adapter metadata for the device profile.
    pub fn profile(&self, known_bad: &[KnownBadGpu]) -> GpuProfile {
        GpuProfile::from_adapter(&self.adapter, known_bad)
    }
}
