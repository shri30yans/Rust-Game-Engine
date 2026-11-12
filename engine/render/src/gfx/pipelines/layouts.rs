//! Bind-group layouts and pipeline layouts for renderer passes.

/// Bind-group and pipeline layouts that outlive individual shader modules.
pub(crate) struct PipelineLayouts {
    pub globals_bgl: wgpu::BindGroupLayout,
    pub globals_taa_bgl: wgpu::BindGroupLayout,
    pub shadow_u_bgl: wgpu::BindGroupLayout,
    pub post_bgl: wgpu::BindGroupLayout,
    pub debug_color_bgl: wgpu::BindGroupLayout,
    pub debug_depth_bgl: wgpu::BindGroupLayout,
    pub debug_coverage_bgl: wgpu::BindGroupLayout,
    pub debug_shadow_bgl: wgpu::BindGroupLayout,
    pub scene_instances_bgl: wgpu::BindGroupLayout,
    pub ui_bgl: wgpu::BindGroupLayout,
    pub tiles_write_bgl: wgpu::BindGroupLayout,
    pub view_bgl: wgpu::BindGroupLayout,
    pub material_bgl: wgpu::BindGroupLayout,
    pub scene_layout: wgpu::PipelineLayout,
    pub scene_taa_layout: wgpu::PipelineLayout,
    pub textured_scene_layout: wgpu::PipelineLayout,
    pub textured_scene_taa_layout: wgpu::PipelineLayout,
    pub shadow_layout: wgpu::PipelineLayout,
    pub post_layout: wgpu::PipelineLayout,
    pub debug_color_layout: wgpu::PipelineLayout,
    pub debug_depth_layout: wgpu::PipelineLayout,
    pub debug_coverage_layout: wgpu::PipelineLayout,
    pub debug_shadow_layout: wgpu::PipelineLayout,
    pub ui_layout: wgpu::PipelineLayout,
}

fn bgl(binding: u32, vis: wgpu::ShaderStages, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty,
        count: None,
    }
}

fn uniform_ty() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn tex_ty() -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    }
}

impl PipelineLayouts {
    pub(crate) fn new(device: &wgpu::Device, tiles_write_bgl: wgpu::BindGroupLayout) -> Self {
        let frag = wgpu::ShaderStages::FRAGMENT;
        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let storage_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let storage_entry_fragment = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: frag,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let globals_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals-and-scene"),
            entries: &[
                bgl(0, vf | wgpu::ShaderStages::COMPUTE, uniform_ty()),
                storage_entry(1),
                storage_entry(2),
            ],
        });
        let globals_taa_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals-scene-taa"),
            entries: &[
                bgl(0, vf, uniform_ty()),
                storage_entry(1),
                storage_entry(2),
                storage_entry(3),
            ],
        });
        let shadow_u_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow-u"),
            entries: &[bgl(0, wgpu::ShaderStages::VERTEX, uniform_ty())],
        });
        let post_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                bgl(0, frag, tex_ty()),
                bgl(
                    1,
                    frag,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                bgl(2, frag, uniform_ty()),
                bgl(3, frag, tex_ty()),
                bgl(4, frag, tex_ty()),
            ],
        });
        let debug_color_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("debug-color"),
            entries: &[bgl(5, frag, tex_ty())],
        });
        let debug_depth_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("debug-depth"),
            entries: &[bgl(
                6,
                frag,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            )],
        });
        let debug_coverage_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("debug-coverage"),
                entries: &[
                    bgl(5, frag, tex_ty()),
                    bgl(
                        6,
                        frag,
                        wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                    ),
                ],
            });
        let debug_shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("debug-shadow"),
            entries: &[bgl(
                7,
                frag,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
            )],
        });
        let scene_instances_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("scene-instances"),
                entries: &[
                    storage_entry(0),
                    storage_entry(1),
                    storage_entry(2),
                    storage_entry(3),
                ],
            });
        let view_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene-view"),
            entries: &[
                bgl(0, vf, uniform_ty()),
                storage_entry(1),
                storage_entry(2),
                storage_entry(3),
                storage_entry(4),
                storage_entry_fragment(5),
                bgl(
                    6,
                    frag,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                bgl(
                    7,
                    frag,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                ),
                bgl(8, frag, tex_ty()),
                bgl(
                    9,
                    frag,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let material_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene-material"),
            entries: &[
                bgl(0, frag, uniform_ty()),
                bgl(1, frag, tex_ty()),
                bgl(2, frag, tex_ty()),
                bgl(3, frag, tex_ty()),
                bgl(4, frag, tex_ty()),
                bgl(
                    5,
                    frag,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let ui_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui"),
            entries: &[
                bgl(0, frag, tex_ty()),
                bgl(
                    1,
                    frag,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                bgl(2, frag, uniform_ty()),
            ],
        });

        let mk_pl = |label: &str, bgls: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: bgls,
                immediate_size: 0,
            })
        };
        // Group 2 is reserved for the lighting environment probes.
        let scene_layout = mk_pl("scene", &[Some(&view_bgl)]);
        let scene_taa_layout = mk_pl("scene-taa", &[Some(&view_bgl)]);
        let textured_scene_layout =
            mk_pl("textured-scene", &[Some(&view_bgl), Some(&material_bgl)]);
        let textured_scene_taa_layout = mk_pl(
            "textured-scene-taa",
            &[Some(&view_bgl), Some(&material_bgl)],
        );
        let shadow_layout = mk_pl("shadow", &[Some(&shadow_u_bgl), Some(&scene_instances_bgl)]);
        let post_layout = mk_pl("post", &[Some(&post_bgl)]);
        let debug_color_layout = mk_pl("debug-color", &[Some(&debug_color_bgl)]);
        let debug_depth_layout = mk_pl("debug-depth", &[Some(&debug_depth_bgl)]);
        let debug_coverage_layout = mk_pl("debug-coverage", &[Some(&debug_coverage_bgl)]);
        let debug_shadow_layout = mk_pl("debug-shadow", &[Some(&debug_shadow_bgl)]);
        let ui_layout = mk_pl("ui", &[Some(&ui_bgl)]);

        Self {
            globals_bgl,
            globals_taa_bgl,
            shadow_u_bgl,
            post_bgl,
            debug_color_bgl,
            debug_depth_bgl,
            debug_coverage_bgl,
            debug_shadow_bgl,
            scene_instances_bgl,
            ui_bgl,
            tiles_write_bgl,
            view_bgl,
            material_bgl,
            scene_layout,
            scene_taa_layout,
            textured_scene_layout,
            textured_scene_taa_layout,
            shadow_layout,
            post_layout,
            debug_color_layout,
            debug_depth_layout,
            debug_coverage_layout,
            debug_shadow_layout,
            ui_layout,
        }
    }
}
