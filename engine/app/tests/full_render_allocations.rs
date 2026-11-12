//! Ignored profiling fixture for allocations made by a steady, textured Gfx render.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::alloc::System;
use std::hint::black_box;
use std::thread;
use std::time::Duration;

use engine_assets::{ColourSpace, TextureData, prepare_texture};
use glam::Affine3A;
use platform::Gpu;
use render::{
    Camera, Gfx, Layer, Mat, Material, MaterialDesc, MeshBuilder, PrimitiveDesc, RenderTier,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const SIZE: u32 = 128;
const PRIMITIVE_COUNT: usize = 500;
const MEASURED_FRAMES: usize = 120;

#[test]
#[ignore = "52,396 branch allocations over 120 frames; c37781b measured 52,499, within observed run variation; keep for the follow-up allocation profile"]
fn full_render_allocations_for_textured_scene() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless GPU");
    let mut settings = RenderTier::High.settings();
    settings.taa = false;
    let mut gfx = Gfx::new(
        &gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        (SIZE, SIZE),
        settings,
    )
    .expect("renderer");
    let mesh = gfx
        .add_mesh(&MeshBuilder::unit_cube(Mat::new([1.0; 3], 0.8, 0.0)))
        .expect("mesh uploads");
    let source = TextureData {
        ktx2: include_bytes!("../../assets/tests/fixtures/uastc.ktx2").to_vec(),
    };
    let texture = gfx
        .add_texture(
            prepare_texture(&source, ColourSpace::Srgb, gfx.texture_family())
                .expect("fixture texture prepares"),
        )
        .expect("texture uploads");
    let material = gfx
        .add_material(MaterialDesc {
            base_color_texture: Some(texture),
            ..MaterialDesc::default()
        })
        .expect("material uploads");
    for _ in 0..PRIMITIVE_COUNT {
        gfx.scene_mut()
            .add(
                mesh,
                PrimitiveDesc {
                    transform: Affine3A::IDENTITY,
                    material: Material::Textured(material),
                    layer: Layer::World,
                    casts_shadow: true,
                },
            )
            .expect("primitive enters retained scene");
    }
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("full-render-allocation-target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    for _ in 0..16 {
        gfx.begin_frame(Camera::default());
        black_box(gfx.render(&view, 1.0 / 60.0));
    }

    // The quiet interval brackets the measured region in Instruments' time-based allocation trace.
    thread::sleep(Duration::from_secs(2));
    let region = Region::new(GLOBAL);
    for _ in 0..MEASURED_FRAMES {
        gfx.begin_frame(Camera::default());
        black_box(gfx.render(&view, 1.0 / 60.0));
    }
    let stats = region.change();
    println!("full Gfx::render allocation stats over {MEASURED_FRAMES} frames: {stats:?}");
    thread::sleep(Duration::from_secs(2));
}
