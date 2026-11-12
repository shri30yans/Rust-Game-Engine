//! Per-pass GPU timestamps. The frame is the interval from the earliest pass start to the
//! latest pass end. On tiled GPUs, independently timestamped passes overlap; per-pass values
//! are useful for relative comparisons only, and only frame time is valid for budgets.
//! Composite can report 0.000 ms on these backends as part of the same timing effect.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// GPU render pass with its own timestamp pair. This list also determines the query-set size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum GpuPass {
    /// First sun shadow cascade.
    ShadowNear,
    /// Second sun shadow cascade.
    ShadowFar,
    /// Depth, normal and motion prepass.
    Prepass,
    /// GTAO estimate.
    Gtao,
    /// GTAO bilateral denoise.
    GtaoDenoise,
    /// HDR scene shading.
    Scene,
    /// Bloom downsample level 0.
    BloomDown0,
    /// Bloom downsample level 1.
    BloomDown1,
    /// Bloom downsample level 2.
    BloomDown2,
    /// Bloom downsample level 3.
    BloomDown3,
    /// Bloom downsample level 4.
    BloomDown4,
    /// Bloom upsample level 1.
    BloomUp1,
    /// Bloom upsample level 2.
    BloomUp2,
    /// Bloom upsample level 3.
    BloomUp3,
    /// Bloom upsample level 4.
    BloomUp4,
    /// Temporal anti-aliasing resolve.
    Taa,
    /// Auto-exposure measurement.
    Exposure,
    /// Tonemap composite and UI.
    Composite,
}

/// Ordered pass inventory used for query sizing and benchmark labels.
pub const GPU_PASSES: [GpuPass; 18] = [
    GpuPass::ShadowNear,
    GpuPass::ShadowFar,
    GpuPass::Prepass,
    GpuPass::Gtao,
    GpuPass::GtaoDenoise,
    GpuPass::Scene,
    GpuPass::BloomDown0,
    GpuPass::BloomDown1,
    GpuPass::BloomDown2,
    GpuPass::BloomDown3,
    GpuPass::BloomDown4,
    GpuPass::BloomUp1,
    GpuPass::BloomUp2,
    GpuPass::BloomUp3,
    GpuPass::BloomUp4,
    GpuPass::Taa,
    GpuPass::Exposure,
    GpuPass::Composite,
];

const PASS_COUNT: usize = GPU_PASSES.len();

fn query_count() -> u32 {
    u32::try_from(PASS_COUNT)
        .unwrap_or(u32::MAX)
        .saturating_mul(2)
}

fn readback_bytes() -> u64 {
    u64::from(query_count()).saturating_mul(8)
}

fn sum_passes(pass_times: &[f32; PASS_COUNT], passes: &[GpuPass]) -> f32 {
    passes
        .iter()
        .map(|pass| pass_times.get(*pass as usize).copied().unwrap_or(0.0))
        .sum()
}

fn pass_time(pass_times: &[f32; PASS_COUNT], pass: GpuPass) -> f32 {
    pass_times.get(pass as usize).copied().unwrap_or(0.0)
}

/// GPU milliseconds from the latest completed sample.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct GpuTimes {
    /// Earliest pass start to latest pass end.
    pub frame: f32,
    /// Sum of the two shadow cascades.
    pub shadow: f32,
    /// Depth-normal prepass.
    pub prepass: f32,
    /// GTAO estimate and denoise sum.
    pub gtao: f32,
    /// HDR scene.
    pub scene: f32,
    /// Bloom downsample and upsample sum.
    pub bloom: f32,
    /// Tonemap and UI.
    pub composite: f32,
    /// TAA resolve (zero when TAA is off).
    pub taa: f32,
    /// Auto-exposure measurement (zero when disabled).
    pub exposure: f32,
    /// Individual pass durations in [`GPU_PASSES`] order; absent passes are zero.
    pub passes: [f32; PASS_COUNT],
}

/// Asynchronous GPU timestamp reader.
pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    period: f32,
    ready: Arc<AtomicBool>,
    in_flight: bool,
    written: [bool; PASS_COUNT],
    pub times: GpuTimes,
}

