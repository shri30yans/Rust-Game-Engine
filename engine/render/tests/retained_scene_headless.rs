//! Headless checks for retained instance upload accounting.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_assets::TextureData;
use engine_assets::format::{Bounds, IndexEncoding, MeshLod, PositionEncoding};
use glam::{Affine3A, Mat4, Vec3};
use platform::Gpu;
use render::{
    Camera, ColourSpace, Gfx, Layer, Look, Mat, MaterialDesc, MeshBuilder, MeshFormat,
    PrimitiveDesc, RenderError, RenderTier,
};

const SIZE: u32 = 256;
const PRIMITIVE_COUNT: usize = 1_000;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[test]
fn static_scene_uploads_no_instance_bytes_after_the_first_frame() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let mut settings = RenderTier::High.settings();
    settings.taa = false;
    let mut gfx = Gfx::new(&gpu, FORMAT, (SIZE, SIZE), settings).expect("renderer builds");
    let mesh = gfx
        .add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0)))
        .expect("mesh uploads");
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("retained-scene-target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    for _ in 0..PRIMITIVE_COUNT {
        gfx.scene_mut()
            .add(
                mesh,
                PrimitiveDesc {
                    transform: Affine3A::from_mat4(Mat4::from_translation(Vec3::new(
                        0.0, 1.7, -6.0,
                    ))),
                    material: render::Material::Procedural(Look::default()),
                    layer: Layer::World,
                    casts_shadow: true,
                },
            )
            .expect("primitive added");
    }
    gfx.begin_frame(Camera::default());
    gfx.render(&view, 1.0 / 60.0);
    gfx.begin_frame(Camera::default());
    let second = gfx.render(&view, 1.0 / 60.0);
    assert_eq!(second.dirty_slots, 0);
    assert_eq!(second.instance_bytes_uploaded, 0);
}

#[test]
fn moving_one_primitive_uploads_current_then_previous_transform_with_taa() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let mut settings = RenderTier::High.settings();
    settings.taa = true;
    let mut gfx = Gfx::new(&gpu, FORMAT, (SIZE, SIZE), settings).expect("renderer builds");
    let mesh = gfx
        .add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0)))
        .expect("mesh uploads");
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("retained-scene-taa-target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let primitive = gfx
        .scene_mut()
        .add(
            mesh,
            PrimitiveDesc {
                transform: Affine3A::from_mat4(Mat4::from_translation(Vec3::new(0.0, 1.7, -6.0))),
                material: render::Material::Procedural(Look::default()),
                layer: Layer::World,
                casts_shadow: true,
            },
        )
        .expect("primitive added");
    gfx.begin_frame(Camera::default());
    let initial = gfx.render(&view, 1.0 / 60.0);
    assert_eq!(initial.instance_bytes_uploaded, 112);

    gfx.begin_frame(Camera::default());
    gfx.scene_mut()
        .set_transform(
            primitive,
            Affine3A::from_mat4(Mat4::from_translation(Vec3::new(1.0, 1.7, -6.0))),
        )
        .expect("primitive moved");
    let moved = gfx.render(&view, 1.0 / 60.0);
    assert_eq!(moved.dirty_slots, 1);
    assert_eq!(moved.instance_bytes_uploaded, 64);

    gfx.begin_frame(Camera::default());
    let settled = gfx.render(&view, 1.0 / 60.0);
    assert_eq!(settled.instance_bytes_uploaded, 48);
    gfx.begin_frame(Camera::default());
    let static_again = gfx.render(&view, 1.0 / 60.0);
    assert_eq!(static_again.instance_bytes_uploaded, 0);
}

