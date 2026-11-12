//! Initial [`super::Gfx`] construction.

use super::pipelines::{self, GfxPipeCtx, HDR, PipelineLayouts};
use super::*;
use crate::RenderError;
use crate::font::ATLAS;
use crate::screen_space;
use crate::shaders::{ShaderId, ShaderLibrary, create_module};
use crate::taa::Jitter;
use crate::targets::{self, FrameTargetDesc, TaaTargetDesc};
use crate::tiles::Tiles;
use crate::timer::GpuTimer;
use crate::ui::Ui;
use platform::Gpu;

/// GPU resources created on the current test thread while a render-scale change runs.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResourceCreationCounts {
    pub textures: u64,
    pub views: u64,
    pub bind_groups: u64,
}

#[cfg(test)]
std::thread_local! {
    static RESOURCE_CREATIONS: std::cell::Cell<ResourceCreationCounts> = const {
        std::cell::Cell::new(ResourceCreationCounts {
            textures: 0,
            views: 0,
            bind_groups: 0,
        })
    };
}

#[cfg(test)]
pub(crate) fn resource_creation_counts() -> ResourceCreationCounts {
    RESOURCE_CREATIONS.get()
}

#[cfg(test)]
pub(crate) fn record_texture_creation() {
    RESOURCE_CREATIONS.update(|counts| {
        let mut counts = counts;
        counts.textures = counts.textures.saturating_add(1);
        counts
    });
}

#[cfg(test)]
pub(crate) fn record_view_creation() {
    RESOURCE_CREATIONS.update(|counts| {
        let mut counts = counts;
        counts.views = counts.views.saturating_add(1);
        counts
    });
}

#[cfg(test)]
pub(crate) fn record_bind_group_creation() {
    RESOURCE_CREATIONS.update(|counts| {
        let mut counts = counts;
        counts.bind_groups = counts.bind_groups.saturating_add(1);
        counts
    });
}

/// Keeps [`ShaderId::ALL`] and [`ShaderId::files`] referenced in non-hot-reload builds.
fn record_shader_inventory() {
    let mut file_refs = 0usize;
    for id in ShaderId::ALL {
        file_refs = file_refs.saturating_add(id.files().len());
    }
    let _ = file_refs;
}

/// Prebuilt bind groups for the terminal renderer debug views.
pub(super) fn debug_bindings(
    device: &wgpu::Device,
    layouts: &PipelineLayouts,
    targets: &crate::targets::FrameTargets,
    screen: Option<&crate::screen_space::ScreenSpace>,
    shadow: &wgpu::TextureView,
    coverage_enabled: bool,
) -> DebugBindings {
    let create =
        |label: &str, layout: &wgpu::BindGroupLayout, binding: u32, view: &wgpu::TextureView| {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::TextureView(view),
                }],
            });
            #[cfg(test)]
            record_bind_group_creation();
            group
        };
    DebugBindings {
        hdr: create("debug-hdr", &layouts.debug_color_bgl, 5, &targets.hdr),
        ambient_occlusion: screen.map(|screen| {
            create(
                "debug-ao",
                &layouts.debug_color_bgl,
                5,
                &screen.ao_final_view,
            )
        }),
        depth: screen.map(|screen| {
            create(
                "debug-depth",
                &layouts.debug_depth_bgl,
                6,
                &screen.depth_sample_view,
            )
        }),
        coverage: if coverage_enabled {
            screen
                .map(|screen| &screen.depth_sample_view)
                .or(targets.depth.as_ref())
                .map(|depth| {
                    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("debug-coverage"),
                        layout: &layouts.debug_coverage_bgl,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 5,
                                resource: wgpu::BindingResource::TextureView(&targets.hdr),
                            },
                            wgpu::BindGroupEntry {
                                binding: 6,
                                resource: wgpu::BindingResource::TextureView(depth),
                            },
                        ],
                    });
                    #[cfg(test)]
                    record_bind_group_creation();
                    group
                })
        } else {
            None
        },
        shadow: create("debug-shadow", &layouts.debug_shadow_bgl, 7, shadow),
    }
}

