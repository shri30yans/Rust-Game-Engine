//! Pipeline layouts and render-pipeline builders used by [`super::Gfx`].

mod layouts;
pub(super) mod scene;

pub(crate) use layouts::PipelineLayouts;
pub(crate) use scene::create_scene_pipelines;

use crate::frame_plan::OpaqueDepth;
use crate::mesh::Vertex;
use crate::ui::UiVertex;

pub(crate) const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub(crate) const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Per-build rendering state that does not change when shaders are hot-reloaded.
pub(crate) struct GfxPipeCtx {
    pub format: wgpu::TextureFormat,
    pub msaa: wgpu::MultisampleState,
    pub taa: bool,
    pub opaque_depth: OpaqueDepth,
}

pub(crate) struct PostPipelines {
    pub p_down_first: wgpu::RenderPipeline,
    pub p_down: wgpu::RenderPipeline,
    pub p_up: wgpu::RenderPipeline,
    pub p_comp: wgpu::RenderPipeline,
}

/// Dedicated terminal presentation pipelines for renderer buffer inspection.
pub(crate) struct DebugPipelines {
    pub p_hdr: wgpu::RenderPipeline,
    pub p_ao: wgpu::RenderPipeline,
    pub p_depth: wgpu::RenderPipeline,
    pub p_coverage: wgpu::RenderPipeline,
    pub p_shadow: wgpu::RenderPipeline,
}

const PA: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];
const COOKED_PA: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];
const QUANTIZED_PA: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Uint16x4];
const UA: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4, 5 => Float32x4];

pub(crate) fn shadow_bufs() -> [Option<wgpu::VertexBufferLayout<'static>>; 1] {
    [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &PA,
    })]
}

pub(crate) fn cooked_shadow_bufs(
    quantized: bool,
) -> [Option<wgpu::VertexBufferLayout<'static>>; 1] {
    [Some(wgpu::VertexBufferLayout {
        array_stride: if quantized { 8 } else { 12 },
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: if quantized { &QUANTIZED_PA } else { &COOKED_PA },
    })]
}

pub(crate) fn ui_bufs() -> [Option<wgpu::VertexBufferLayout<'static>>; 1] {
    [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<UiVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &UA,
    })]
}

pub(crate) struct PipelineDesc<'a> {
    pub label: &'a str,
    pub layout: &'a wgpu::PipelineLayout,
    pub sm: &'a wgpu::ShaderModule,
    pub vs: &'a str,
    pub fs: Option<&'a str>,
    pub bufs: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub prim: wgpu::PrimitiveState,
    pub depth: Option<wgpu::DepthStencilState>,
    pub ms: wgpu::MultisampleState,
    pub targets: &'a [Option<wgpu::ColorTargetState>],
}

fn pipe(device: &wgpu::Device, d: &PipelineDesc<'_>) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(d.label),
        layout: Some(d.layout),
        vertex: wgpu::VertexState {
            module: d.sm,
            entry_point: Some(d.vs),
            compilation_options: Default::default(),
            buffers: d.bufs,
        },
        primitive: d.prim,
        depth_stencil: d.depth.clone(),
        multisample: d.ms,
        fragment: d.fs.map(|f| wgpu::FragmentState {
            module: d.sm,
            entry_point: Some(f),
            compilation_options: Default::default(),
            targets: d.targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

pub(crate) fn create_shadow_pipeline(
    device: &wgpu::Device,
    layouts: &PipelineLayouts,
    sm: &wgpu::ShaderModule,
) -> [wgpu::RenderPipeline; 3] {
    use wgpu::CompareFunction as C;
    let back = wgpu::PrimitiveState {
        cull_mode: Some(wgpu::Face::Back),
        ..Default::default()
    };
    let make = |label, vs, buffers| {
        pipe(
            device,
            &PipelineDesc {
                label,
                layout: &layouts.shadow_layout,
                sm,
                vs,
                fs: None,
                bufs: buffers,
                prim: back,
                depth: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(C::LessEqual),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 2.5,
                        clamp: 0.0,
                    },
                }),
                ms: Default::default(),
                targets: &[],
            },
        )
    };
    let procedural = shadow_bufs();
    let cooked_float = cooked_shadow_bufs(false);
    let cooked_quantized = cooked_shadow_bufs(true);
    [
        make("shadow-procedural", "vs_shadow", &procedural),
        make("shadow-cooked-float", "vs_shadow_cooked", &cooked_float),
        make(
            "shadow-cooked-quantized",
            "vs_shadow_quantized",
            &cooked_quantized,
        ),
    ]
}

