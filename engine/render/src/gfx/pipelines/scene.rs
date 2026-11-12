//! Opaque, viewmodel, glow, sky, and particle scene pipelines.

use super::{DEPTH, GfxPipeCtx, HDR, PipelineDesc, PipelineLayouts, pipe};
use crate::frame_plan::OpaqueDrawGroup;
use crate::mesh::Vertex;

const VA: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![1 => Snorm16x2, 2 => Unorm8x4, 3 => Unorm8x4];
const COOKED_VA: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![1 => Snorm16x2, 2 => Uint32, 3 => Uint16x2];
const PARTICLE_ATTR: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];

pub(crate) struct ScenePipelines {
    pub p_scene: [[wgpu::RenderPipeline; 3]; 2],
    pub p_base_colour: [[wgpu::RenderPipeline; 3]; 2],
    pub p_lighting_only: [[wgpu::RenderPipeline; 3]; 2],
    pub p_vm: [[wgpu::RenderPipeline; 3]; 2],
    pub p_base_colour_vm: [[wgpu::RenderPipeline; 3]; 2],
    pub p_lighting_only_vm: [[wgpu::RenderPipeline; 3]; 2],
    pub p_coverage: [[wgpu::RenderPipeline; 3]; 2],
    pub p_coverage_vm: [[wgpu::RenderPipeline; 3]; 2],
    pub p_sky: wgpu::RenderPipeline,
    pub p_glow: [[wgpu::RenderPipeline; 3]; 2],
    pub p_part_alpha: wgpu::RenderPipeline,
    pub p_part_add: wgpu::RenderPipeline,
}

fn mesh_bufs() -> [Option<wgpu::VertexBufferLayout<'static>>; 2] {
    [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &super::PA,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::VertexAttributes>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VA,
        }),
    ]
}

fn cooked_bufs(quantized: bool) -> [Option<wgpu::VertexBufferLayout<'static>>; 2] {
    [
        Some(wgpu::VertexBufferLayout {
            array_stride: if quantized { 8 } else { 12 },
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: if quantized {
                &super::QUANTIZED_PA
            } else {
                &super::COOKED_PA
            },
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &COOKED_VA,
        }),
    ]
}

fn part_bufs() -> [Option<wgpu::VertexBufferLayout<'static>>; 1] {
    [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<crate::Particle>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &PARTICLE_ATTR,
    })]
}