impl GpuTimer {
    /// Creates a timer when timestamp queries are supported.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<GpuTimer> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let query_count = query_count();
        let bytes = readback_bytes();
        let buf = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage,
                mapped_at_creation: false,
            })
        };
        Some(GpuTimer {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("frame-timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: query_count,
            }),
            resolve: buf(
                "ts-resolve",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            readback: buf(
                "ts-readback",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            ),
            period: queue.get_timestamp_period(),
            ready: Arc::new(AtomicBool::new(false)),
            in_flight: false,
            written: [false; PASS_COUNT],
            times: GpuTimes::default(),
        })
    }

    /// Timestamp pair for one render pass.
    pub fn writes(&mut self, pass: GpuPass) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if self.in_flight {
            return None;
        }
        let index = pass as usize;
        let query = u32::try_from(index).ok()?.checked_mul(2)?;
        *self.written.get_mut(index)? = true;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(query),
            end_of_pass_write_index: Some(query + 1),
        })
    }

    /// Resolves this frame's timestamp pairs.
    pub fn resolve(&mut self, enc: &mut wgpu::CommandEncoder) {
        if !self.in_flight {
            let query_count = query_count();
            let bytes = readback_bytes();
            enc.resolve_query_set(&self.queries, 0..query_count, &self.resolve, 0);
            enc.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, bytes);
        }
    }

    /// Begins asynchronous readback after submission.
    pub fn after_submit(&mut self) {
        if self.in_flight {
            return;
        }
        self.in_flight = true;
        let ready = Arc::clone(&self.ready);
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| {
                ready.store(r.is_ok(), Ordering::Release);
            });
    }

    /// Collects a finished measurement without blocking.
    pub fn collect(&mut self, device: &wgpu::Device) {
        if !self.in_flight {
            return;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        if !self.ready.swap(false, Ordering::Acquire) {
            return;
        }
        if let Ok(data) = self.readback.slice(..).get_mapped_range() {
            let ticks: &[u64] = bytemuck::cast_slice(&data);
            let mut passes = [0.0; PASS_COUNT];
            let mut earliest = u64::MAX;
            let mut latest = 0u64;
            for pass in GPU_PASSES {
                let index = pass as usize;
                if !self.written.get(index).copied().unwrap_or(false) {
                    continue;
                }
                let Some(start) = ticks.get(index * 2).copied() else {
                    continue;
                };
                let Some(end) = ticks.get(index * 2 + 1).copied() else {
                    continue;
                };
                earliest = earliest.min(start);
                latest = latest.max(end);
                if let Some(pass_time) = passes.get_mut(index) {
                    *pass_time = end.saturating_sub(start) as f32 * self.period * 1e-6;
                }
            }
            self.times = GpuTimes {
                frame: if earliest == u64::MAX {
                    0.0
                } else {
                    latest.saturating_sub(earliest) as f32 * self.period * 1e-6
                },
                shadow: sum_passes(&passes, &[GpuPass::ShadowNear, GpuPass::ShadowFar]),
                prepass: pass_time(&passes, GpuPass::Prepass),
                gtao: sum_passes(&passes, &[GpuPass::Gtao, GpuPass::GtaoDenoise]),
                scene: pass_time(&passes, GpuPass::Scene),
                bloom: sum_passes(
                    &passes,
                    &[
                        GpuPass::BloomDown0,
                        GpuPass::BloomDown1,
                        GpuPass::BloomDown2,
                        GpuPass::BloomDown3,
                        GpuPass::BloomDown4,
                        GpuPass::BloomUp1,
                        GpuPass::BloomUp2,
                        GpuPass::BloomUp3,
                        GpuPass::BloomUp4,
                    ],
                ),
                composite: pass_time(&passes, GpuPass::Composite),
                taa: pass_time(&passes, GpuPass::Taa),
                exposure: pass_time(&passes, GpuPass::Exposure),
                passes,
            };
        }
        self.readback.unmap();
        self.written = [false; PASS_COUNT];
        self.in_flight = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{GPU_PASSES, GpuPass, PASS_COUNT, query_count, sum_passes};

    #[test]
    fn each_pass_has_its_own_query_pair() {
        assert_eq!(GPU_PASSES.len(), PASS_COUNT);
        let count = usize::try_from(query_count()).unwrap_or(usize::MAX);
        assert_eq!(count / 2, GPU_PASSES.len());
    }

    #[test]
    fn stage_totals_sum_their_individual_passes() {
        let mut times = [0.0; PASS_COUNT];
        for (pass, value) in [
            (GpuPass::ShadowNear, 0.2),
            (GpuPass::ShadowFar, 0.3),
            (GpuPass::Gtao, 0.4),
            (GpuPass::GtaoDenoise, 0.5),
        ] {
            if let Some(slot) = times.get_mut(pass as usize) {
                *slot = value;
            }
        }
        assert_eq!(
            sum_passes(&times, &[GpuPass::ShadowNear, GpuPass::ShadowFar]),
            0.5
        );
        assert_eq!(
            sum_passes(&times, &[GpuPass::Gtao, GpuPass::GtaoDenoise]),
            0.9
        );
    }
}
