//! Headless runner: renders to an offscreen texture. Used for `--screenshot` and
//! `--bench` so results can be checked without a window or display.
use super::{Gpu, Input, PlatformApp, PlatformError};
use std::path::Path;
use std::time::Instant;

/// Colour format of the offscreen target (sRGB, so PNG output needs no conversion).
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Timing results of a headless run.
pub struct Timings {
    /// Wall ms per frame, including waiting for the GPU to finish (no vsync).
    pub frame_ms: Vec<f32>,
}

/// Runs `frames` frames at a fixed simulated dt (1/60 s) so output is deterministic.
/// If `screenshot` is set, the last frame is read back and written as PNG.
pub fn run<A: PlatformApp>(
    app: &mut A,
    size: (u32, u32),
    frames: u32,
    screenshot: Option<&Path>,
) -> Result<Timings, PlatformError> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("gpu: {}", gpu.info());
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    app.init(&gpu, FORMAT, size);
    let mut input = Input::default();
    input.screen = glam::Vec2::new(size.0 as f32, size.1 as f32);
    let mut frame_ms = Vec::with_capacity(frames as usize);
    for _ in 0..frames {
        let t = Instant::now();
        app.frame(&view, &mut input, 1.0 / 60.0);
        input.end_frame();
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        frame_ms.push(t.elapsed().as_secs_f32() * 1000.0);
    }
    if let Some(path) = screenshot {
        write_png(&gpu, &tex, size, path)?;
    }
    Ok(Timings { frame_ms })
}

/// Copies `tex` to a mapped buffer and writes it as an 8-bit RGBA PNG at `path`.
fn write_png(
    gpu: &Gpu,
    tex: &wgpu::Texture,
    size: (u32, u32),
    path: &Path,
) -> Result<(), PlatformError> {
    let (w, h) = size;
    // Row pitch must be 256-aligned; compute in u64 so the multiply cannot wrap.
    let bpr = u32::try_from((u64::from(w) * 4).div_ceil(256) * 256)
        .map_err(|_| PlatformError::ImageTooLarge(w, h))?;
    let buf_len = u64::from(bpr) * u64::from(h);
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: buf_len,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bpr),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([enc.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        // The receiver is alive until the recv below, so a send failure means nobody is waiting.
        let _ = tx.send(r);
    });
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv().map_err(|_| PlatformError::ReadbackLost)??;
    let data = buf.slice(..).get_mapped_range()?;
    let row_len = (w as usize) * 4;
    let mut rgba = Vec::with_capacity(row_len * h as usize);
    for y in 0..h as usize {
        let s = y * bpr as usize;
        let row = data
            .get(s..s + row_len)
            .ok_or(PlatformError::ReadbackShort)?;
        rgba.extend_from_slice(row);
    }
    drop(data);
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    println!("screenshot: {}", path.display());
    Ok(())
}
