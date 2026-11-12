//! HDR render targets, bloom mips, TAA history, and post-process bind groups.
//!
//! Post passes read the scene colour through a source: the HDR target, or the TAA history
//! slot the resolve wrote this frame. [`FrameTargets::post`] holds one bind-group set per
//! source, so a frame picks its set without rebuilding bind groups.

use crate::BLOOM_LEVELS;

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Two values selected by the TAA write slot: `false` is slot zero, `true` is slot one.
pub(crate) struct Pair<T> {
    pub zero: T,
    pub one: T,
}

impl<T> Pair<T> {
    /// The value for `slot`.
    pub fn pick(&self, slot: bool) -> &T {
        if slot { &self.one } else { &self.zero }
    }
}

/// Post bind groups that read one scene-colour source.
pub(crate) struct PostSource {
    /// Bloom level 0 down-sample, reading the scene colour.
    pub down0: wgpu::BindGroup,
    /// Composite (tonemap, grade), reading the scene colour and bloom level 0.
    pub comp: wgpu::BindGroup,
    /// Exposure write pass. Binding 4 is the neutral placeholder, not the render target.
    pub measure: wgpu::BindGroup,
}

/// TAA history ping-pong. Slot `w` is written this frame; `bg.pick(w)` reads the other slot.
pub(crate) struct TaaTargets {
    pub history: Pair<wgpu::TextureView>,
    pub bg: Pair<wgpu::BindGroup>,
}

pub(crate) struct FrameTargets {
    #[cfg(test)]
    pub hdr_texture: wgpu::Texture,
    pub hdr: wgpu::TextureView,
    pub msaa: Option<wgpu::TextureView>,
    pub depth: Option<wgpu::TextureView>,
    pub bloom: Vec<(wgpu::TextureView, (u32, u32))>,
    /// Bloom down-sample bind groups for levels 1.. (level 0 is in `post`).
    pub bg_down: Vec<wgpu::BindGroup>,
    pub bg_up: Vec<wgpu::BindGroup>,
    /// Post sets indexed by the TAA write slot (both entries are the HDR target without TAA).
    pub post: Pair<PostSource>,
    pub taa: Option<TaaTargets>,
}

/// TAA inputs that the target build needs from the screen-space pass.
#[derive(Clone, Copy)]
pub(crate) struct TaaTargetDesc<'a> {
    pub bgl: &'a wgpu::BindGroupLayout,
    pub velocity: &'a wgpu::TextureView,
    pub depth: &'a wgpu::TextureView,
}

/// Inputs for building per-frame HDR/bloom targets.
#[derive(Clone, Copy)]
pub(crate) struct FrameTargetDesc<'a> {
    pub device: &'a wgpu::Device,
    pub post_bgl: &'a wgpu::BindGroupLayout,
    pub sampler: &'a wgpu::Sampler,
    pub post_buf: &'a wgpu::Buffer,
    pub window_size: (u32, u32),
    pub msaa: u32,
    /// When true, depth lives in the screen-space pass instead.
    pub skip_depth: bool,
    pub exposure: &'a wgpu::TextureView,
    /// Neutral 1×1 placeholder for binding 4 while writing `exposure`.
    pub measure_exposure: &'a wgpu::TextureView,
    pub taa_history_format: wgpu::TextureFormat,
    /// `Some` when TAA runs; the history slots are then the post sources.
    pub taa: Option<TaaTargetDesc<'a>>,
}

pub(crate) fn neutral_1x1(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::TextureView) {
    let one = |label, format| {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
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
        #[cfg(test)]
        crate::gfx::setup::record_texture_creation();
        let bytes = [0u8, 0, 0x3c, 0x3c];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
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
        let view = tex.create_view(&Default::default());
        #[cfg(test)]
        crate::gfx::setup::record_view_creation();
        view
    };
    (
        one("ao-neutral", wgpu::TextureFormat::R16Float),
        one("exposure-neutral", wgpu::TextureFormat::R16Float),
    )
}

/// Shared inputs for the post bind-group sets.
struct PostInputs<'a> {
    device: &'a wgpu::Device,
    post_bgl: &'a wgpu::BindGroupLayout,
    sampler: &'a wgpu::Sampler,
    post_buf: &'a wgpu::Buffer,
    exposure: &'a wgpu::TextureView,
    measure_exposure: &'a wgpu::TextureView,
}