#[test]
fn mesh_removal_refuses_live_users_and_invalidates_the_mesh_id() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let mut gfx =
        Gfx::new(&gpu, FORMAT, (SIZE, SIZE), RenderTier::High.settings()).expect("renderer builds");
    let starting_memory = gfx.asset_memory_bytes();
    let mesh = gfx
        .add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0)))
        .expect("mesh uploads");
    let primitive = gfx
        .scene_mut()
        .add(
            mesh,
            PrimitiveDesc {
                transform: Affine3A::IDENTITY,
                material: render::Material::Procedural(Look::default()),
                layer: Layer::World,
                casts_shadow: true,
            },
        )
        .expect("primitive added");
    assert!(matches!(
        gfx.remove_mesh(mesh),
        Err(RenderError::MeshInUse(_))
    ));
    gfx.scene_mut()
        .remove(primitive)
        .expect("primitive removed");
    gfx.remove_mesh(mesh).expect("mesh released");
    assert_eq!(gfx.asset_memory_bytes(), starting_memory);
    assert!(matches!(
        gfx.scene_mut().add(
            mesh,
            PrimitiveDesc {
                transform: Affine3A::IDENTITY,
                material: render::Material::Procedural(Look::default()),
                layer: Layer::World,
                casts_shadow: true,
            }
        ),
        Err(RenderError::UnknownMesh(_))
    ));
}

#[test]
fn texture_memory_accounting_returns_to_zero_after_removal() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let mut gfx =
        Gfx::new(&gpu, FORMAT, (SIZE, SIZE), RenderTier::High.settings()).expect("renderer builds");
    let starting_memory = gfx.asset_memory_bytes();
    let source = TextureData {
        ktx2: include_bytes!("../../assets/tests/fixtures/uastc.ktx2").to_vec(),
    };
    let prepared = engine_assets::prepare_texture(&source, ColourSpace::Srgb, gfx.texture_family())
        .expect("texture prepares");
    let id = gfx.add_texture(prepared).expect("texture uploads");
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture-accounting-target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    gfx.begin_frame(Camera::default());
    let uploaded = gfx.render(&view, 1.0 / 60.0);
    assert!(uploaded.texture_bytes_by_family.iter().sum::<u64>() > 0);
    gfx.remove_texture(id).expect("texture released");
    assert_eq!(gfx.asset_memory_bytes(), starting_memory);
    gfx.begin_frame(Camera::default());
    let released = gfx.render(&view, 1.0 / 60.0);
    assert_eq!(released.texture_bytes_by_family, [0; 4]);
}

#[test]
fn textured_fixture_samples_real_pbr_output_on_desktop_and_phone_tiers() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let source = TextureData {
        ktx2: include_bytes!("../../assets/tests/fixtures/uastc.ktx2").to_vec(),
    };
    for tier in [RenderTier::High, RenderTier::Low] {
        let mut settings = tier.settings();
        settings.taa = false;
        settings.bloom = false;
        let mut gfx = Gfx::new(&gpu, FORMAT, (SIZE, SIZE), settings).expect("renderer builds");
        let mut textures = Vec::with_capacity(3);
        for colour_space in [ColourSpace::Srgb, ColourSpace::Linear, ColourSpace::Linear] {
            let prepared =
                engine_assets::prepare_texture(&source, colour_space, gfx.texture_family())
                    .expect("fixture transcodes");
            textures.push(gfx.add_texture(prepared).expect("fixture uploads"));
        }
        let material = gfx
            .add_material(MaterialDesc {
                base_color_texture: textures.first().copied(),
                normal_texture: textures.get(1).copied(),
                orm_texture: textures.get(2).copied(),
                ..MaterialDesc::default()
            })
            .expect("textured material creates");
        let mesh = gfx
            .add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0)))
            .expect("mesh uploads");
        gfx.scene_mut()
            .add(
                mesh,
                PrimitiveDesc {
                    transform: Affine3A::from_translation(Vec3::new(0.0, 1.7, -6.0)),
                    material: render::Material::Textured(material),
                    layer: Layer::World,
                    casts_shadow: true,
                },
            )
            .expect("textured fixture enters scene");
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("textured-pbr-golden-target"),
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
        let pixels = read_target(&gpu, &target);
        let center = pixel_rgb(&pixels, SIZE / 2, SIZE / 2);
        let lit_neighbor = pixel_rgb(&pixels, SIZE / 2 + 20, SIZE / 2);
        assert!(
            center.iter().any(|channel| *channel > 20),
            "{tier:?}: {center:?}"
        );
        assert_ne!(
            center, lit_neighbor,
            "{tier:?}: textured material had no visible variation"
        );
    }
}

