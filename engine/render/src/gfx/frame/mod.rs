//! Frame preparation, checked pass dispatch, and command submission.
//!
//! [`Gfx::render`] copies the fixed checked [`crate::FramePlan`] and visits each stored pass once.
//! Child modules own the preparation work or one class of GPU pass; they share one command encoder
//! and one [`PreparedFrame`] so no second schedule or inferred pass order exists.

mod post;
mod prepare;
mod prepass;
mod scene;
mod shadow;

use super::*;

/// Data prepared once and shared by the ordered frame-pass encoders.
struct PreparedFrame {
    active_size: (u32, u32),
    inv_view_proj: Mat4,
    alpha_particles: usize,
    additive_particles: usize,
    ui_vertices: usize,
    prepass_scheduled: bool,
    scene_available: bool,
    post_slot: bool,
    stats: FrameStats,
}

impl Gfx {
    /// Render everything submitted since `begin_frame` into `target`.
    pub fn render(&mut self, target: &wgpu::TextureView, dt: f32) -> FrameStats {
        let mut frame = self.prepare_frame(dt);
        let encode_started = std::time::Instant::now();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        if let Some(timer) = self.timer.as_mut() {
            timer.collect(&self.device);
        }

        // Copying the bounded plan permits mutable renderer access without allocating. This is the
        // sole dispatch path: every stored pass is considered exactly once in its validated order.
        let plan = self.plan;
        for pass in plan.passes() {
            match *pass {
                crate::FramePass::Shadow => self.encode_shadow(&mut encoder, &mut frame.stats),
                crate::FramePass::Prepass => {
                    frame.prepass_scheduled = true;
                    self.encode_prepass(&mut encoder, frame.active_size);
                }
                crate::FramePass::Gtao => {
                    self.encode_gtao(&mut encoder, frame.active_size, frame.inv_view_proj);
                }
                crate::FramePass::GtaoDenoise => {
                    self.encode_gtao_denoise(&mut encoder, frame.active_size);
                }
                crate::FramePass::Scene(view_mode) => {
                    frame.scene_available = self.encode_scene(
                        &mut encoder,
                        frame.active_size,
                        frame.prepass_scheduled,
                        view_mode,
                        (frame.alpha_particles, frame.additive_particles),
                        &mut frame.stats,
                    );
                }
                crate::FramePass::Taa if frame.scene_available => {
                    self.encode_taa(&mut encoder, frame.active_size, &mut frame.post_slot);
                }
                crate::FramePass::Bloom(source) if frame.scene_available => self.encode_bloom(
                    &mut encoder,
                    frame.active_size,
                    source,
                    frame.post_slot,
                    &mut frame.stats,
                ),
                crate::FramePass::Exposure(source) if frame.scene_available => {
                    self.encode_exposure(&mut encoder, source, frame.post_slot);
                }
                crate::FramePass::Composite(source) if frame.scene_available => self
                    .encode_terminal(
                        &mut encoder,
                        target,
                        crate::FramePass::Composite(source),
                        frame.post_slot,
                        &mut frame.stats,
                    ),
                crate::FramePass::View(view_mode) if frame.scene_available => self.encode_terminal(
                    &mut encoder,
                    target,
                    crate::FramePass::View(view_mode),
                    frame.post_slot,
                    &mut frame.stats,
                ),
                crate::FramePass::Ui if frame.scene_available => {
                    self.encode_ui(&mut encoder, target, frame.ui_vertices, &mut frame.stats);
                }
                _ => {}
            }
        }

        if let Some(timer) = self.timer.as_mut() {
            timer.resolve(&mut encoder);
        }
        self.queue.submit([encoder.finish()]);
        frame.stats.encode_time_ns =
            u64::try_from(encode_started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        if let Some(timer) = self.timer.as_mut() {
            timer.after_submit();
        }
        self.history_scale = self.settings.render_scale;
        self.last = frame.stats;
        frame.stats
    }
}
