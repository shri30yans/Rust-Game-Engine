//! Temporal resolve, post-processing, terminal presentation, and UI encoding.

use super::{FrameStats, FullscreenOptions, Gfx, GpuPass, ScreenSpace, to_u32};

impl Gfx {
    /// Resolves HDR into the alternating history target and selects it for later post passes.
    pub(super) fn encode_taa(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        active_size: (u32, u32),
        post_slot: &mut bool,
    ) {
        let (Some(pipeline), Some(targets)) = (
            self.screen.as_ref().and_then(|screen| screen.taa.as_ref()),
            self.targets.taa.as_ref(),
        ) else {
            return;
        };
        let write_slot = self.taa_write;
        let timestamps = self
            .timer
            .as_mut()
            .and_then(|timer| timer.writes(GpuPass::Taa));
        ScreenSpace::fullscreen_with(
            encoder,
            "taa",
            &pipeline.pipeline,
            &[(0, &self.globals_taa_bg), (1, targets.bg.pick(write_slot))],
            targets.history.pick(write_slot),
            FullscreenOptions {
                clear: wgpu::Color::BLACK,
                viewport: Some(active_size),
            },
            timestamps,
        );
        *post_slot = write_slot;
        self.taa_write = !write_slot;
        self.history_valid = true;
    }

    /// Builds and combines the bloom pyramid from the selected scene-colour source.
    pub(super) fn encode_bloom(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        active_size: (u32, u32),
        source: crate::PostSource,
        post_slot: bool,
        stats: &mut FrameStats,
    ) {
        let targets = &self.targets;
        let post = post_source(targets, source, post_slot);
        let mut viewport = active_size;
        for (index, (destination, _)) in targets.bloom.iter().enumerate() {
            viewport = ((viewport.0 / 2).max(1), (viewport.1 / 2).max(1));
            let (pipeline, bind_group) = if index == 0 {
                (&self.p_down_first, Some(&post.down0))
            } else {
                (&self.p_down, targets.bg_down.get(index - 1))
            };
            let Some(bind_group) = bind_group else {
                continue;
            };
            let pass_id = match index {
                0 => GpuPass::BloomDown0,
                1 => GpuPass::BloomDown1,
                2 => GpuPass::BloomDown2,
                3 => GpuPass::BloomDown3,
                _ => GpuPass::BloomDown4,
            };
            let timestamps = self.timer.as_mut().and_then(|timer| timer.writes(pass_id));
            encode_bloom_fullscreen(
                encoder,
                BloomFullscreen {
                    destination,
                    pipeline,
                    bind_group,
                    clear: true,
                    timestamps,
                    viewport,
                },
                stats,
            );
        }
        for index in (1..crate::BLOOM_LEVELS).rev() {
            let (Some((destination, _)), Some(bind_group)) =
                (targets.bloom.get(index - 1), targets.bg_up.get(index))
            else {
                continue;
            };
            let pass_id = match index {
                1 => GpuPass::BloomUp1,
                2 => GpuPass::BloomUp2,
                3 => GpuPass::BloomUp3,
                _ => GpuPass::BloomUp4,
            };
            let timestamps = self.timer.as_mut().and_then(|timer| timer.writes(pass_id));
            let mut viewport = active_size;
            for _ in 0..index {
                viewport = ((viewport.0 / 2).max(1), (viewport.1 / 2).max(1));
            }
            encode_bloom_fullscreen(
                encoder,
                BloomFullscreen {
                    destination,
                    pipeline: &self.p_up,
                    bind_group,
                    clear: false,
                    timestamps,
                    viewport,
                },
                stats,
            );
        }
    }

    /// Measures automatic exposure from the selected scene-colour source.
    pub(super) fn encode_exposure(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        source: crate::PostSource,
        post_slot: bool,
    ) {
        let Some(screen) = self.screen.as_ref() else {
            return;
        };
        let post = post_source(&self.targets, source, post_slot);
        let timestamps = self
            .timer
            .as_mut()
            .and_then(|timer| timer.writes(GpuPass::Exposure));
        ScreenSpace::fullscreen(
            encoder,
            "auto-exposure",
            &screen.p_exposure,
            &post.measure,
            &screen.exposure_view,
            FullscreenOptions {
                clear: wgpu::Color::WHITE,
                viewport: None,
            },
            timestamps,
        );
    }