pub(crate) fn create_scene_pipelines(
    device: &wgpu::Device,
    layouts: &PipelineLayouts,
    sm: &wgpu::ShaderModule,
    ctx: &GfxPipeCtx,
) -> ScenePipelines {
    let back = wgpu::PrimitiveState {
        cull_mode: Some(wgpu::Face::Back),
        ..Default::default()
    };
    let none = wgpu::PrimitiveState::default();
    let nb = wgpu::DepthBiasState::default();
    let mesh = mesh_bufs();
    let cooked_float = cooked_bufs(false);
    let cooked_quantized = cooked_bufs(true);
    let part = part_bufs();
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
    let premul = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
    let sky_depth = ctx.opaque_depth.state(OpaqueDrawGroup::Sky);
    let particle_depth = ctx.opaque_depth.state(OpaqueDrawGroup::Particles);
    let scene_layout = if ctx.taa {
        &layouts.scene_taa_layout
    } else {
        &layouts.scene_layout
    };
    let hdr_t = |blend: Option<wgpu::BlendState>| {
        [Some(wgpu::ColorTargetState {
            format: HDR,
            blend,
            write_mask: wgpu::ColorWrites::ALL,
        })]
    };
    let scene = |label, layout, vs, fs, bufs, prim, blend, depth_cmp, depth_write| {
        pipe(
            device,
            &PipelineDesc {
                label,
                layout,
                sm,
                vs,
                fs: Some(fs),
                bufs,
                prim,
                depth: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(depth_cmp),
                    stencil: Default::default(),
                    bias: nb,
                }),
                ms: ctx.msaa,
                targets: &hdr_t(blend),
            },
        )
    };
    let mesh_pipelines = |pass: &str, vm: bool, glow: bool| {
        let depth_group = if glow {
            OpaqueDrawGroup::Glow
        } else if vm {
            OpaqueDrawGroup::Viewmodel
        } else {
            OpaqueDrawGroup::WorldOpaque
        };
        let depth = ctx.opaque_depth.state(depth_group);
        std::array::from_fn(|material_kind| {
            std::array::from_fn(|format| {
                let cooked = format != 0;
                let quantized = format == 2;
                let vs = match (cooked, quantized, vm) {
                    (false, _, false) => "vs_main",
                    (false, _, true) => "vs_vm",
                    (true, false, false) => "vs_cooked",
                    (true, true, false) => "vs_cooked_quantized",
                    (true, false, true) => "vs_vm_cooked",
                    (true, true, true) => "vs_vm_cooked_quantized",
                };
                let label = match (pass, format) {
                    ("scene", 0) => "scene-procedural",
                    ("scene", 1) => "scene-cooked-float",
                    ("scene", _) => "scene-cooked-quantized",
                    ("base-colour", 0) => "base-colour-procedural",
                    ("base-colour", 1) => "base-colour-cooked-float",
                    ("base-colour", _) => "base-colour-cooked-quantized",
                    ("lighting-only", 0) => "lighting-only-procedural",
                    ("lighting-only", 1) => "lighting-only-cooked-float",
                    ("lighting-only", _) => "lighting-only-cooked-quantized",
                    ("viewmodel", 0) => "viewmodel-procedural",
                    ("viewmodel", 1) => "viewmodel-cooked-float",
                    ("viewmodel", _) => "viewmodel-cooked-quantized",
                    ("base-colour-viewmodel", 0) => "base-colour-viewmodel-procedural",
                    ("base-colour-viewmodel", 1) => "base-colour-viewmodel-cooked-float",
                    ("base-colour-viewmodel", _) => "base-colour-viewmodel-cooked-quantized",
                    ("lighting-only-viewmodel", 0) => "lighting-only-viewmodel-procedural",
                    ("lighting-only-viewmodel", 1) => "lighting-only-viewmodel-cooked-float",
                    ("lighting-only-viewmodel", _) => "lighting-only-viewmodel-cooked-quantized",
                    (_, 0) => "glow-procedural",
                    (_, 1) => "glow-cooked-float",
                    _ => "glow-cooked-quantized",
                };
                let format_layout = match (ctx.taa, material_kind) {
                    (false, 0) => &layouts.scene_layout,
                    (true, 0) => &layouts.scene_taa_layout,
                    (false, _) => &layouts.textured_scene_layout,
                    (true, _) => &layouts.textured_scene_taa_layout,
                };
                let fragment = match (pass, material_kind) {
                    ("opaque-coverage", _) => "fs_coverage_marker",
                    ("glow", 0) => "fs_glow",
                    ("glow", _) => "fs_glow_textured",
                    ("base-colour", 0) | ("base-colour-viewmodel", 0) => "fs_base_colour",
                    ("base-colour", _) | ("base-colour-viewmodel", _) => "fs_base_colour_textured",
                    ("lighting-only", 0) | ("lighting-only-viewmodel", 0) => "fs_lighting_only",
                    ("lighting-only", _) | ("lighting-only-viewmodel", _) => {
                        "fs_lighting_only_textured"
                    }
                    (_, 0) => "fs_main",
                    (_, _) => "fs_textured",
                };
                if cooked {
                    let buffers = if quantized {
                        &cooked_quantized
                    } else {
                        &cooked_float
                    };
                    scene(
                        label,
                        format_layout,
                        vs,
                        fragment,
                        buffers,
                        if glow { none } else { back },
                        if glow { Some(add) } else { None },
                        depth.compare,
                        depth.write,
                    )
                } else {
                    scene(
                        label,
                        format_layout,
                        vs,
                        fragment,
                        &mesh,
                        if glow { none } else { back },
                        if glow { Some(add) } else { None },
                        depth.compare,
                        depth.write,
                    )
                }
            })
        })
    };
    ScenePipelines {
        p_scene: mesh_pipelines("scene", false, false),
        p_base_colour: mesh_pipelines("base-colour", false, false),
        p_lighting_only: mesh_pipelines("lighting-only", false, false),
        p_vm: mesh_pipelines("viewmodel", true, false),
        p_base_colour_vm: mesh_pipelines("base-colour-viewmodel", true, false),
        p_lighting_only_vm: mesh_pipelines("lighting-only-viewmodel", true, false),
        p_coverage: mesh_pipelines("opaque-coverage", false, false),
        p_coverage_vm: mesh_pipelines("opaque-coverage", true, false),
        p_sky: scene(
            "sky",
            scene_layout,
            "vs_sky",
            "fs_sky",
            &[],
            none,
            None,
            sky_depth.compare,
            sky_depth.write,
        ),
        p_glow: mesh_pipelines("glow", false, true),
        p_part_alpha: scene(
            "part-alpha",
            scene_layout,
            "vs_particle",
            "fs_particle",
            &part,
            none,
            Some(premul),
            particle_depth.compare,
            particle_depth.write,
        ),
        p_part_add: scene(
            "part-add",
            scene_layout,
            "vs_particle",
            "fs_particle",
            &part,
            none,
            Some(add),
            particle_depth.compare,
            particle_depth.write,
        ),
    }
}
