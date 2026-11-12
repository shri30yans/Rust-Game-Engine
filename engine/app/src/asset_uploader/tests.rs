//! Uploader tests: pending-to-ready at the frame boundary, texture failures, the per-frame byte
//! budget, and reload that keeps the primitive live.

use std::io::Cursor;
use std::time::Instant;

use engine_assets::format::{
    Asset, Bounds, IndexEncoding, Material, Mesh, MeshLod, MeshPack, Node, PositionEncoding, encode,
};
use engine_assets::{AssetPath, AssetStore, ContentPack, PackReader};
use glam::{Affine3A, Vec3};
use jobs::JobPool;
use platform::Gpu;
use render::{Camera, Gfx, Layer, PrimitiveDesc, RenderTier};

use super::{AssetUploader, UploadError, UploadStatus};

fn mount_mesh(
    store: &mut AssetStore,
    pool: &JobPool,
    path: &str,
    mesh: MeshPack,
) -> Result<AssetPath, Box<dyn std::error::Error>> {
    let path = AssetPath::new(path)?;
    let source_hash = mesh.source_hash;
    let mesh_bytes = encode(&Asset::MeshPack(mesh), source_hash)?;
    let mut pack_bytes = Vec::new();
    ContentPack::write([(path.clone(), mesh_bytes)], &mut pack_bytes)?;
    let content = PackReader::read(Cursor::new(pack_bytes))?;
    store.mount_content(content, pool)?;
    Ok(path)
}

