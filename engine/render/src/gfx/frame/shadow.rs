//! Shadow-cascade pass encoding.

use super::{FrameStats, Gfx, GpuPass};

impl Gfx {
    /// Encodes the near cascade and the dirty far cascade.
    pub(super) fn encode_shadow(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        stats: &mut FrameStats,
    ) {
        for (cascade, (layer, uniform_bind_group)) in
            self.shadow_layers.iter().zip(&self.shadow_ubg).enumerate()
        {
            if cascade == 1 && !self.far_shadow_dirty {
                continue;
            }
            let pass_id = if cascade == 0 {
                GpuPass::ShadowNear
            } else {
                GpuPass::ShadowFar
            };
            let timestamps = self.timer.as_mut().and_then(|timer| timer.writes(pass_id));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: layer,
                    depth_ops: Some(wgpu::Operations {
                        load: if cascade == 0 || self.far_shadow_dirty {
                            wgpu::LoadOp::Clear(1.0)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: timestamps,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, uniform_bind_group, &[]);
            pass.set_bind_group(1, &self.scene_bg, &[]);
            for bucket in &self.buckets {
                let Some(mesh) = self.meshes.get(bucket.mesh.0) else {
                    continue;
                };
                let Some(lod) = mesh.lod(bucket.lod) else {
                    continue;
                };
                let ranges = &bucket.ranges;
                let count = ranges.shadow_count.get(cascade).copied().unwrap_or(0);
                if count == 0 {
                    continue;
                }
                let Some(pipeline) = self.p_shadow.get(lod.format.pipeline_index()) else {
                    continue;
                };
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(0, lod.vb.slice(..));
                let start = ranges.shadow_start.get(cascade).copied().unwrap_or(0);
                pass.set_index_buffer(lod.ib.slice(..), lod.index_format);
                pass.draw_indexed(
                    lod.indices.clone(),
                    lod.vertex_base,
                    start..start.saturating_add(count),
                );
                stats.shadow_draws += 1;
            }
        }
    }
}