fn read_target(gpu: &Gpu, texture: &wgpu::Texture) -> Vec<u8> {
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("textured-pbr-golden-readback"),
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
            let _ = sender.send(result);
        });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device readback completes");
    receiver
        .recv()
        .expect("readback callback runs")
        .expect("readback buffer maps");
    let mapped = buffer
        .slice(..)
        .get_mapped_range()
        .expect("mapped range is available");
    let pixels = mapped.to_vec();
    drop(mapped);
    pixels
}

fn pixel_rgb(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let offset = usize::try_from((y * SIZE + x) * 4).expect("pixel offset fits usize");
    let Some(rgb) = pixels.get(offset..offset + 3) else {
        return [0; 3];
    };
    rgb.try_into().unwrap_or([0; 3])
}

#[test]
fn textured_material_uses_a_shared_group_and_cannot_be_removed_while_live() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let mut gfx =
        Gfx::new(&gpu, FORMAT, (SIZE, SIZE), RenderTier::High.settings()).expect("renderer builds");
    let mesh = gfx
        .add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0)))
        .expect("mesh uploads");
    let material = gfx
        .add_material(MaterialDesc::default())
        .expect("material group creates");
    let primitive = gfx
        .scene_mut()
        .add(
            mesh,
            PrimitiveDesc {
                transform: Affine3A::from_translation(Vec3::new(0.0, 1.7, -6.0)),
                material: render::Material::Textured(material),
                layer: Layer::World,
                casts_shadow: true,
            },
        )
        .expect("textured primitive registers");
    assert!(matches!(
        gfx.remove_material(material),
        Err(RenderError::MaterialInUse(_))
    ));
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("textured-material-target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    gfx.begin_frame(Camera::default());
    let stats = gfx.render(&view, 1.0 / 60.0);
    assert_eq!(stats.instances, 1);
    gfx.scene_mut()
        .remove(primitive)
        .expect("primitive removed");
    gfx.remove_material(material).expect("material released");
    assert!(matches!(
        gfx.remove_material(material),
        Err(RenderError::UnknownMaterial(_))
    ));
}

#[test]
fn every_tier_builds_all_scene_pipeline_keys_on_downlevel_limits() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let limits = gpu.device.limits();
    let downlevel = wgpu::Limits::downlevel_defaults();
    assert!(limits.max_bind_groups >= downlevel.max_bind_groups);
    assert!(
        limits.max_storage_buffers_per_shader_stage
            >= downlevel.max_storage_buffers_per_shader_stage
    );
    assert!(
        limits.max_sampled_textures_per_shader_stage
            >= downlevel.max_sampled_textures_per_shader_stage
    );
    for tier in [
        RenderTier::Test,
        RenderTier::Low,
        RenderTier::Medium,
        RenderTier::High,
        RenderTier::Ultra,
    ] {
        Gfx::new(&gpu, FORMAT, (SIZE, SIZE), tier.settings())
            .expect("all pass/format/material pipelines build");
    }
}

#[test]
fn cooked_lod_selection_holds_its_choice_through_the_hysteresis_band() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let mut gfx =
        Gfx::new(&gpu, FORMAT, (SIZE, SIZE), RenderTier::Test.settings()).expect("renderer builds");
    let mut lods = [
        cooked_triangle(PositionEncoding::Float32),
        cooked_triangle(PositionEncoding::Float32),
        cooked_triangle(PositionEncoding::Float32),
    ];
    for (lod, threshold) in lods.iter_mut().zip([0.5, 0.25, 0.0]) {
        lod.screen_size_threshold = threshold;
        lod.bounds.radius = 1.0;
    }
    let mesh = gfx.add_cooked_mesh(&lods).expect("LOD set uploads");
    let primitive = gfx
        .scene_mut()
        .add(
            mesh,
            PrimitiveDesc {
                transform: Affine3A::from_translation(Vec3::new(0.0, 1.7, -3.4)),
                material: render::Material::Procedural(Look::default()),
                layer: Layer::World,
                casts_shadow: false,
            },
        )
        .expect("primitive registers");
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lod-dolly-target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    for (screen_size, expected_lod) in [(0.44, 1usize), (0.46, 1), (0.44, 1), (0.56, 0)] {
        let distance = 1.0 / (screen_size * (Camera::default().fov_y * 0.5).tan());
        gfx.scene_mut()
            .set_transform(
                primitive,
                Affine3A::from_translation(Vec3::new(0.0, 1.7, -distance)),
            )
            .expect("dolly moves the primitive");
        gfx.begin_frame(Camera::default());
        let stats = gfx.render(&view, 1.0 / 60.0);
        assert_eq!(stats.lod_counts.get(expected_lod), Some(&1));
    }
}

