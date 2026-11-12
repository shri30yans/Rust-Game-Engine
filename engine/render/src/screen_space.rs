//! Prepass buffers (normal, motion), screen-space ambient occlusion, auto-exposure, and the
//! TAA resolve pipeline. Pure data flow: the prepass writes normals, depth and motion; GTAO
//! and TAA read them; history textures and their bind groups live in `targets`.

use crate::scale_px_ceil;
use crate::tier::RenderFeatures;
use bytemuck::{Pod, Zeroable};
use glam::Mat4;

const NORMAL: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const VELOCITY: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
const AO: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
const EXP: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GtaoUniforms {
    inv_view_proj: [[f32; 4]; 4],
    texel: [f32; 4],
    params: [f32; 4],
    viewport: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DenoiseUniforms {
    texel: [f32; 4],
    viewport: [f32; 4],
}

/// GPU textures and pipelines for optional screen-space passes.
pub(crate) struct ScreenSpace {
    pub features: RenderFeatures,
    pub ao_size: (u32, u32),
    pub normal_view: wgpu::TextureView,
    pub ao_raw_view: wgpu::TextureView,
    pub ao_final_view: wgpu::TextureView,
    pub depth_render_view: wgpu::TextureView,
    pub exposure_view: wgpu::TextureView,
    /// Per-pixel motion in uv units (current minus previous), written by the prepass.
    pub velocity_view: wgpu::TextureView,
    /// Depth as a sampled texture (the prepass renders to `depth_render_view`).
    pub depth_sample_view: wgpu::TextureView,
    /// TAA resolve pipeline and layout, present when TAA is enabled.
    pub taa: Option<TaaPipeline>,
    #[cfg(any(feature = "hot-reload", test))]
    prepass_layout: wgpu::PipelineLayout,
    #[cfg(any(feature = "hot-reload", test))]
    gtao_layout: wgpu::PipelineLayout,
    #[cfg(any(feature = "hot-reload", test))]
    denoise_layout: wgpu::PipelineLayout,
    #[cfg(any(feature = "hot-reload", test))]
    exposure_layout: wgpu::PipelineLayout,
    pub p_prepass: [wgpu::RenderPipeline; 3],
    pub p_gtao: wgpu::RenderPipeline,
    pub p_denoise: wgpu::RenderPipeline,
    pub p_exposure: wgpu::RenderPipeline,
    pub gtao_uni: wgpu::Buffer,
    pub denoise_uni: wgpu::Buffer,
    pub gtao_bg: wgpu::BindGroup,
    pub denoise_bg: wgpu::BindGroup,
}

/// Clear value and active viewport for a full-screen pass.
pub(crate) struct FullscreenOptions {
    pub clear: wgpu::Color,
    pub viewport: Option<(u32, u32)>,
}

/// The TAA resolve pass: its layout (globals plus the TAA inputs) and pipeline.
pub(crate) struct TaaPipeline {
    /// Inputs: current HDR, history, velocity and depth.
    pub bgl: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
    #[cfg(any(feature = "hot-reload", test))]
    pub format: wgpu::TextureFormat,
    #[cfg(any(feature = "hot-reload", test))]
    layout: wgpu::PipelineLayout,
}

fn tex(
    device: &wgpu::Device,
    label: &str,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    #[cfg(test)]
    crate::gfx::setup::record_texture_creation();
    let view = texture.create_view(&Default::default());
    #[cfg(test)]
    crate::gfx::setup::record_view_creation();
    view
}

fn gtao_bgl(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("gtao"),
        entries: &[
            entry_uniform(0),
            entry_depth(1),
            entry_sampler(2, false),
            entry_tex(3),
            entry_sampler(4, true),
        ],
    })
}

fn denoise_bgl(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("gtao-denoise"),
        entries: &[
            entry_uniform(0),
            entry_tex(1),
            entry_depth(2),
            entry_tex(3),
            entry_sampler(4, true),
        ],
    })
}