impl PostInputs<'_> {
    fn bind(
        &self,
        label: &str,
        src: &wgpu::TextureView,
        bloom: &wgpu::TextureView,
        exposure: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: self.post_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(src),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.post_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(bloom),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(exposure),
                },
            ],
        });
        #[cfg(test)]
        crate::gfx::setup::record_bind_group_creation();
        group
    }

    /// Bind groups that read `src` as the scene colour.
    fn post_source(&self, src: &wgpu::TextureView, bloom0: &wgpu::TextureView) -> PostSource {
        PostSource {
            down0: self.bind("post", src, src, self.exposure),
            comp: self.bind("post-comp", src, bloom0, self.exposure),
            measure: self.bind("post-measure", src, bloom0, self.measure_exposure),
        }
    }
}

pub(crate) fn build(desc: &FrameTargetDesc<'_>) -> FrameTargets {
    let FrameTargetDesc {
        device,
        post_bgl,
        sampler,
        post_buf,
        window_size,
        msaa,
        skip_depth,
        exposure,
        measure_exposure,
        taa_history_format,
        taa,
    } = *desc;
    // Targets retain the maximum viewport so scale changes never recreate GPU resources.
    let sw = window_size.0.max(1);
    let sh = window_size.1.max(1);
    let tex =
        |label: &str, w: u32, h: u32, format: wgpu::TextureFormat, samples: u32, sampled: bool| {
            let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
            if sampled {
                usage |= wgpu::TextureUsages::TEXTURE_BINDING;
            }
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            });
            #[cfg(test)]
            crate::gfx::setup::record_texture_creation();
            let view = texture.create_view(&Default::default());
            #[cfg(test)]
            crate::gfx::setup::record_view_creation();
            view
        };
    #[cfg(test)]
    let hdr_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST;
    #[cfg(not(test))]
    let hdr_usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let hdr_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hdr"),
        size: wgpu::Extent3d {
            width: sw,
            height: sh,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR,
        usage: hdr_usage,
        view_formats: &[],
    });
    #[cfg(test)]
    crate::gfx::setup::record_texture_creation();
    let hdr = hdr_texture.create_view(&Default::default());
    #[cfg(test)]
    crate::gfx::setup::record_view_creation();
    let msaa_v = if msaa > 1 {
        Some(tex("hdr-msaa", sw, sh, HDR, msaa, false))
    } else {
        None
    };
    let depth = if skip_depth {
        None
    } else {
        Some(tex("depth", sw, sh, DEPTH, msaa, true))
    };
    let mut bloom = Vec::new();
    let (mut w, mut h) = (sw, sh);
    for _ in 0..BLOOM_LEVELS {
        w = (w / 2).max(1);
        h = (h / 2).max(1);
        bloom.push((tex("bloom", w, h, HDR, 1, true), (w, h)));
    }

    let inputs = PostInputs {
        device,
        post_bgl,
        sampler,
        post_buf,
        exposure,
        measure_exposure,
    };
    let bloom0 = bloom.first().map(|b| &b.0).unwrap_or(&hdr);
    // Bloom levels 1.. read the level above; level 0 reads the scene colour (see `post`).
    let bg_down: Vec<wgpu::BindGroup> = bloom
        .iter()
        .take(BLOOM_LEVELS.saturating_sub(1))
        .map(|(v, _)| inputs.bind("post", v, v, exposure))
        .collect();
    let bg_up: Vec<wgpu::BindGroup> = bloom
        .iter()
        .map(|(v, _)| inputs.bind("post", v, v, exposure))
        .collect();

    let taa_targets = taa.map(|t| {
        let h0 = tex("taa-history-0", sw, sh, taa_history_format, 1, true);
        let h1 = tex("taa-history-1", sw, sh, taa_history_format, 1, true);
        // Slot 0 is written from history 1 and slot 1 from history 0.
        let bg_for = |history: &wgpu::TextureView| {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("taa"),
                layout: t.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&hdr),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(history),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(t.velocity),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(t.depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            });
            #[cfg(test)]
            crate::gfx::setup::record_bind_group_creation();
            group
        };
        let bg = Pair {
            zero: bg_for(&h1),
            one: bg_for(&h0),
        };
        TaaTargets {
            history: Pair { zero: h0, one: h1 },
            bg,
        }
    });

    let post = match &taa_targets {
        Some(t) => Pair {
            zero: inputs.post_source(&t.history.zero, bloom0),
            one: inputs.post_source(&t.history.one, bloom0),
        },
        None => Pair {
            zero: inputs.post_source(&hdr, bloom0),
            one: inputs.post_source(&hdr, bloom0),
        },
    };

    FrameTargets {
        #[cfg(test)]
        hdr_texture,
        hdr,
        msaa: msaa_v,
        depth,
        bloom,
        bg_down,
        bg_up,
        post,
        taa: taa_targets,
    }
}