#[test]
fn renderer_rejects_more_than_four_mesh_lods() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let mut gfx =
        Gfx::new(&gpu, FORMAT, (SIZE, SIZE), RenderTier::Test.settings()).expect("renderer builds");
    let lods = [
        cooked_triangle(PositionEncoding::Float32),
        cooked_triangle(PositionEncoding::Float32),
        cooked_triangle(PositionEncoding::Float32),
        cooked_triangle(PositionEncoding::Float32),
        cooked_triangle(PositionEncoding::Float32),
    ];
    assert!(matches!(
        gfx.add_cooked_mesh(&lods),
        Err(RenderError::InvalidCookedMesh)
    ));
}

#[test]
fn cooked_float_and_quantized_meshes_draw_through_retained_scene() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let mut gfx =
        Gfx::new(&gpu, FORMAT, (SIZE, SIZE), RenderTier::High.settings()).expect("renderer builds");
    let float = gfx
        .add_cooked_mesh(&[cooked_triangle(PositionEncoding::Float32)])
        .expect("float cooked mesh uploads");
    let quantized = gfx
        .add_cooked_mesh(&[cooked_triangle(PositionEncoding::Quantized16)])
        .expect("quantized cooked mesh uploads");
    assert_eq!(float.format(), MeshFormat::CookedFloat32);
    assert_eq!(quantized.format(), MeshFormat::CookedQuantized16);

    for mesh in [float, quantized] {
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
            .expect("cooked primitive registers");
    }
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cooked-mesh-target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    gfx.begin_frame(Camera::default());
    let stats = gfx.render(&view, 1.0 / 60.0);
    assert_eq!(stats.instances, 2);
}

#[test]
fn cooked_mesh_rejects_streams_that_disagree_with_the_declared_counts() {
    let gpu = Gpu::new(wgpu::Instance::default(), None).expect("headless render needs an adapter");
    let mut gfx =
        Gfx::new(&gpu, FORMAT, (SIZE, SIZE), RenderTier::Test.settings()).expect("renderer builds");
    let mut lod = cooked_triangle(PositionEncoding::Quantized16);
    lod.attributes.pop();
    assert!(matches!(
        gfx.add_cooked_mesh(std::slice::from_ref(&lod)),
        Err(RenderError::InvalidCookedMesh)
    ));
}

fn cooked_triangle(encoding: PositionEncoding) -> MeshLod {
    let positions = match encoding {
        PositionEncoding::Float32 => {
            bytemuck::cast_slice(&[[-0.8f32, -0.8, 0.0], [0.8, -0.8, 0.0], [0.0, 0.8, 0.0]])
                .to_vec()
        }
        PositionEncoding::Quantized16 => [
            [6_553u16, 6_553u16, 32_767u16, 0u16],
            [58_982, 6_553, 32_767, 0],
            [32_767, 58_982, 32_767, 0],
        ]
        .into_iter()
        .flat_map(|vertex| vertex.into_iter().flat_map(u16::to_le_bytes))
        .collect(),
    };
    let mut attributes = Vec::with_capacity(36);
    for _ in 0..3 {
        attributes.extend_from_slice(&[0; 12]);
    }
    MeshLod {
        positions,
        attributes,
        indices: [0u16, 1, 2]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect(),
        index_encoding: IndexEncoding::U16,
        position_encoding: encoding,
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
