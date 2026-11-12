//! Headless proof that opaque shading covers every depth pixel with and without the prepass.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use engine_assets::format::{Bounds, IndexEncoding, MeshLod, PositionEncoding};
use glam::{Affine3A, Vec3};
use platform::Gpu;
use render::{Camera, Gfx, Layer, Look, Mat, MeshBuilder, PrimitiveDesc, RenderTier, ViewMode};

const SIZE: u32 = 128;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const TIERS: [RenderTier; 5] = [
    RenderTier::Test,
    RenderTier::Low,
    RenderTier::Medium,
    RenderTier::High,
    RenderTier::Ultra,
];

#[test]
fn opaque_shading_covers_depth_for_every_tier_and_prepass_state() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("coverage test needs a GPU adapter");
    #[cfg(target_os = "macos")]
    assert_eq!(gpu.adapter.get_info().backend, wgpu::Backend::Metal);

    for tier in TIERS {
        for prepass in [false, true] {
            let pixels = render_fixture(&gpu, tier, prepass);
            let (covered, missing_marker) = coverage_counts(&pixels);
            assert!(
                covered > 0,
                "{tier:?}, prepass={prepass}: fixture must cover pixels"
            );
            assert_eq!(
                missing_marker, 0,
                "{tier:?}, prepass={prepass}: depth-covered pixels without opaque marker"
            );
        }
    }
}

fn render_fixture(gpu: &Gpu, tier: RenderTier, prepass: bool) -> Vec<u8> {
    let mut settings = tier.settings();
    settings.shadow_size = 128;
    settings.bloom = false;
    settings.view_mode = ViewMode::OpaqueCoverage { prepass };
    let mut gfx = Gfx::new(gpu, FORMAT, (SIZE, SIZE), settings).expect("renderer builds");
    let box_mesh = gfx
        .add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0)))
        .expect("box mesh uploads");
    for x in [-0.25, 0.25] {
        gfx.scene_mut()
            .add(
                box_mesh,
                PrimitiveDesc {
                    transform: Affine3A::from_translation(Vec3::new(x, 1.7, -6.0)),
                    material: render::Material::Procedural(Look::default()),
                    layer: Layer::World,
                    casts_shadow: false,
                },
            )
            .expect("coplanar box added");
    }
    let quantized = gfx
        .add_cooked_mesh(&[quantized_triangle()])
        .expect("quantized cooked mesh uploads");
    gfx.scene_mut()
        .add(
            quantized,
            PrimitiveDesc {
                transform: Affine3A::from_translation(Vec3::new(-1.2, 1.2, -4.5)),
                material: render::Material::Procedural(Look::default()),
                layer: Layer::World,
                casts_shadow: false,
            },
        )
        .expect("quantized mesh added");

    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("opaque-coverage-target"),
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
    let view = target.create_view(&Default::default());
    gfx.begin_frame(Camera::default());
    gfx.render(&view, 1.0 / 60.0);
    readback(gpu, &target)
}

fn quantized_triangle() -> MeshLod {
    let positions = [
        [6_553u16, 6_553u16, 32_767u16, 0u16],
        [58_982, 6_553, 32_767, 0],
        [32_767, 58_982, 32_767, 0],
    ]
    .into_iter()
    .flat_map(|vertex| vertex.into_iter().flat_map(u16::to_le_bytes))
    .collect();
    let attributes = (0..3).flat_map(|_| [0u8; 12]).collect();
    MeshLod {
        positions,
        attributes,
        indices: [0u16, 1, 2]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect(),
        index_encoding: IndexEncoding::U16,
        position_encoding: PositionEncoding::Quantized16,
        bounds: Bounds {
            min: [-1.0, -1.0, -1.0],
            max: [1.0, 1.0, 1.0],
            center: [0.0; 3],
            radius: 2.0,
        },
        screen_size_threshold: 0.0,
        screen_error: 0.0,
        vertex_count: 3,
        index_count: 3,
    }
}

fn readback(gpu: &Gpu, texture: &wgpu::Texture) -> Vec<u8> {
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("opaque-coverage-readback"),
        size: u64::from(SIZE) * u64::from(SIZE) * 4,
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

fn coverage_counts(pixels: &[u8]) -> (usize, usize) {
    let (pixels, remainder) = pixels.as_chunks::<4>();
    assert!(remainder.is_empty(), "readback has whole RGBA pixels");
    let mut covered = 0usize;
    let mut missing_marker = 0usize;
    for &[marker, depth_covered, _, _] in pixels {
        if depth_covered > 127 {
            covered = covered.saturating_add(1);
            if marker != u8::MAX {
                missing_marker = missing_marker.saturating_add(1);
            }
        }
    }
    (covered, missing_marker)
}
