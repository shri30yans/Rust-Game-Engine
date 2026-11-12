//! Depth, normal, motion, and ambient-occlusion pass encoding.

use super::{FullscreenOptions, Gfx, GpuPass, Mat4, ScreenSpace, scale_px_ceil};

impl Gfx {
    /// Encodes the depth, normal, and velocity producer requested by the plan.
    pub(super) fn encode_prepass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        active_size: (u32, u32),
    ) {
        let Some(screen) = self.screen.as_ref() else {
            return;
        };
        let timestamps = self
            .timer
            .as_mut()
            .and_then(|timer| timer.writes(GpuPass::Prepass));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("prepass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &screen.normal_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &screen.velocity_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &screen.depth_render_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(
            0.0,
            0.0,
            active_size.0 as f32,
            active_size.1 as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(0, 0, active_size.0, active_size.1);
        pass.set_bind_group(0, &self.view_bg, &[]);
        for bucket in &self.buckets {
            let Some(mesh) = self.meshes.get(bucket.mesh.0) else {
                continue;
            };
            let Some(lod) = mesh.lod(bucket.lod) else {
                continue;
            };
            let ranges = &bucket.ranges;
            let count = ranges.both + ranges.mo;
            if count == 0 {
                continue;
            }
            let Some(pipeline) = screen.p_prepass.get(lod.format.pipeline_index()) else {
                continue;
            };
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, lod.vb.slice(..));
            pass.set_index_buffer(lod.ib.slice(..), lod.index_format);
            pass.draw_indexed(
                lod.indices.clone(),
                lod.vertex_base,
                ranges.start..ranges.start.saturating_add(count),
            );
        }
    }

    /// Encodes raw GTAO after its checked depth and normal inputs are available.
    pub(super) fn encode_gtao(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        active_size: (u32, u32),
        inverse_view_projection: Mat4,
    ) {
        let Some(screen) = self.screen.as_ref() else {
            return;
        };
        let viewport = (
            scale_px_ceil(active_size.0, screen.features.gtao_resolution_scale()).max(1),
            scale_px_ceil(active_size.1, screen.features.gtao_resolution_scale()).max(1),
        );
        screen.write_gtao_uniforms(
            &self.queue,
            inverse_view_projection,
            self.settings.render_scale,
        );
        let timestamps = self
            .timer
            .as_mut()
            .and_then(|timer| timer.writes(GpuPass::Gtao));
        ScreenSpace::fullscreen(
            encoder,
            "gtao",
            &screen.p_gtao,
            &screen.gtao_bg,
            &screen.ao_raw_view,
            FullscreenOptions {
                clear: wgpu::Color::WHITE,
                viewport: Some(viewport),
            },
            timestamps,
        );
    }

    /// Encodes GTAO denoising after the checked raw-AO input is available.
    pub(super) fn encode_gtao_denoise(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        active_size: (u32, u32),
    ) {
        let Some(screen) = self.screen.as_ref() else {
            return;
        };
        let viewport = (
            scale_px_ceil(active_size.0, screen.features.gtao_resolution_scale()).max(1),
            scale_px_ceil(active_size.1, screen.features.gtao_resolution_scale()).max(1),
        );
        let timestamps = self
            .timer
            .as_mut()
            .and_then(|timer| timer.writes(GpuPass::GtaoDenoise));
        ScreenSpace::fullscreen(
            encoder,
            "gtao-denoise",
            &screen.p_denoise,
            &screen.denoise_bg,
            &screen.ao_final_view,
            FullscreenOptions {
                clear: wgpu::Color::WHITE,
                viewport: Some(viewport),
            },
            timestamps,
        );
    }
}