fn entry_uniform(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn entry_tex(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn entry_depth(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn entry_sampler(binding: u32, filter: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(if filter {
            wgpu::SamplerBindingType::Filtering
        } else {
            wgpu::SamplerBindingType::NonFiltering
        }),
        count: None,
    }
}

fn taa_bgl(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("taa"),
        entries: &[
            entry_tex(0),
            entry_tex(1),
            entry_tex(2),
            entry_depth(3),
            entry_sampler(4, true),
        ],
    })
}

/// Inputs for allocating screen-space textures and pipelines.
pub(crate) struct ScreenSpaceDesc<'a> {
    pub device: &'a wgpu::Device,
    pub features: RenderFeatures,
    pub scene_shader: &'a wgpu::ShaderModule,
    pub post_shader: &'a wgpu::ShaderModule,
    pub gtao_shader: &'a wgpu::ShaderModule,
    pub denoise_shader: &'a wgpu::ShaderModule,
    pub globals_taa_bgl: &'a wgpu::BindGroupLayout,
    pub prepass_layout: &'a wgpu::PipelineLayout,
    pub post_bgl: &'a wgpu::BindGroupLayout,
    pub linear: &'a wgpu::Sampler,
    pub window_size: (u32, u32),
    /// Build the TAA resolve pipeline (requires the prepass, which this struct implies).
    pub taa: bool,
    pub taa_history_format: wgpu::TextureFormat,
    pub taa_shader: &'a wgpu::ShaderModule,
}