    /// Presents the terminal pass explicitly selected in the checked frame plan.
    pub(super) fn encode_terminal(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        terminal: crate::FramePass,
        post_slot: bool,
        stats: &mut FrameStats,
    ) {
        let post = match terminal {
            crate::FramePass::Composite(source) => post_source(&self.targets, source, post_slot),
            _ => post_source(&self.targets, crate::PostSource::Hdr, post_slot),
        };
        let (label, pipeline, bind_group) = match terminal {
            crate::FramePass::Composite(_) => ("composite", &self.p_comp, &post.comp),
            crate::FramePass::View(crate::ViewMode::BaseColour | crate::ViewMode::LightingOnly) => {
                ("view-hdr", &self.p_debug_hdr, &self.debug_bindings.hdr)
            }
            crate::FramePass::View(crate::ViewMode::AmbientOcclusion) => {
                let binding = self
                    .debug_bindings
                    .ambient_occlusion
                    .as_ref()
                    .unwrap_or(&self.debug_bindings.hdr);
                ("view-ambient-occlusion", &self.p_debug_ao, binding)
            }
            crate::FramePass::View(crate::ViewMode::Depth) => {
                if let Some(binding) = self.debug_bindings.depth.as_ref() {
                    ("view-depth", &self.p_debug_depth, binding)
                } else {
                    ("view-hdr", &self.p_debug_hdr, &self.debug_bindings.hdr)
                }
            }
            crate::FramePass::View(crate::ViewMode::OpaqueCoverage { .. }) => {
                let Some(binding) = self.debug_bindings.coverage.as_ref() else {
                    return;
                };
                ("view-opaque-coverage", &self.p_debug_coverage, binding)
            }
            crate::FramePass::View(crate::ViewMode::Shadow) => (
                "view-shadow",
                &self.p_debug_shadow,
                &self.debug_bindings.shadow,
            ),
            crate::FramePass::View(crate::ViewMode::Lit) => {
                ("view-hdr", &self.p_debug_hdr, &self.debug_bindings.hdr)
            }
            _ => return,
        };
        let timestamps = self
            .timer
            .as_mut()
            .and_then(|timer| timer.writes(GpuPass::Composite));
        ScreenSpace::fullscreen(
            encoder,
            label,
            pipeline,
            bind_group,
            target,
            FullscreenOptions {
                clear: wgpu::Color::BLACK,
                viewport: None,
            },
            timestamps,
        );
        stats.draws += 1;
    }

    /// Draws retained UI after the terminal pass has written the presentation target.
    pub(super) fn encode_ui(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        ui_vertices: usize,
        stats: &mut FrameStats,
    ) {
        if ui_vertices == 0 {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.p_ui);
        pass.set_bind_group(0, &self.ui_bg, &[]);
        self.queue
            .write_buffer(&self.ui_u, 0, bytemuck::bytes_of(&self.ui.uniform));
        pass.set_vertex_buffer(0, self.ui_buf.slice(..));
        pass.draw(0..to_u32(ui_vertices), 0..1);
        stats.draws += 1;
    }
}

/// Returns the prebuilt binding set for the source declared by the checked frame plan.
fn post_source(
    targets: &crate::targets::FrameTargets,
    source: crate::PostSource,
    history_slot: bool,
) -> &crate::targets::PostSource {
    match source {
        crate::PostSource::Hdr => targets.post.pick(false),
        crate::PostSource::History => targets.post.pick(history_slot),
    }
}

/// Parameters for encoding one bloom level.
struct BloomFullscreen<'a> {
    destination: &'a wgpu::TextureView,
    pipeline: &'a wgpu::RenderPipeline,
    bind_group: &'a wgpu::BindGroup,
    clear: bool,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'a>>,
    viewport: (u32, u32),
}

/// Encodes one bloom level and accounts for its fullscreen draw.
fn encode_bloom_fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    bloom: BloomFullscreen<'_>,
    stats: &mut FrameStats,
) {
    let BloomFullscreen {
        destination,
        pipeline,
        bind_group,
        clear,
        timestamps,
        viewport,
    } = bloom;
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("bloom"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: destination,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if clear {
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: timestamps,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_viewport(0.0, 0.0, viewport.0 as f32, viewport.1 as f32, 0.0, 1.0);
    pass.set_scissor_rect(0, 0, viewport.0, viewport.1);
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
    stats.draws += 1;
}
