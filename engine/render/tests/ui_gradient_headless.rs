//! Headless GPU check that a linear gradient reaches the pixels: a 200x50 quad drawn through the
//! UI pass with a black-to-white gradient runs left to right, so the left, middle and right
//! columns read back near black, mid grey and white. The gradient runs at 90 degrees because the
//! shader's 0 degree axis points up (CSS convention), so a 0 degree gradient would vary by row,
//! not by column. The test fails (it does not skip) when the machine has no GPU adapter.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use platform::Gpu;
use render::{Camera, Gfx, Post, RenderTier, Settings};

const W: u32 = 200;
const H: u32 = 50;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Allowed error per channel, in 8-bit steps.
const TOLERANCE: i32 = 4;
/// Copy rows must be padded to 256 bytes; 200 pixels of RGBA are 800 bytes.
const ROW_STRIDE: u32 = 1024;

fn gpu() -> Gpu {
    Gpu::new(wgpu::Instance::default(), None).expect("headless gradient test needs a GPU adapter")
}

/// High tier with bloom and TAA off, so the UI pass is the only thing that writes pixels.
fn settings() -> Settings {
    let mut s = RenderTier::High.settings();
    s.bloom = false;
    s.taa = false;
    s
}

/// Copies the target to the CPU and returns its tightly packed RGBA bytes.
fn readback(gpu: &Gpu, tex: &wgpu::Texture) -> Vec<u8> {
    let len = u64::from(ROW_STRIDE) * u64::from(H);
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gradient-readback"),
        size: len,
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
                bytes_per_row: Some(ROW_STRIDE),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([enc.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).expect("readback receiver is alive");
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    rx.recv()
        .expect("map callback ran")
        .expect("readback buffer mapped");
    let padded = buf.slice(..).get_mapped_range().expect("mapped range");
    let row_bytes = (W * 4) as usize;
    padded
        .chunks(ROW_STRIDE as usize)
        .take(H as usize)
        .flat_map(|row| row.get(..row_bytes).unwrap_or_default().iter().copied())
        .collect()
}

/// RGB of pixel `(x, y)` in a readback frame.
fn rgb(frame: &[u8], x: u32, y: u32) -> [u8; 3] {
    let start = ((y * W + x) * 4) as usize;
    frame
        .get(start..start + 3)
        .expect("pixel is inside the frame")
        .try_into()
        .expect("three colour bytes")
}

/// Grey level at the centre of column `x` for a 0 to 1 gradient across the width, rounded to
/// the nearest 8-bit step. Integer maths: (x + 0.5) / W * 255 is (2x + 1) * 255 / (2W).
fn expected_grey(x: u32) -> u8 {
    let level = ((2 * x + 1) * 255 + W) / (2 * W);
    u8::try_from(level).expect("a 0 to 1 gradient stays within 0..=255")
}

fn assert_near_grey(label: &str, got: [u8; 3], want: u8) {
    for (channel, value) in got.iter().enumerate() {
        let diff = i32::from(*value) - i32::from(want);
        assert!(
            diff.abs() <= TOLERANCE,
            "{label}: channel {channel} is {value}, want {want} +/- {TOLERANCE} (pixel {got:?})"
        );
    }
}

#[test]
fn a_ninety_degree_gradient_runs_black_to_white_across_the_quad() {
    let gpu = gpu();
    let mut g = Gfx::new(&gpu, FORMAT, (W, H), settings()).expect("renderer builds");
    g.post = Post {
        bloom: 0.0,
        vignette: 0.0,
        grain: 0.0,
        saturation: 1.0,
        contrast: 1.0,
        shadow_tint: [0.0; 4],
        highlight_tint: [0.0; 4],
        ..Post::default()
    };
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gradient-target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());

    g.begin_frame(Camera::default());
    g.ui.set_linear_gradient_uniform(
        90.0,
        &[(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [1.0, 1.0, 1.0, 1.0])],
    );
    g.ui.linear_gradient(0.0, 0.0, W as f32, H as f32, 0.0, [1.0, 1.0, 1.0, 1.0]);
    g.render(&view, 1.0 / 60.0);
    let frame = readback(&gpu, &target);

    let row = H / 2;
    // Sample inside the quad: its outermost columns blend with the clear colour at the edge.
    for (label, x) in [("left", 2), ("middle", W / 2), ("right", W - 3)] {
        assert_near_grey(label, rgb(&frame, x, row), expected_grey(x));
    }
}