pub(crate) fn create_post_pipelines(
    device: &wgpu::Device,
    layouts: &PipelineLayouts,
    sm: &wgpu::ShaderModule,
    ctx: &GfxPipeCtx,
) -> PostPipelines {
    let none = wgpu::PrimitiveState::default();
    let one = wgpu::MultisampleState::default();
    let add = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    };
    let hdr_t = |blend: Option<wgpu::BlendState>| {
        [Some(wgpu::ColorTargetState {
            format: HDR,
            blend,
            write_mask: wgpu::ColorWrites::ALL,
        })]
    };
    let t_swap = [Some(wgpu::ColorTargetState {
        format: ctx.format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let fullscreen = |label: &str, fs: &str, targets: &[Option<wgpu::ColorTargetState>]| {
        pipe(
            device,
            &PipelineDesc {
                label,
                layout: &layouts.post_layout,
                sm,
                vs: "vs_full",
                fs: Some(fs),
                bufs: &[],
                prim: none,
                depth: None,
                ms: one,
                targets,
            },
        )
    };
    let down0_t = hdr_t(None);
    let down_t = hdr_t(None);
    let up_t = hdr_t(Some(add));
    PostPipelines {
        p_down_first: fullscreen("bloom-down0", "fs_down_first", &down0_t),
        p_down: fullscreen("bloom-down", "fs_down", &down_t),
        p_up: fullscreen("bloom-up", "fs_up", &up_t),
        p_comp: fullscreen("composite", "fs_composite", &t_swap),
    }
}

/// Builds the dedicated terminal pipelines used to inspect renderer inputs without using a
/// shader-wide mode branch.
pub(crate) fn create_debug_pipelines(
    device: &wgpu::Device,
    layouts: &PipelineLayouts,
    sm: &wgpu::ShaderModule,
    ctx: &GfxPipeCtx,
) -> DebugPipelines {
    let target = [Some(wgpu::ColorTargetState {
        format: ctx.format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let fullscreen = |label: &str, layout: &wgpu::PipelineLayout, fs: &str| {
        pipe(
            device,
            &PipelineDesc {
                label,
                layout,
                sm,
                vs: "vs_full",
                fs: Some(fs),
                bufs: &[],
                prim: wgpu::PrimitiveState::default(),
                depth: None,
                ms: wgpu::MultisampleState::default(),
                targets: &target,
            },
        )
    };
    DebugPipelines {
        p_hdr: fullscreen("debug-hdr", &layouts.debug_color_layout, "fs_view_hdr"),
        p_ao: fullscreen("debug-ao", &layouts.debug_color_layout, "fs_view_ao"),
        p_depth: fullscreen("debug-depth", &layouts.debug_depth_layout, "fs_view_depth"),
        p_coverage: fullscreen(
            "debug-opaque-coverage",
            &layouts.debug_coverage_layout,
            "fs_view_opaque_coverage",
        ),
        p_shadow: fullscreen(
            "debug-shadow",
            &layouts.debug_shadow_layout,
            "fs_view_shadow",
        ),
    }
}

pub(crate) fn create_ui_pipeline(
    device: &wgpu::Device,
    layouts: &PipelineLayouts,
    sm: &wgpu::ShaderModule,
    ctx: &GfxPipeCtx,
) -> wgpu::RenderPipeline {
    let none = wgpu::PrimitiveState::default();
    let one = wgpu::MultisampleState::default();
    let premul = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
    let t_swap_ui = [Some(wgpu::ColorTargetState {
        format: ctx.format,
        blend: Some(premul),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    pipe(
        device,
        &PipelineDesc {
            label: "ui",
            layout: &layouts.ui_layout,
            sm,
            vs: "vs_ui",
            fs: Some("fs_ui"),
            bufs: &ui_bufs(),
            prim: none,
            depth: None,
            ms: one,
            targets: &t_swap_ui,
        },
    )
}
