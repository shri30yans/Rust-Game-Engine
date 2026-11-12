//! Fixed-step, extract, retained-scene, cull and frame-exchange paths stay allocation-free after warm-up.
//!
//! One `#[test]` only: the allocator counter is process-wide, so parallel tests would pollute it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::{AssetUploader, Extractor, FixedClock, RenderFrame, TickRate, frame_exchange};
use engine_assets::{
    AssetPath, AssetStore, ColourSpace, ContentPack, PackReader, TextureData, prepare_texture,
};
use engine_core::Tick;
use engine_scene::{Look, MaterialKind, MeshKind, Visual, Watched, register_components};
use engine_world::{Transform, World};
use glam::{Affine3A, Vec4};
use jobs::JobPool;
use platform::Gpu;
use render::{Gfx, Layer, Mat, MaterialDesc, MeshBuilder, PrimitiveDesc, RenderScene, RenderTier};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;
use std::hint::black_box;
use std::io::Cursor;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const FRAMES: u64 = 1000;
const ENTITIES: usize = 1_000;

#[test]
fn hot_paths_allocate_nothing_after_warm_up() {
    let rate = TickRate::new(60).expect("valid rate");
    let mut clock = FixedClock::new(rate, 8).expect("clock");
    for frame in 0..16u64 {
        black_box(clock.advance(black_box(frame * 20_000_000)));
    }

    let region = Region::new(GLOBAL);
    for frame in 0..FRAMES {
        black_box(clock.advance(black_box(frame * 20_000_000)));
    }
    let stats = region.change();

    assert_eq!(stats.allocations, 0, "advance allocated: {stats:?}");
    assert_eq!(stats.reallocations, 0, "advance reallocated: {stats:?}");

    // Pack hash checks stay allocation-free on unchanged entries.
    let mut pack_bytes = Vec::new();
    let path = AssetPath::new("game/levels/idle.level.ron").expect("level path");
    ContentPack::write(
        [(
            path.clone(),
            b"(version: 1, name: \"idle\", gravity: (0.0, -9.81, 0.0), shapes: [])".to_vec(),
        )],
        &mut pack_bytes,
    )
    .expect("encode level pack");
    let content = PackReader::read(Cursor::new(pack_bytes)).expect("read level pack");
    let mut watcher = Watched::level(path, &content).expect("watcher");
    let region = Region::new(GLOBAL);
    for _ in 0..FRAMES {
        black_box(watcher.poll(&content));
    }
    let stats = region.change();
    assert_eq!(stats.allocations, 0, "idle watcher allocated: {stats:?}");
    assert_eq!(
        stats.reallocations, 0,
        "idle watcher reallocated: {stats:?}"
    );
    // Extract every transform and half the visuals, with both render slot tables pre-sized.
    let mut world = World::new(9).expect("world");
    register_components(&mut world).expect("components");
    let mut moving_entity = None;
    for index in 0..ENTITIES {
        let id = world.spawn().expect("spawn");
        if moving_entity.is_none() {
            moving_entity = Some(id);
        }
        world.insert(id, Transform::IDENTITY).expect("insert");
        // Every other entity is drawn, so the visual path is measured too.
        if index % 2 == 0 {
            let visual = Visual {
                mesh: MeshKind::Box,
                look: Look {
                    material: MaterialKind::Crate,
                    tint: None,
                },
            };
            world.insert(id, visual).expect("visual");
        }
    }
    let mut extractor = Extractor::with_capacity(ENTITIES);
    let mut frame = RenderFrame::with_capacity(ENTITIES);
    extractor
        .extract(&world, Tick::ZERO, &mut frame)
        .expect("initial extract");
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless GPU");
    let mut settings = RenderTier::High.settings();
    settings.taa = false;
    let mut gfx =
        Gfx::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, (128, 128), settings).expect("renderer");
    let pool = JobPool::new(0).expect("job pool");
    let mut store = AssetStore::new();
    let mut uploader = AssetUploader::new();
    uploader
        .update(
            &mut store,
            &mut gfx,
            &pool,
            RenderTier::High.upload_budget_bytes(),
        )
        .expect("idle uploader warm-up");
    let region = Region::new(GLOBAL);
    for _ in 0..FRAMES {
        black_box(
            uploader
                .update(
                    &mut store,
                    &mut gfx,
                    &pool,
                    RenderTier::High.upload_budget_bytes(),
                )
                .expect("idle uploader update"),
        );
    }
    let stats = region.change();
    assert_eq!(stats.allocations, 0, "idle uploader allocated: {stats:?}");
    assert_eq!(
        stats.reallocations, 0,
        "idle uploader reallocated: {stats:?}"
    );

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
    let scene = gfx.scene_mut();
    let mut primitives = Vec::with_capacity(ENTITIES / 2);
    for visual in frame.visuals() {
        let primitive = scene
            .add(
                mesh,
                PrimitiveDesc {
                    transform: Affine3A::IDENTITY,
                    material: render::Material::Textured(material),
                    layer: Layer::World,
                    casts_shadow: true,
                },
            )
            .expect("primitive");
        primitives.push((visual.id, primitive));
    }
    let planes = [Vec4::W; 6];
    for tick in 0..16u32 {
        extractor
            .extract(&world, Tick::new(tick), &mut frame)
            .expect("warm-up extract");
        update_scene(&frame, scene, &primitives);
        black_box(scene.cull(&planes, &[planes; 2], |_, _, _, _, _, _, _, _, _| {}));
    }

    let region = Region::new(GLOBAL);
    for tick in 0..FRAMES {
        if let Some(id) = moving_entity {
            let mut transform = world.get_mut::<Transform>(id).expect("moving transform");
            transform
                .set_translation(glam::Vec3::new(tick as f32 * 0.001, 0.0, 0.0))
                .expect("finite move");
        }
        let tick = Tick::new(u32::try_from(tick).expect("tick fits u32"));
        extractor
            .extract(black_box(&world), black_box(tick), &mut frame)
            .expect("extract");
        update_scene(&frame, scene, &primitives);
        black_box(scene.cull(&planes, &[planes; 2], |_, _, _, _, _, _, _, _, _| {}));
    }
    let stats = region.change();

    assert_eq!(
        stats.allocations, 0,
        "extract, retained scene update, or cull allocated: {stats:?}"
    );
    assert_eq!(
        stats.reallocations, 0,
        "extract, retained scene update, or cull reallocated: {stats:?}"
    );
    assert_eq!(frame.transforms().len(), ENTITIES);

    // Exchange: acquire, publish, next, recycle on one thread, after warm-up.
    let (mut sim, mut render) = frame_exchange(ENTITIES);
    for _ in 0..16 {
        let frame = sim.acquire().expect("acquire");
        sim.publish(frame).expect("publish");
        let frame = render.next_frame().expect("next");
        render.recycle(frame).expect("recycle");
    }

    let region = Region::new(GLOBAL);
    for _ in 0..FRAMES {
        let frame = black_box(sim.acquire().expect("acquire"));
        sim.publish(frame).expect("publish");
        let frame = black_box(render.next_frame().expect("next"));
        render.recycle(frame).expect("recycle");
    }
    let stats = region.change();

    assert_eq!(stats.allocations, 0, "exchange cycle allocated: {stats:?}");
    assert_eq!(
        stats.reallocations, 0,
        "exchange cycle reallocated: {stats:?}"
    );
}

fn update_scene(
    frame: &RenderFrame,
    scene: &mut RenderScene,
    primitives: &[(engine_world::NetId, render::PrimitiveId)],
) {
    for (visual_index, visual) in frame.visuals().iter().enumerate() {
        let (id, primitive) = primitives.get(visual_index).expect("primitive for visual");
        assert_eq!(*id, visual.id);
        let transform = frame
            .transform_entry(visual.id)
            .expect("transform for visual");
        let current = transform.current;
        let affine = Affine3A::from_scale_rotation_translation(
            current.scale(),
            current.rotation(),
            current.translation(),
        );
        scene
            .set_transform(*primitive, affine)
            .expect("update primitive");
    }
}