pub(super) fn supported_msaa(
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    settings: Settings,
) -> u32 {
    let features = adapter.get_texture_format_features(HDR);
    let requested = settings.effective_msaa();
    let adapter_specific_sample_counts = device
        .features()
        .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
    if requested > 1
        && features.flags.sample_count_supported(requested)
        && (requested == 4 || adapter_specific_sample_counts)
    {
        requested
    } else {
        1
    }
}

/// Builds the material texture sampler selected by a quality tier.
pub(super) fn material_sampler(device: &wgpu::Device, tier: crate::RenderTier) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("material-anisotropic"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: match tier {
            crate::RenderTier::Test | crate::RenderTier::Low => 2,
            crate::RenderTier::Medium => 4,
            crate::RenderTier::High | crate::RenderTier::Ultra => 8,
        },
        ..Default::default()
    })
}

/// Prepares the shadow-map views for both cascades.
pub(super) fn shadow_views(
    device: &wgpu::Device,
    size: u32,
) -> ([wgpu::TextureView; 2], wgpu::TextureView) {
    #[cfg(test)]
    record_texture_creation();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 2,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: pipelines::DEPTH,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let layer = |index: u32| {
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("shadow-layer"),
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: index,
            array_layer_count: Some(1),
            ..Default::default()
        });
        #[cfg(test)]
        record_view_creation();
        view
    };
    let array = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("shadow-arr"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    #[cfg(test)]
    record_view_creation();
    ([layer(0), layer(1)], array)
}

