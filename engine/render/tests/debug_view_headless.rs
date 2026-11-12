//! Headless GPU checks that every checked terminal renderer view encodes pixels and that UI is
//! drawn after an isolated view. These tests fail rather than skip when the machine has no GPU.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use glam::{Affine3A, Vec3};
use platform::Gpu;
use render::{
    Camera, Gfx, Layer, Look, Mat, MeshBuilder, PrimitiveDesc, RenderTier, Settings, ViewMode,
};

const SIZE: u32 = 128;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn gpu() -> Gpu {
    Gpu::new(wgpu::Instance::default(), None).expect("debug-view test needs a GPU adapter")
}

fn settings(view_mode: ViewMode, tier: RenderTier) -> Settings {
    let mut settings = tier.settings();
    settings.bloom = false;
    settings.view_mode = view_mode;
    settings
}

fn target(gpu: &Gpu) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("debug-view-target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

fn readback(gpu: &Gpu, texture: &wgpu::Texture) -> Vec<u8> {
    let size = u64::from(SIZE) * u64::from(SIZE) * 4;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("debug-view-readback"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).expect("readback receiver is alive");
        });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    receiver
        .recv()
        .expect("map callback ran")
        .expect("readback buffer mapped");
    buffer
        .slice(..)
        .get_mapped_range()
        .expect("mapped range")
        .to_vec()
}

fn render_mode(gpu: &Gpu, view_mode: ViewMode, tier: RenderTier, ui: bool) -> Vec<u8> {
    let mut gfx =
        Gfx::new(gpu, FORMAT, (SIZE, SIZE), settings(view_mode, tier)).expect("renderer builds");
    let mesh = gfx
        .add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xc66a36, 0.65, 0.0)))
        .expect("mesh uploads");
    gfx.scene_mut()
        .add(
            mesh,
            PrimitiveDesc {
                transform: Affine3A::from_translation(Vec3::new(0.0, 1.7, -6.0)),
                material: render::Material::Procedural(Look::default()),
                layer: Layer::World,
                casts_shadow: true,
            },
        )
        .expect("primitive added");
    let (texture, view) = target(gpu);
    gfx.begin_frame(Camera::default());
    if ui {
        gfx.ui.rect(0.0, 0.0, 20.0, 20.0, 0.0, [1.0, 0.0, 0.0, 1.0]);
    }
    gfx.render(&view, 1.0 / 60.0);
    readback(gpu, &texture)
}

#[test]
fn every_view_mode_encodes_a_terminal_frame() {
    let gpu = gpu();
    #[cfg(target_os = "macos")]
    assert_eq!(gpu.adapter.get_info().backend, wgpu::Backend::Metal);
    for tier in [RenderTier::Low, RenderTier::High] {
        for view_mode in [
            ViewMode::Lit,
            ViewMode::BaseColour,
            ViewMode::LightingOnly,
            ViewMode::AmbientOcclusion,
            ViewMode::Depth,
            ViewMode::Shadow,
        ] {
            let frame = render_mode(&gpu, view_mode, tier, false);
            assert!(
                frame.as_chunks::<4>().0.iter().any(|pixel| {
                    let [red, green, blue, _] = *pixel;
                    [red, green, blue] != [0, 0, 0]
                }),
                "{tier:?} {view_mode:?} must encode a visible terminal frame"
            );
        }
    }
}

#[test]
fn ui_draws_after_the_selected_terminal_view() {
    let gpu = gpu();
    let frame = render_mode(&gpu, ViewMode::Depth, RenderTier::Low, true);
    let pixel: [u8; 4] = frame
        .get(..4)
        .and_then(|bytes| bytes.try_into().ok())
        .expect("top-left output pixel");
    let [red, green, blue, _] = pixel;
    assert!(
        red > 200 && green < 30 && blue < 30,
        "UI must overlay the depth terminal pass, got {pixel:?}"
    );
}