fn simple_pack(source_hash: u64, mesh_count: usize) -> MeshPack {
    let mesh = Mesh {
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
    };
    MeshPack {
        source_hash,
        meshes: vec![mesh; mesh_count],
        materials: vec![engine_assets::format::Material {
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
        collision: Vec::new(),
        textures: Vec::new(),
    }
}

#[test]
fn pack_status_moves_from_pending_to_ready_at_the_frame_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let pack = MeshPack {
        source_hash: 11,
        meshes: Vec::new(),
        materials: Vec::new(),
        nodes: vec![Node {
            name: "root".to_owned(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            tags: Vec::new(),
        }],
        clips: Vec::new(),
        collision: Vec::new(),
        textures: Vec::new(),
    };
    let pool = JobPool::new(0)?;
    let mut store = AssetStore::new();
    let path = mount_mesh(&mut store, &pool, "empty.meshpack", pack)?;
    let handle = store.load::<MeshPack>(&pool, path)?;
    let mut uploader = AssetUploader::new();
    assert!(matches!(uploader.status(&handle), UploadStatus::Pending));
    store.apply_loaded(&pool, Instant::now())?;
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    let mut gfx = Gfx::new(
        &gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        (32, 32),
        RenderTier::Low.settings(),
    )?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    assert!(matches!(uploader.status(&handle), UploadStatus::Ready(_)));
    store.release(handle)?;
    Ok(())
}

#[test]
fn a_missing_pack_texture_fails_with_its_content_hash() -> Result<(), Box<dyn std::error::Error>> {
    let hash = 0x1234_u64;
    let pack = MeshPack {
        source_hash: 12,
        meshes: Vec::new(),
        materials: vec![engine_assets::format::Material {
            base_color: [1.0; 4],
            metallic: 0.0,
            roughness: 1.0,
            base_color_texture: Some(hash),
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
        collision: Vec::new(),
        textures: vec![hash],
    };
    let pool = JobPool::new(0)?;
    let mut store = AssetStore::new();
    let path = mount_mesh(&mut store, &pool, "content/cooked/textured.meshpack", pack)?;
    let handle = store.load::<MeshPack>(&pool, path)?;
    let mut uploader = AssetUploader::new();
    store.apply_loaded(&pool, Instant::now())?;
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    let mut gfx = Gfx::new(
        &gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        (32, 32),
        RenderTier::Low.settings(),
    )?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    for _ in 0..3 {
        store.apply_loaded(&pool, Instant::now())?;
        uploader.update(
            &mut store,
            &mut gfx,
            &pool,
            RenderTier::Low.upload_budget_bytes(),
        )?;
        if matches!(uploader.status(&handle), UploadStatus::Failed(_)) {
            break;
        }
    }
    assert!(
        matches!(
            uploader.status(&handle),
            UploadStatus::Failed(UploadError::Texture { texture, .. }) if texture == "0000000000001234"
        ),
        "unexpected upload status: {:?}",
        uploader.status(&handle)
    );
    store.release(handle)?;
    Ok(())
}

#[test]
fn forty_megabyte_pack_spreads_uploads_within_each_desktop_frame_budget()
-> Result<(), Box<dyn std::error::Error>> {
    const MESH_COUNT: usize = 8;
    const VERTEX_COUNT: u32 = 175_000;
    const INDEX_COUNT: u32 = 262_500;
    const BYTE_BUDGET: u64 = RenderTier::High.upload_budget_bytes();
    let vertex_bytes = usize::try_from(VERTEX_COUNT)? * 12;
    let index_bytes = usize::try_from(INDEX_COUNT)? * 4;
    let mesh_count = u64::try_from(MESH_COUNT)?;
    let mesh = || Mesh {
        lods: vec![MeshLod {
            positions: vec![0; vertex_bytes],
            attributes: vec![0; vertex_bytes],
            indices: vec![0; index_bytes],
            index_encoding: IndexEncoding::U32,
            position_encoding: PositionEncoding::Float32,
            bounds: Bounds {
                min: [-1.0; 3],
                max: [1.0; 3],
                center: [0.0; 3],
                radius: 1.0,
            },
            screen_size_threshold: 0.0,
            screen_error: 0.0,
            vertex_count: VERTEX_COUNT,
            index_count: INDEX_COUNT,
        }],
        material: 0,
        node: 0,
    };
    let pack = MeshPack {
        source_hash: 40,
        meshes: (0..MESH_COUNT).map(|_| mesh()).collect(),
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
        collision: Vec::new(),
        textures: Vec::new(),
    };
    let pool = JobPool::new(0)?;
    let mut store = AssetStore::new();
    let path = mount_mesh(&mut store, &pool, "large.meshpack", pack)?;
    let handle = store.load::<MeshPack>(&pool, path)?;
    let mut uploader = AssetUploader::new();
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    let mut gfx = Gfx::new(
        &gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        (32, 32),
        RenderTier::High.settings(),
    )?;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("large-pack-upload-budget-target"),
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
    let mut frame_count = 0;
    let mut total_bytes = 0u64;
    while !matches!(uploader.status(&handle), UploadStatus::Ready(_)) {
        store.apply_loaded(&pool, Instant::now())?;
        let bytes = uploader.update(&mut store, &mut gfx, &pool, BYTE_BUDGET)?;
        assert!(
            bytes <= BYTE_BUDGET,
            "uploader exceeded the frame budget: {bytes}"
        );
        gfx.note_asset_upload_bytes(bytes);
        gfx.begin_frame(render::Camera::default());
        let stats = gfx.render(&view, 1.0 / 60.0);
        assert!(stats.asset_upload_bytes <= BYTE_BUDGET);
        assert_eq!(stats.asset_upload_bytes, bytes);
        total_bytes = total_bytes.saturating_add(bytes);
        frame_count += 1;
        assert!(
            frame_count <= MESH_COUNT + 1,
            "large pack did not make progress"
        );
    }
    assert!(frame_count > 1, "large pack uploaded in one frame");
    assert_eq!(
        total_bytes,
        u64::from(VERTEX_COUNT) * 24 * mesh_count + u64::from(INDEX_COUNT) * 4 * mesh_count
    );
    store.release(handle)?;
    Ok(())
}

#[test]
fn reload_keeps_primitive_live_and_rejects_a_mesh_count_mismatch()
-> Result<(), Box<dyn std::error::Error>> {
    let pool = JobPool::new(0)?;
    let start = Instant::now();
    let mut store = AssetStore::new();
    let path = mount_mesh(&mut store, &pool, "reload.meshpack", simple_pack(1, 1))?;
    let handle = store.load::<MeshPack>(&pool, path)?;
    let mut uploader = AssetUploader::new();
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    let mut gfx = Gfx::new(
        &gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        (32, 32),
        RenderTier::Low.settings(),
    )?;
    let memory_before_load = gfx.asset_memory_bytes();
    store.apply_loaded(&pool, start)?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    let (old_mesh, old_material) = match uploader.status(&handle) {
        UploadStatus::Ready(pack) => (
            *pack.meshes.first().ok_or("initial mesh missing")?,
            *pack.materials.first().ok_or("initial material missing")?,
        ),
        status => return Err(format!("initial pack is not ready: {status:?}").into()),
    };
    let primitive = gfx.scene_mut().add(
        old_mesh,
        PrimitiveDesc {
            transform: Affine3A::from_translation(Vec3::new(0.0, 1.7, -6.0)),
            material: render::Material::Textured(old_material),
            layer: Layer::ViewModel,
            casts_shadow: false,
        },
    )?;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("asset-reload-primitive-target"),
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
    let memory_with_old = gfx.asset_memory_bytes();
    let render_primitive = |gfx: &mut Gfx| -> Result<u32, Box<dyn std::error::Error>> {
        gfx.begin_frame(Camera::default());
        Ok(gfx.render(&view, 1.0 / 60.0).instances)
    };
    assert_eq!(render_primitive(&mut gfx)?, 1);

    mount_mesh(&mut store, &pool, "reload.meshpack", simple_pack(2, 1))?;
    store.apply_loaded(&pool, start + std::time::Duration::from_millis(300))?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    assert_eq!(render_primitive(&mut gfx)?, 1);
    store.apply_loaded(&pool, start + std::time::Duration::from_millis(600))?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    let (new_mesh, new_material) = match uploader.status(&handle) {
        UploadStatus::Ready(pack) => (
            *pack.meshes.first().ok_or("reloaded mesh missing")?,
            *pack.materials.first().ok_or("reloaded material missing")?,
        ),
        status => return Err(format!("reloaded pack is not ready: {status:?}").into()),
    };
    assert_ne!(old_mesh, new_mesh);
    assert_ne!(old_material, new_material);
    gfx.scene_mut()
        .set_transform(primitive, Affine3A::IDENTITY)?;
    assert_eq!(render_primitive(&mut gfx)?, 1);
    assert_eq!(gfx.asset_memory_bytes(), memory_with_old);

    mount_mesh(&mut store, &pool, "reload.meshpack", simple_pack(3, 2))?;
    store.apply_loaded(&pool, start + std::time::Duration::from_millis(900))?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    store.apply_loaded(&pool, start + std::time::Duration::from_millis(1200))?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    assert_eq!(render_primitive(&mut gfx)?, 1);
    assert_eq!(gfx.asset_memory_bytes(), memory_with_old);
    assert!(
        matches!(uploader.status(&handle), UploadStatus::Ready(pack) if pack.meshes.first() == Some(&new_mesh))
    );
    gfx.scene_mut().remove(primitive)?;
    store.release(handle)?;
    store.apply_loaded(&pool, start + std::time::Duration::from_millis(1500))?;
    uploader.update(
        &mut store,
        &mut gfx,
        &pool,
        RenderTier::Low.upload_budget_bytes(),
    )?;
    assert_eq!(gfx.asset_memory_bytes(), memory_before_load);
    Ok(())
}