pub(crate) fn new_gfx(
    gpu: &Gpu,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    settings: Settings,
) -> Result<Gfx, RenderError> {
    let plan = crate::FramePlan::build(settings)?;
    let features = settings.features();
    let device = gpu.device.clone();
    let queue = gpu.queue.clone();
    let adapter_features = gpu.adapter.features();
    let texture_family = crate::choose_texture_family(crate::TextureCapabilities {
        bc: adapter_features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
        astc_4x4: adapter_features.contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC),
        etc2: adapter_features.contains(wgpu::Features::TEXTURE_COMPRESSION_ETC2),
    });
    let msaa = supported_msaa(&gpu.adapter, &gpu.device, settings);
    let compact_history = wgpu::TextureFormat::Rg11b10Ufloat;
    let compact_features = gpu.adapter.get_texture_format_features(compact_history);
    let adapter_specific_formats = gpu
        .device
        .features()
        .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
    let taa_history_format = if adapter_specific_formats
        && compact_features
            .allowed_usages
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
        && compact_features
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
    {
        compact_history
    } else {
        HDR
    };
    record_shader_inventory();
    let shader_library = ShaderLibrary::new();
    let tiles_write_bgl = Tiles::bind_group_layout(&device);
    let layouts = PipelineLayouts::new(&device, tiles_write_bgl);
    let scene_sm = create_module(&device, ShaderId::Scene, &shader_library)?;
    let shadow_sm = create_module(&device, ShaderId::Shadow, &shader_library)?;
    let post_sm = create_module(&device, ShaderId::Post, &shader_library)?;
    let gtao_sm = create_module(&device, ShaderId::Gtao, &shader_library)?;
    let denoise_sm = create_module(&device, ShaderId::GtaoDenoise, &shader_library)?;
    let ui_sm = create_module(&device, ShaderId::Ui, &shader_library)?;
    let tiles_sm = create_module(&device, ShaderId::Tiles, &shader_library)?;
    let taa_sm = create_module(&device, ShaderId::Taa, &shader_library)?;

    let msaa_state = wgpu::MultisampleState {
        count: msaa,
        mask: !0,
        alpha_to_coverage_enabled: false,
    };
    let pipe_ctx = GfxPipeCtx {
        format,
        msaa: msaa_state,
        taa: features.taa(),
        opaque_depth: plan.opaque_depth(),
    };
    let tiles = Tiles::new(
        &device,
        &layouts.globals_bgl,
        layouts.tiles_write_bgl.clone(),
        (size.0.max(1), size.1.max(1)),
        &tiles_sm,
    );
    let scene = pipelines::create_scene_pipelines(&device, &layouts, &scene_sm, &pipe_ctx);
    let post = pipelines::create_post_pipelines(&device, &layouts, &post_sm, &pipe_ctx);
    let debug = pipelines::create_debug_pipelines(&device, &layouts, &post_sm, &pipe_ctx);
    let p_shadow = pipelines::create_shadow_pipeline(&device, &layouts, &shadow_sm);
    let p_ui = pipelines::create_ui_pipeline(&device, &layouts, &ui_sm, &pipe_ctx);

    let buf = |label: &str, size: u64, usage: wgpu::BufferUsages| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let uni = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
    let vtx = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
    let globals_buf = buf("globals", std::mem::size_of::<Globals>() as u64, uni);
    let shadow_u = [buf("su0", 64, uni), buf("su1", 64, uni)];
    let mk_sbg = |b: &wgpu::Buffer| {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("su"),
            layout: &layouts.shadow_u_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: b.as_entire_binding(),
            }],
        });
        #[cfg(test)]
        record_bind_group_creation();
        group
    };
    let shadow_ubg = [mk_sbg(&shadow_u[0]), mk_sbg(&shadow_u[1])];
    let (shadow_layers, shadow_arr) = shadow_views(&device, settings.shadow_size);
    let cmp = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("shadow-cmp"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        compare: Some(wgpu::CompareFunction::LessEqual),
        ..Default::default()
    });
    let linear = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("linear"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    });
    let material_sampler = material_sampler(&device, settings.tier);
    let default_material_textures = [
        default_material_texture(
            &device,
            &queue,
            "default-base-color",
            [255, 255, 255, 255],
            true,
        ),
        default_material_texture(
            &device,
            &queue,
            "default-normal",
            [128, 128, 255, 255],
            false,
        ),
        default_material_texture(&device, &queue, "default-orm", [255, 255, 0, 255], false),
        default_material_texture(&device, &queue, "default-emissive", [0, 0, 0, 255], true),
    ];
    let post_buf = buf("post", std::mem::size_of::<PostU>() as u64, uni);
    let instance_capacity = INITIAL_SCENE_CAPACITY;
    let index_capacity = instance_capacity.saturating_mul(INDEXES_PER_PRIMITIVE);
    let scene_storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
    let inst_buf = buf(
        "instances",
        (instance_capacity * std::mem::size_of::<Instance>()) as u64,
        scene_storage,
    );
    let previous_inst_buf = buf(
        "previous-instances",
        (instance_capacity * std::mem::size_of::<PreviousTransform>()) as u64,
        scene_storage,
    );
    let index_buf = buf(
        "visible-slot-indices",
        (index_capacity * std::mem::size_of::<u32>()) as u64,
        scene_storage,
    );
    let mesh_records_buf = buf(
        "mesh-records",
        u64::try_from(instance_capacity)
            .unwrap_or(u64::MAX)
            .saturating_mul(std::mem::size_of::<MeshRecord>() as u64),
        scene_storage,
    );
    let scene_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene-instances"),
        layout: &layouts.scene_instances_bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: inst_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: index_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: previous_inst_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: mesh_records_buf.as_entire_binding(),
            },
        ],
    });
    let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals-and-scene"),
        layout: &layouts.globals_bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: inst_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: index_buf.as_entire_binding(),
            },
        ],
    });
    let globals_taa_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals-scene-taa"),
        layout: &layouts.globals_taa_bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: inst_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: index_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: previous_inst_buf.as_entire_binding(),
            },
        ],
    });
    #[cfg(test)]
    record_bind_group_creation();
    let part_buf = buf(
        "particles",
        (MAX_PARTICLES * std::mem::size_of::<Particle>()) as u64,
        vtx,
    );
    let ui_buf = buf(
        "ui",
        (MAX_UI_VERTS * std::mem::size_of::<UiVertex>()) as u64,
        vtx,
    );
    let ui_u = buf("ui-u", 256, uni);
    let gamma = if format.is_srgb() { 0.0f32 } else { 1.0 };
    queue.write_buffer(&ui_u, 0, bytemuck::cast_slice(&[gamma, 0.0, 0.0, 0.0]));
    #[cfg(test)]
    record_texture_creation();
    let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("atlas"),
        size: wgpu::Extent3d {
            width: ATLAS,
            height: ATLAS,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let atlas_view = atlas_tex.create_view(&Default::default());
    #[cfg(test)]
    record_view_creation();
    let ui_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ui"),
        layout: &layouts.ui_bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&atlas_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&linear),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: ui_u.as_entire_binding(),
            },
        ],
    });
    #[cfg(test)]
    record_bind_group_creation();

    let screen = features.prepass_enabled().then(|| {
        screen_space::create(&screen_space::ScreenSpaceDesc {
            device: &device,
            features,
            scene_shader: &scene_sm,
            post_shader: &post_sm,
            gtao_shader: &gtao_sm,
            denoise_shader: &denoise_sm,
            globals_taa_bgl: &layouts.globals_taa_bgl,
            prepass_layout: &layouts.scene_layout,
            post_bgl: &layouts.post_bgl,
            linear: &linear,
            window_size: size,
            taa: features.taa(),
            taa_history_format,
            taa_shader: &taa_sm,
        })
    });
    let (white_ao, exposure_neutral) = targets::neutral_1x1(&device, &queue);
    let ao_view = screen
        .as_ref()
        .map(|screen| &screen.ao_final_view)
        .unwrap_or(&white_ao);
    let view_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene-view"),
        layout: &layouts.view_bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: inst_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: index_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: previous_inst_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: mesh_records_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: tiles.read_buffer().as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&shadow_arr),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Sampler(&cmp),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(ao_view),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::Sampler(&linear),
            },
        ],
    });
    #[cfg(test)]
    record_bind_group_creation();
    let exp_view = screen
        .as_ref()
        .map(|s| &s.exposure_view)
        .unwrap_or(&exposure_neutral);
    let taa_desc = screen.as_ref().and_then(|s| {
        let pipe = s.taa.as_ref()?;
        Some(TaaTargetDesc {
            bgl: &pipe.bgl,
            velocity: &s.velocity_view,
            depth: &s.depth_sample_view,
        })
    });
    let targets = targets::build(&FrameTargetDesc {
        device: &device,
        post_bgl: &layouts.post_bgl,
        sampler: &linear,
        post_buf: &post_buf,
        window_size: size,
        msaa,
        skip_depth: features.prepass_enabled(),
        exposure: exp_view,
        measure_exposure: &exposure_neutral,
        taa_history_format,
        taa: taa_desc,
    });
    let debug_bindings = debug_bindings(
        &device,
        &layouts,
        &targets,
        screen.as_ref(),
        &shadow_arr,
        matches!(settings.view_mode, crate::ViewMode::OpaqueCoverage { .. }),
    );
    let mut ui = Ui::new();
    ui.size = glam::Vec2::new(size.0 as f32, size.1 as f32);
    #[cfg(feature = "hot-reload")]
    let shader_watcher = crate::hot_reload::ShaderWatcher::default_watcher()?;
    Ok(Gfx {
        adapter: gpu.adapter.clone(),
        device,
        queue,
        format,
        size,
        settings,
        plan,
        msaa,
        taa_history_format,
        meshes: Slots::new(),
        mesh_records: Vec::with_capacity(instance_capacity),
        mesh_records_buf,
        mesh_record_capacity: instance_capacity,
        textures: Slots::new(),
        materials: Slots::new(),
        texture_family,
        material_sampler,
        default_material_textures,
        scene: crate::scene::RenderScene::with_capacity(INITIAL_SCENE_CAPACITY),
        scene_dirty_ranges: Vec::with_capacity(INITIAL_SCENE_CAPACITY),
        previous_dirty_ranges: Vec::with_capacity(INITIAL_SCENE_CAPACITY),
        buckets: Vec::new(),
        last_lods: Vec::with_capacity(INITIAL_SCENE_CAPACITY),
        index_staging: Vec::with_capacity(index_capacity),
        instance_capacity,
        index_capacity,
        index_buf,
        scene_bg,
        view_bg,
        shadow_array_view: shadow_arr,
        shadow_sampler: cmp,
        white_ao_view: white_ao,
        globals_bgl: layouts.globals_bgl.clone(),
        globals_taa_bgl: layouts.globals_taa_bgl.clone(),
        previous_inst_buf,
        inst_buf,
        shadow_staging: [
            Vec::with_capacity(INITIAL_SCENE_CAPACITY),
            Vec::with_capacity(INITIAL_SCENE_CAPACITY),
        ],
        cascade_mats: [Mat4::IDENTITY; 2],
        cascade_planes: [[Vec4::ZERO; 6]; 2],
        far_shadow_cache: Vec::with_capacity(INITIAL_SCENE_CAPACITY),
        far_shadow_matrix: Mat4::IDENTITY,
        far_shadow_valid: false,
        far_shadow_dirty: true,
        #[cfg(test)]
        target_rebuilds: 1,
        particles: [
            Vec::with_capacity(MAX_PARTICLES),
            Vec::with_capacity(MAX_PARTICLES),
        ],
        part_buf,
        lights: Vec::with_capacity(MAX_LIGHTS),
        globals_buf,
        globals_bg,
        globals_taa_bg,
        shadow_u,
        shadow_ubg,
        shadow_layers,
        post_buf,
        linear,
        targets,
        ui_buf,
        ui_u,
        ui_bg,
        atlas_tex,
        p_shadow,
        tiles,
        p_scene: scene.p_scene,
        p_base_colour: scene.p_base_colour,
        p_lighting_only: scene.p_lighting_only,
        p_vm: scene.p_vm,
        p_base_colour_vm: scene.p_base_colour_vm,
        p_lighting_only_vm: scene.p_lighting_only_vm,
        p_coverage: scene.p_coverage,
        p_coverage_vm: scene.p_coverage_vm,
        p_sky: scene.p_sky,
        p_glow: scene.p_glow,
        p_part_alpha: scene.p_part_alpha,
        p_part_add: scene.p_part_add,
        p_down_first: post.p_down_first,
        p_down: post.p_down,
        p_up: post.p_up,
        p_comp: post.p_comp,
        p_debug_hdr: debug.p_hdr,
        p_debug_ao: debug.p_ao,
        p_debug_depth: debug.p_depth,
        p_debug_coverage: debug.p_coverage,
        p_debug_shadow: debug.p_shadow,
        debug_bindings,
        p_ui,
        cam: Camera::default(),
        view_proj: Mat4::IDENTITY,
        jit_view_proj: Mat4::IDENTITY,
        prev_view_proj: Mat4::IDENTITY,
        jitter: Jitter::new(),
        history_valid: false,
        history_scale: settings.render_scale,
        taa_write: false,
        planes: [Vec4::ZERO; 6],
        env: Env::default(),
        post: Post::default(),
        ui,
        time: 0.0,
        last: FrameStats::default(),
        asset_upload_bytes_pending: 0,
        culled: 0,
        visible_main: 0,
        visible_shadow: [0; 2],
        timer: GpuTimer::new(&gpu.device, &gpu.queue),
        screen,
        exposure_neutral,
        layouts,
        scene_sm,
        post_sm,
        gtao_sm,
        denoise_sm,
        taa_sm,
        #[cfg(any(feature = "hot-reload", test))]
        shader_library,
        pipe_ctx,
        #[cfg(any(feature = "hot-reload", test))]
        shadow_sm,
        #[cfg(any(feature = "hot-reload", test))]
        ui_sm,
        #[cfg(any(feature = "hot-reload", test))]
        tiles_sm,
        #[cfg(feature = "hot-reload")]
        shader_watcher,
        #[cfg(feature = "hot-reload")]
        hot_reload_clock: std::time::Instant::now(),
    })
}

fn default_material_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    rgba: [u8; 4],
    srgb: bool,
) -> GpuDefaultTexture {
    let format = if srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&Default::default());
    GpuDefaultTexture {
        _texture: texture,
        view,
    }
}
