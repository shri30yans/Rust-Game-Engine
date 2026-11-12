//! Typed errors for the platform layer. Setup (adapter, device, window, surface, event loop)
//! and the headless readback return these instead of panicking, so the caller decides what
//! a missing GPU or a failed screenshot means.

use thiserror::Error;

/// Everything that can go wrong while bringing up or driving the platform layer.
#[derive(Debug, Error)]
pub enum PlatformError {
    /// No GPU adapter satisfies the request (headless, or compatible with the surface).
    #[error("no suitable GPU adapter: {0}")]
    NoAdapter(#[from] wgpu::RequestAdapterError),
    /// The adapter refused to create a logical device.
    #[error("request_device failed: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    /// The OS refused to create the window.
    #[error("could not create window: {0}")]
    Window(#[from] winit::error::OsError),
    /// The windowing event loop could not be created or run.
    #[error("event loop failed: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    /// wgpu could not create a surface for the window.
    #[error("could not create surface: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    /// The surface reports no configuration this adapter can drive.
    #[error("surface supports no configuration on this adapter")]
    SurfaceUnsupported,
    /// The surface reports no texture format at all.
    #[error("surface reports no texture formats")]
    NoSurfaceFormat,
    /// A `width x height` image is too large for the 256-aligned readback buffer.
    #[error("image of {0}x{1} exceeds the readback limits")]
    ImageTooLarge(u32, u32),
    /// The readback channel closed before the buffer was mapped.
    #[error("readback buffer was dropped before it was mapped")]
    ReadbackLost,
    /// Mapping the readback buffer failed.
    #[error("readback buffer mapping failed: {0}")]
    Map(#[from] wgpu::BufferAsyncError),
    /// Reading the mapped range of the readback buffer failed.
    #[error("reading the mapped readback buffer failed: {0}")]
    MappedRange(#[from] wgpu::MapRangeError),
    /// The readback buffer holds fewer bytes than its layout requires.
    #[error("readback buffer is shorter than the image layout")]
    ReadbackShort,
    /// PNG encoding failed.
    #[error("png encoding failed: {0}")]
    Png(#[from] png::EncodingError),
    /// Creating the output file failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// `available_parallelism` failed or reported zero CPUs.
    #[error("cannot determine the CPU count: {0}")]
    CpuCount(#[source] std::io::Error),
    /// CPU or memory probing through sysinfo failed (unsupported platform or internal error).
    #[error("system information unavailable: {0}")]
    SystemInfo(String),
    /// An app id or config filename does not meet its portable path rules.
    #[error("invalid config {kind}: {value}")]
    InvalidConfigName {
        /// Which validated value was rejected.
        kind: &'static str,
        /// The rejected value.
        value: String,
    },
    /// No platform config directory could be determined.
    #[error("could not determine the platform config directory")]
    ConfigDirectoryUnavailable,
}
