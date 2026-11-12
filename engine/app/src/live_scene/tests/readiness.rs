//! Waiting for cooked visuals and committing edits at a frame boundary.
use super::*;
use std::time::Duration;

#[test]
fn a_world_without_a_watched_level_does_not_enable_live_reload() {
    let mut world = World::new(1).expect("world");
    let mut live = LiveScene::new();
    let (mut store, uploader, pool) = reload_assets();
    assert_eq!(
        live.poll_at(
            &mut world,
            &engine_scene::SceneContent::empty(),
            &mut store,
            &uploader,
            &pool,
            Instant::now(),
        )
        .expect("no watcher is inactive"),
        None
    );
}

pub(super) fn visual_pack(source_hash: u64) -> engine_assets::format::MeshPack {
    use engine_assets::format::{
        Bounds, CollisionSet, CollisionShape, IndexEncoding, Material, Mesh, MeshLod, Node,
        PositionEncoding,
    };

    engine_assets::format::MeshPack {
        source_hash,
        meshes: vec![Mesh {
            lods: vec![MeshLod {
                positions: vec![0; 36],
                attributes: vec![0; 36],
                indices: vec![0; 6],
                index_encoding: IndexEncoding::U16,
                position_encoding: PositionEncoding::Float32,
                bounds: Bounds {
                    min: [-1.0; 3],
                    max: [1.0; 3],
                    center: [0.0; 3],
                    radius: 1.0,
                },
                screen_size_threshold: 0.0,
                screen_error: 0.0,
                vertex_count: 3,
                index_count: 3,
            }],
            material: 0,
            node: 0,
        }],
        materials: vec![Material {
            base_color: [1.0; 4],
            metallic: 0.0,
            roughness: 1.0,
            base_color_texture: None,
            orm_texture: None,
            normal_texture: None,
            emissive_texture: None,
            emissive: false,
        }],
        nodes: vec![Node {
            name: "root".to_owned(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            tags: Vec::new(),
        }],
        clips: Vec::new(),
        collision: vec![CollisionSet {
            node: 0,
            surface: "test".to_owned(),
            is_sensor: false,
            shapes: vec![CollisionShape::Box {
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                half_extents: [1.0; 3],
            }],
        }],
        textures: Vec::new(),
    }
}

#[test]
fn pending_cooked_replacement_keeps_the_old_entity_and_primitive_live()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::extract::{Extractor, RenderFrame};
    use crate::scene_present::ScenePresenter;
    use engine_assets::format::{Asset, AssetLocation, encode};
    use engine_assets::{AssetPath, ContentPack, PackReader};
    use platform::Gpu;
    use render::{Camera, Gfx, RenderTier};

    let old_id = AssetId::new(1)?;
    let replacement_id = AssetId::new(2)?;
    let index = AssetIndex::from([
        (
            old_id,
            AssetLocation {
                pack: "old.meshpack".to_owned(),
                node: Some(0),
                content_hash: 11,
            },
        ),
        (
            replacement_id,
            AssetLocation {
                pack: "replacement.meshpack".to_owned(),
                node: Some(0),
                content_hash: 22,
            },
        ),
    ]);
    let initial = "(version: 1, name: \"old\", gravity: (0.0, -9.81, 0.0), instances: [(asset_id: 1, transform: (translation: (0.0, 0.0, -6.0)))])";
    let replacement = "(version: 1, name: \"replacement\", gravity: (0.0, -9.81, 0.0), instances: [(asset_id: 2, transform: (translation: (0.0, 0.0, -6.0)))])";
    let content_pack = |level: &str| -> Result<ContentPack, Box<dyn std::error::Error>> {
        let mut bytes = Vec::new();
        ContentPack::write(
            [
                (
                    AssetPath::new("content/cooked/index.ron")?,
                    ron::to_string(&index)?.into_bytes(),
                ),
                (
                    AssetPath::new("content/cooked/old.meshpack")?,
                    encode(&Asset::MeshPack(visual_pack(11)), 11)?,
                ),
                (
                    AssetPath::new("content/cooked/replacement.meshpack")?,
                    encode(&Asset::MeshPack(visual_pack(22)), 22)?,
                ),
                (AssetPath::new("game/level.ron")?, level.as_bytes().to_vec()),
            ],
            &mut bytes,
        )?;
        Ok(PackReader::read(Cursor::new(bytes))?)
    };
    let initial_pack = content_pack(initial)?;
    let watched = Watched::level(AssetPath::new("game/level.ron")?, &initial_pack)?;
    let mut world = World::new(4)?;
    engine_scene::register_components(&mut world)?;
    let content = SceneContent::new(initial_pack.clone(), PrefabRegistry::default(), |_, _| {
        engine_scene::ColliderMetadata::default()
    });
    let old_level = Level::parse(initial, "old.ron")?;
    install_level_from_scene_content(&mut world, &old_level, &content)?;
    let old_entity = engine_scene::installed(&world)?
        .entries()
        .get(&old_id)
        .ok_or("old entry missing")?
        .id;
    world.insert_resource(WatchedLevel::new(watched))?;

    let pool = JobPool::new(0)?;
    let mut store = AssetStore::new();
    let mut uploader = AssetUploader::new();
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    let mut gfx = Gfx::new(
        &gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        (32, 32),
        RenderTier::Low.settings(),
    )?;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("live-scene-pending-replacement"),
        size: wgpu::Extent3d {
            width: 32,
            height: 32,
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
    let mut scene = ScenePresenter::default();
    scene.setup(&mut gfx)?;
    store.mount_content(initial_pack.clone(), &pool)?;
    let old_handle =
        store.load::<MeshPack>(&pool, AssetPath::new("content/cooked/old.meshpack")?)?;
    std::thread::sleep(Duration::from_millis(10));
    store.apply_loaded(&pool, Instant::now())?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    assert!(uploader.is_ready(&old_handle, 11));
    let mut extractor = Extractor::with_capacity(4);
    let mut frame = RenderFrame::with_capacity(4);
    let t0 = Instant::now();
    let draw = |scene: &mut ScenePresenter,
                frame: &RenderFrame,
                gfx: &mut Gfx,
                store: &mut AssetStore,
                uploader: &AssetUploader|
     -> Result<u32, Box<dyn std::error::Error>> {
        let mut assets = crate::asset_uploader::ClientAssets {
            store,
            uploader,
            pool: &pool,
        };
        scene.load_frame_assets(frame, &mut assets)?;
        gfx.begin_frame(Camera::default());
        scene.draw(frame, 0.0, gfx, &mut assets)?;
        Ok(gfx.render(&view, 1.0 / 60.0).instances)
    };

    live_scene_frame(&mut extractor, &world, &mut frame)?;
    draw(&mut scene, &frame, &mut gfx, &mut store, &uploader)?;
    std::thread::sleep(Duration::from_millis(10));
    store.apply_loaded(&pool, t0)?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    assert_eq!(
        draw(&mut scene, &frame, &mut gfx, &mut store, &uploader)?,
        1
    );

    let mut live = LiveScene::new();
    let _ = live.poll_at(&mut world, &content, &mut store, &uploader, &pool, t0)?;
    store.mount_content(content_pack(replacement)?, &pool)?;
    assert_eq!(
        live.poll_at(
            &mut world,
            &content,
            &mut store,
            &uploader,
            &pool,
            t0 + POLL_INTERVAL,
        )?,
        None
    );
    assert!(
        world.is_alive(old_entity),
        "pending edit removed the old entity"
    );
    live_scene_frame(&mut extractor, &world, &mut frame)?;
    assert_eq!(
        draw(&mut scene, &frame, &mut gfx, &mut store, &uploader)?,
        1
    );

    std::thread::sleep(Duration::from_millis(10));
    store.apply_loaded(&pool, t0 + POLL_INTERVAL * 2)?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    assert!(
        world.is_alive(old_entity),
        "upload frame removed the old entity"
    );
    assert_eq!(
        draw(&mut scene, &frame, &mut gfx, &mut store, &uploader)?,
        1
    );

    let committed = live
        .poll_at(
            &mut world,
            &content,
            &mut store,
            &uploader,
            &pool,
            t0 + POLL_INTERVAL * 3,
        )?
        .ok_or("ready replacement did not commit")?;
    assert_eq!(committed.name(), "replacement");
    assert!(!world.is_alive(old_entity));
    live_scene_frame(&mut extractor, &world, &mut frame)?;
    assert_eq!(
        draw(&mut scene, &frame, &mut gfx, &mut store, &uploader)?,
        1
    );
    Ok(())
}

fn live_scene_frame(
    extractor: &mut crate::extract::Extractor,
    world: &World,
    frame: &mut crate::extract::RenderFrame,
) -> Result<(), AppError> {
    extractor.extract(world, engine_core::Tick::ZERO, frame)
}