pub(crate) fn create(desc: &ScreenSpaceDesc<'_>) -> ScreenSpace {
    let ScreenSpaceDesc {
        device,
        features,
        scene_shader,
        post_shader,
        gtao_shader,
        denoise_shader,
        globals_taa_bgl,
        prepass_layout,
        post_bgl,
        linear,
        window_size,
        taa,
        taa_history_format,
        taa_shader,
    } = *desc;
    let sw = window_size.0.max(1);
    let sh = window_size.1.max(1);
    let s = features.gtao_resolution_scale();
    let ao_w = scale_px_ceil(sw, if features.gtao() { s } else { 1.0 }).max(1);
    let ao_h = scale_px_ceil(sh, if features.gtao() { s } else { 1.0 }).max(1);

    let normal_view = tex(device, "normal", sw, sh, NORMAL);
    let ao_raw_view = tex(device, "ao-raw", ao_w, ao_h, AO);
    let ao_final_view = tex(device, "ao-final", ao_w, ao_h, AO);
    let exposure_view = tex(device, "exposure", 1, 1, EXP);

    let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width: sw,
            height: sh,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    #[cfg(test)]
    crate::gfx::setup::record_texture_creation();
    let depth_render_view = depth_tex.create_view(&Default::default());
    #[cfg(test)]
    crate::gfx::setup::record_view_creation();
    let depth_sample_view = depth_tex.create_view(&wgpu::TextureViewDescriptor {
        aspect: wgpu::TextureAspect::DepthOnly,
        ..Default::default()
    });
    #[cfg(test)]
    crate::gfx::setup::record_view_creation();

    let velocity_view = tex(device, "velocity", sw, sh, VELOCITY);

    let depth_samp = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("depth-near"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    let gtao_layout = gtao_bgl(device);
    let denoise_layout = denoise_bgl(device);
    let p_prepass = [
        prepass_pipeline(device, prepass_layout, scene_shader, features.taa(), 0),
        prepass_pipeline(device, prepass_layout, scene_shader, features.taa(), 1),
        prepass_pipeline(device, prepass_layout, scene_shader, features.taa(), 2),
    ];
    let taa_pipeline = taa.then(|| {
        let bgl = taa_bgl(device);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("taa"),
            bind_group_layouts: &[Some(globals_taa_bgl), Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = fullscreen_pipe(device, &layout, taa_shader, "fs_taa", taa_history_format);
        TaaPipeline {
            bgl,
            pipeline,
            #[cfg(any(feature = "hot-reload", test))]
            format: taa_history_format,
            #[cfg(any(feature = "hot-reload", test))]
            layout,
        }
    });
    let gtao_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gtao"),
        bind_group_layouts: &[Some(&gtao_layout)],
        immediate_size: 0,
    });
    let p_gtao = fullscreen_pipe(device, &gtao_pl, gtao_shader, "fs_gtao", AO);
    let denoise_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gtao-denoise"),
        bind_group_layouts: &[Some(&denoise_layout)],
        immediate_size: 0,
    });
    let p_denoise = fullscreen_pipe(device, &denoise_pl, denoise_shader, "fs_denoise", AO);
    let exposure_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("exposure"),
        bind_group_layouts: &[Some(post_bgl)],
        immediate_size: 0,
    });
    let p_exposure = fullscreen_pipe(device, &exposure_layout, post_shader, "fs_exposure", EXP);

    let uni = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
    let gtao_uni = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gtao-uni"),
        size: std::mem::size_of::<GtaoUniforms>() as u64,
        usage: uni,
        mapped_at_creation: false,
    });
    let denoise_uni = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("denoise-uni"),
        size: std::mem::size_of::<DenoiseUniforms>() as u64,
        usage: uni,
        mapped_at_creation: false,
    });

    let gtao_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gtao"),
        layout: &gtao_layout,
        entries: &[
            bg_buf(0, &gtao_uni),
            bg_tex(1, &depth_sample_view),
            bg_samp(2, &depth_samp),
            bg_tex(3, &normal_view),
            bg_samp(4, linear),
        ],
    });
    #[cfg(test)]
    crate::gfx::setup::record_bind_group_creation();
    let denoise_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("denoise"),
        layout: &denoise_layout,
        entries: &[
            bg_buf(0, &denoise_uni),
            bg_tex(1, &ao_raw_view),
            bg_tex(2, &depth_sample_view),
            bg_tex(3, &normal_view),
            bg_samp(4, linear),
        ],
    });
    #[cfg(test)]
    crate::gfx::setup::record_bind_group_creation();

    ScreenSpace {
        features,
        ao_size: (ao_w, ao_h),
        normal_view,
        ao_raw_view,
        ao_final_view,
        depth_render_view,
        exposure_view,
        velocity_view,
        depth_sample_view,
        taa: taa_pipeline,
        #[cfg(any(feature = "hot-reload", test))]
        prepass_layout: prepass_layout.clone(),
        #[cfg(any(feature = "hot-reload", test))]
        gtao_layout: gtao_pl,
        #[cfg(any(feature = "hot-reload", test))]
        denoise_layout: denoise_pl,
        #[cfg(any(feature = "hot-reload", test))]
        exposure_layout,
        p_prepass,
        p_gtao,
        p_denoise,
        p_exposure,
        gtao_uni,
        denoise_uni,
        gtao_bg,
        denoise_bg,
    }
}

