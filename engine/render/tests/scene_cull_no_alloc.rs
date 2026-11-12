//! A moving, warmed retained scene and its camera/cascade cull do not allocate.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::alloc::System;
use std::hint::black_box;

use glam::{Affine3A, Mat4, Vec3, Vec4};
use platform::Gpu;
use render::{Gfx, Layer, Look, Mat, MeshBuilder, MeshId, PrimitiveDesc, RenderScene, RenderTier};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const PRIMITIVES: usize = 1_000;
const FRAMES: usize = 1_000;

fn planes(matrix: &Mat4) -> [Vec4; 6] {
    let rows = [matrix.row(0), matrix.row(1), matrix.row(2), matrix.row(3)];
    let mut planes = [
        rows[3] + rows[0],
        rows[3] - rows[0],
        rows[3] + rows[1],
        rows[3] - rows[1],
        rows[2],
        rows[3] - rows[2],
    ];
    for plane in &mut planes {
        *plane /= plane.truncate().length();
    }
    planes
}

fn cull(
    scene: &RenderScene,
    camera_planes: &[Vec4; 6],
    cascades: &[[Vec4; 6]; 2],
    main: &mut Vec<(MeshId, u32)>,
    shadow: &mut [Vec<(MeshId, u32)>; 2],
) {
    main.clear();
    for indices in shadow.iter_mut() {
        indices.clear();
    }
    black_box(scene.cull(
        camera_planes,
        cascades,
        |mesh, _material, slot, layer, main_visible, _, shadows, _, _| {
            if layer != Layer::World {
                return;
            }
            if main_visible {
                main.push((mesh, slot));
            }
            for (cascade, visible) in shadows.into_iter().enumerate() {
                if visible && let Some(indices) = shadow.get_mut(cascade) {
                    indices.push((mesh, slot));
                }
            }
        },
    ));
}

#[test]
fn moving_scene_culling_allocates_nothing_after_warmup() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless GPU");
    let mut settings = RenderTier::High.settings();
    settings.taa = false;
    let mut gfx =
        Gfx::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, (128, 128), settings).expect("renderer");
    let mesh = gfx
        .add_mesh(&MeshBuilder::unit_cube(Mat::new([1.0; 3], 0.8, 0.0)))
        .expect("mesh uploads");
    let scene = gfx.scene_mut();
    let mut primitives = Vec::with_capacity(PRIMITIVES);
    for index in 0..PRIMITIVES {
        let x = (index % 40) as f32 - 20.0;
        let z = (index / 40) as f32 * -2.0;
        primitives.push(
            scene
                .add(
                    mesh,
                    PrimitiveDesc {
                        transform: Affine3A::from_translation(Vec3::new(x, 0.0, z)),
                        material: render::Material::Procedural(Look::default()),
                        layer: Layer::World,
                        casts_shadow: true,
                    },
                )
                .expect("primitive"),
        );
    }
    let camera_planes = planes(&Mat4::orthographic_rh(-40.0, 40.0, -30.0, 30.0, 0.1, 120.0));
    let cascades = [
        planes(&Mat4::orthographic_rh(-15.0, 15.0, -15.0, 15.0, 0.1, 40.0)),
        planes(&Mat4::orthographic_rh(-50.0, 50.0, -50.0, 50.0, 0.1, 150.0)),
    ];
    let mut main = Vec::with_capacity(PRIMITIVES);
    let mut shadow = [
        Vec::with_capacity(PRIMITIVES),
        Vec::with_capacity(PRIMITIVES),
    ];
    for frame in 0..16 {
        let x = if frame % 2 == 0 { 0.0 } else { 0.01 };
        scene
            .set_transform(
                *primitives.first().expect("moving primitive"),
                Affine3A::from_translation(Vec3::new(x, 0.0, -1.0)),
            )
            .expect("move");
        cull(scene, &camera_planes, &cascades, &mut main, &mut shadow);
    }

    let region = Region::new(GLOBAL);
    for frame in 0..FRAMES {
        let x = if frame % 2 == 0 { 0.0 } else { 0.01 };
        scene
            .set_transform(
                *primitives.first().expect("moving primitive"),
                Affine3A::from_translation(Vec3::new(x, 0.0, -1.0)),
            )
            .expect("move");
        cull(scene, &camera_planes, &cascades, &mut main, &mut shadow);
    }
    let stats = region.change();
    assert_eq!(stats.allocations, 0, "cull allocated: {stats:?}");
    assert_eq!(stats.reallocations, 0, "cull reallocated: {stats:?}");
}