fn prepass_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    scene_shader: &wgpu::ShaderModule,
    taa: bool,
    format: usize,
) -> wgpu::RenderPipeline {
    let bufs = if format == 0 {
        crate::gfx::pipelines::shadow_bufs()
    } else {
        crate::gfx::pipelines::cooked_shadow_bufs(format == 2)
    };
    let entry_point = match (format, taa) {
        (0, false) => "vs_prepass",
        (0, true) => "vs_prepass_taa",
        (1, false) => "vs_prepass_cooked",
        (1, true) => "vs_prepass_taa_cooked",
        (_, false) => "vs_prepass_quantized",
        (_, true) => "vs_prepass_taa_quantized",
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("prepass"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: scene_shader,
            entry_point: Some(entry_point),
            compilation_options: Default::default(),
            buffers: &bufs,
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: scene_shader,
            entry_point: Some("fs_prepass"),
            compilation_options: Default::default(),
            targets: &[
                Some(wgpu::ColorTargetState {
                    format: NORMAL,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: VELOCITY,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
            ],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(any(feature = "hot-reload", test))]
impl ScreenSpace {
    pub(crate) fn rebuild_taa(&mut self, device: &wgpu::Device, taa_shader: &wgpu::ShaderModule) {
        if let Some(t) = self.taa.as_mut() {
            t.pipeline = fullscreen_pipe(device, &t.layout, taa_shader, "fs_taa", t.format);
        }
    }

    pub(crate) fn rebuild_scene(
        &mut self,
        device: &wgpu::Device,
        scene_shader: &wgpu::ShaderModule,
    ) {
        self.p_prepass = std::array::from_fn(|format| {
            prepass_pipeline(
                device,
                &self.prepass_layout,
                scene_shader,
                self.features.taa(),
                format,
            )
        });
    }

    pub(crate) fn rebuild_post(&mut self, device: &wgpu::Device, post_shader: &wgpu::ShaderModule) {
        self.p_exposure = fullscreen_pipe(
            device,
            &self.exposure_layout,
            post_shader,
            "fs_exposure",
            EXP,
        );
    }

    pub(crate) fn rebuild_gtao(&mut self, device: &wgpu::Device, gtao_shader: &wgpu::ShaderModule) {
        self.p_gtao = fullscreen_pipe(device, &self.gtao_layout, gtao_shader, "fs_gtao", AO);
    }

    pub(crate) fn rebuild_denoise(
        &mut self,
        device: &wgpu::Device,
        denoise_shader: &wgpu::ShaderModule,
    ) {
        self.p_denoise = fullscreen_pipe(
            device,
            &self.denoise_layout,
            denoise_shader,
            "fs_denoise",
            AO,
        );
    }
}

fn fullscreen_pipe(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fs: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_full"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn bg_buf(binding: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buf.as_entire_binding(),
    }
}

fn bg_tex(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

fn bg_samp(binding: u32, samp: &wgpu::Sampler) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::Sampler(samp),
    }
}

impl ScreenSpace {
    pub fn write_gtao_uniforms(&self, queue: &wgpu::Queue, inv_view_proj: Mat4, render_scale: f32) {
        let (w, h) = self.ao_size;
        let u = GtaoUniforms {
            inv_view_proj: inv_view_proj.to_cols_array_2d(),
            texel: [1.0 / w as f32, 1.0 / h as f32, 2.5, 0.0],
            params: [12.0, 4.0, 1.25, 0.01],
            viewport: [render_scale, render_scale, 0.0, 0.0],
        };
        queue.write_buffer(&self.gtao_uni, 0, bytemuck::bytes_of(&u));
        let d = DenoiseUniforms {
            texel: [1.0 / w as f32, 1.0 / h as f32, 0.02, 32.0],
            viewport: [render_scale, render_scale, 0.0, 0.0],
        };
        queue.write_buffer(&self.denoise_uni, 0, bytemuck::bytes_of(&d));
    }

    /// Full-screen pass that binds one group at index 0.
    pub fn fullscreen(
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        pipeline: &wgpu::RenderPipeline,
        bg: &wgpu::BindGroup,
        view: &wgpu::TextureView,
        options: FullscreenOptions,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        Self::fullscreen_with(
            encoder,
            label,
            pipeline,
            &[(0, bg)],
            view,
            options,
            timestamp_writes,
        );
    }

    /// Full-screen pass that binds each `(index, group)` pair before drawing. Used by passes
    /// whose pipeline layout has more than one group (TAA: globals at 0, inputs at 1).
    pub fn fullscreen_with(
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        pipeline: &wgpu::RenderPipeline,
        groups: &[(u32, &wgpu::BindGroup)],
        view: &wgpu::TextureView,
        options: FullscreenOptions,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(options.clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some((width, height)) = options.viewport {
            pass.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
            pass.set_scissor_rect(0, 0, width, height);
        }
        pass.set_pipeline(pipeline);
        for &(index, group) in groups {
            pass.set_bind_group(index, group, &[]);
        }
        pass.draw(0..3, 0..1);
    }
}
