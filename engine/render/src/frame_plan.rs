//! Checked rendering pass order derived from [`crate::Settings`].
//!
//! A plan is built at the settings boundary, before GPU resources are created. Every pass names
//! the targets it consumes and produces; validation rejects a pass that reads a target no earlier
//! pass supplied. Neutral targets constructed by the renderer are modeled as explicit external
//! inputs, never as earlier pass outputs. The frame loop consumes this immutable plan directly.

use crate::{RenderError, Settings};

/// The source and ownership of depth during opaque scene drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpaqueDepth {
    /// The opaque pass owns depth because no prepass produced it.
    Own,
    /// The opaque pass consumes depth already written by the prepass.
    FromPrepass,
}

impl OpaqueDepth {
    fn from_prepass(prepass: crate::Prepass) -> Self {
        if prepass.enabled() {
            Self::FromPrepass
        } else {
            Self::Own
        }
    }

    pub(crate) const fn state(self, group: OpaqueDrawGroup) -> OpaqueDepthState {
        use wgpu::CompareFunction::{Less, LessEqual};
        match (self, group) {
            (Self::Own, OpaqueDrawGroup::WorldOpaque) => OpaqueDepthState {
                compare: Less,
                write: true,
            },
            (Self::FromPrepass, OpaqueDrawGroup::WorldOpaque) => OpaqueDepthState {
                compare: LessEqual,
                write: false,
            },
            (Self::Own | Self::FromPrepass, OpaqueDrawGroup::Viewmodel) => OpaqueDepthState {
                compare: Less,
                write: true,
            },
            (Self::Own | Self::FromPrepass, OpaqueDrawGroup::Glow | OpaqueDrawGroup::Particles) => {
                OpaqueDepthState {
                    compare: Less,
                    write: false,
                }
            }
            (Self::Own | Self::FromPrepass, OpaqueDrawGroup::Sky) => OpaqueDepthState {
                compare: LessEqual,
                write: false,
            },
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum OpaqueDrawGroup {
    WorldOpaque,
    Viewmodel,
    Glow,
    Sky,
    Particles,
}

pub(crate) struct OpaqueDepthState {
    pub compare: wgpu::CompareFunction,
    pub write: bool,
}

/// The renderer targets that a planned pass may read or write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameTarget {
    /// The two-cascade shadow map, written by the shadow pass.
    Shadow,
    /// Opaque depth, written by the prepass or main scene.
    Depth,
    /// View-space normals written with the prepass.
    Normal,
    /// Per-pixel motion vectors written with the prepass.
    Velocity,
    /// Raw ambient occlusion before edge-aware denoising.
    AmbientOcclusionRaw,
    /// Denoised ambient occlusion read by the scene shader.
    AmbientOcclusion,
    /// HDR scene colour.
    Hdr,
    /// Temporal anti-aliasing history.
    History,
    /// Bloom pyramid.
    Bloom,
    /// Adapted exposure value.
    Exposure,
    /// The presentation target written before the UI pass.
    Output,
}

impl FrameTarget {
    const COUNT: usize = 11;

    const fn index(self) -> usize {
        match self {
            Self::Shadow => 0,
            Self::Depth => 1,
            Self::Normal => 2,
            Self::Velocity => 3,
            Self::AmbientOcclusionRaw => 4,
            Self::AmbientOcclusion => 5,
            Self::Hdr => 6,
            Self::History => 7,
            Self::Bloom => 8,
            Self::Exposure => 9,
            Self::Output => 10,
        }
    }
}

/// A neutral renderer resource supplied when its optional producing pass is absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameInput {
    /// White ambient occlusion used when GTAO is disabled.
    NeutralAmbientOcclusion,
    /// Black bloom used when bloom is disabled.
    NeutralBloom,
    /// Neutral exposure used when automatic exposure is disabled.
    NeutralExposure,
}

impl FrameInput {
    /// The target value supplied by this input.
    #[must_use]
    pub const fn target(self) -> FrameTarget {
        match self {
            Self::NeutralAmbientOcclusion => FrameTarget::AmbientOcclusion,
            Self::NeutralBloom => FrameTarget::Bloom,
            Self::NeutralExposure => FrameTarget::Exposure,
        }
    }
}

/// A renderer view selected at the settings boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewMode {
    /// The normal lit and post-processed frame.
    #[default]
    Lit,
    /// Surface base colour without lighting.
    BaseColour,
    /// Direct and ambient lighting without material colour.
    LightingOnly,
    /// The denoised ambient-occlusion buffer.
    AmbientOcclusion,
    /// The camera depth buffer.
    Depth,
    /// The sun shadow-map cascade selected for the camera.
    Shadow,
    /// Shows opaque coverage in red and whether the renderer depth target is covered in green.
    /// The selected prepass state is explicit so both depth ownership paths can be inspected.
    OpaqueCoverage {
        /// Whether this diagnostic frame writes depth in the prepass.
        prepass: bool,
    },
}

impl ViewMode {
    /// Stable short label for developer telemetry.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Lit => "Lit",
            Self::BaseColour => "Base colour",
            Self::LightingOnly => "Lighting only",
            Self::AmbientOcclusion => "Ambient occlusion",
            Self::Depth => "Depth",
            Self::Shadow => "Shadow",
            Self::OpaqueCoverage { .. } => "Opaque coverage",
        }
    }
}

/// The scene-colour target selected for ordinary post-processing.
///
/// TAA resolves into [`FrameTarget::History`]. Every subsequent post pass must consume that
/// history in the same frame rather than the pre-resolve HDR target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PostSource {
    /// The scene pass's HDR colour, used when temporal anti-aliasing is absent.
    Hdr,
    /// The history target written by the current temporal resolve.
    History,
}

impl PostSource {
    /// The frame target represented by this post-processing source.
    #[must_use]
    pub const fn target(self) -> FrameTarget {
        match self {
            Self::Hdr => FrameTarget::Hdr,
            Self::History => FrameTarget::History,
        }
    }
}

/// One ordered rendering operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramePass {
    /// Render the sun shadow cascades.
    Shadow,
    /// Write depth, normal and motion before opaque shading.
    Prepass,
    /// Produce raw screen-space ambient occlusion.
    Gtao,
    /// Denoise ambient occlusion.
    GtaoDenoise,
    /// Cull lights and render opaque geometry for `view_mode` into HDR colour.
    Scene(ViewMode),
    /// Resolve HDR colour into temporal history.
    Taa,
    /// Build and combine the bloom pyramid from `source`.
    Bloom(PostSource),
    /// Measure HDR luminance from `source` for automatic exposure.
    Exposure(PostSource),
    /// Present the ordinary lit frame from `source`.
    Composite(PostSource),
    /// Present an isolated renderer view.
    View(ViewMode),
    /// Draw the retained UI after presentation.
    Ui,
}

impl FramePass {
    /// The targets this pass requires before it can run.
    #[must_use]
    pub const fn reads(self) -> &'static [FrameTarget] {
        match self {
            Self::Shadow | Self::Prepass => &[],
            Self::Gtao => &[FrameTarget::Depth, FrameTarget::Normal],
            Self::GtaoDenoise => &[
                FrameTarget::AmbientOcclusionRaw,
                FrameTarget::Depth,
                FrameTarget::Normal,
            ],
            Self::Scene(_) => &[FrameTarget::Shadow, FrameTarget::AmbientOcclusion],
            Self::Taa => &[FrameTarget::Hdr, FrameTarget::Velocity, FrameTarget::Depth],
            Self::Bloom(PostSource::Hdr) | Self::Exposure(PostSource::Hdr) => &[FrameTarget::Hdr],
            Self::Bloom(PostSource::History) | Self::Exposure(PostSource::History) => {
                &[FrameTarget::History]
            }
            Self::Composite(PostSource::Hdr) => {
                &[FrameTarget::Hdr, FrameTarget::Bloom, FrameTarget::Exposure]
            }
            Self::Composite(PostSource::History) => &[
                FrameTarget::History,
                FrameTarget::Bloom,
                FrameTarget::Exposure,
            ],
            Self::View(ViewMode::Lit)
            | Self::View(ViewMode::BaseColour)
            | Self::View(ViewMode::LightingOnly) => &[FrameTarget::Hdr],
            Self::View(ViewMode::OpaqueCoverage { .. }) => &[FrameTarget::Hdr, FrameTarget::Depth],
            Self::View(ViewMode::AmbientOcclusion) => &[FrameTarget::AmbientOcclusion],
            Self::View(ViewMode::Depth) => &[FrameTarget::Depth],
            Self::View(ViewMode::Shadow) => &[FrameTarget::Shadow],
            Self::Ui => &[FrameTarget::Output],
        }
    }

    /// The targets this pass makes available to later passes.
    #[must_use]
    pub const fn writes(self) -> &'static [FrameTarget] {
        match self {
            Self::Shadow => &[FrameTarget::Shadow],
            Self::Prepass => &[
                FrameTarget::Depth,
                FrameTarget::Normal,
                FrameTarget::Velocity,
            ],
            Self::Gtao => &[FrameTarget::AmbientOcclusionRaw],
            Self::GtaoDenoise => &[FrameTarget::AmbientOcclusion],
            Self::Scene(_) => &[FrameTarget::Hdr, FrameTarget::Depth],
            Self::Taa => &[FrameTarget::History],
            Self::Bloom(_) => &[FrameTarget::Bloom],
            Self::Exposure(_) => &[FrameTarget::Exposure],
            Self::Composite(_) | Self::View(_) => &[FrameTarget::Output],
            Self::Ui => &[],
        }
    }
}

/// Largest legal number of passes in one frame plan.
const MAX_PASSES: usize = 11;

/// Immutable, checked list of the passes executed for one settings configuration.
///
/// The fixed storage lets the renderer copy its short checked schedule before encoding, without
/// allocating in the per-frame path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FramePlan {
    passes: [FramePass; MAX_PASSES],
    len: u8,
    opaque_depth: OpaqueDepth,
}

impl FramePlan {
    /// Builds and validates the sole rendering sequence for `settings`.
    ///
    /// GTAO and TAA request the prepass through [`crate::RenderFeatures`], so no legal settings
    /// value can create a plan that reads their depth, normal, or velocity inputs unproduced.
    pub fn build(settings: Settings) -> Result<Self, RenderError> {
        let features = settings.features();
        let mut plan = Self {
            passes: [FramePass::Shadow; MAX_PASSES],
            len: 0,
            opaque_depth: OpaqueDepth::from_prepass(features.prepass),
        };
        plan.push(FramePass::Shadow);
        if features.prepass_enabled() {
            plan.push(FramePass::Prepass);
        }
        if features.gtao() {
            plan.push(FramePass::Gtao);
            plan.push(FramePass::GtaoDenoise);
        }
        plan.push(FramePass::Scene(settings.view_mode));
        let post_source = if features.taa() {
            plan.push(FramePass::Taa);
            PostSource::History
        } else {
            PostSource::Hdr
        };
        if settings.bloom {
            plan.push(FramePass::Bloom(post_source));
        }
        if features.auto_exposure {
            plan.push(FramePass::Exposure(post_source));
        }
        if settings.view_mode == ViewMode::Lit {
            plan.push(FramePass::Composite(post_source));
        } else {
            plan.push(FramePass::View(settings.view_mode));
        }
        if !matches!(settings.view_mode, ViewMode::OpaqueCoverage { .. }) {
            plan.push(FramePass::Ui);
        }
        plan.validate()?;
        Ok(plan)
    }

    /// The checked ordered pass list.
    #[must_use]
    pub fn passes(&self) -> &[FramePass] {
        self.passes.get(..usize::from(self.len)).unwrap_or(&[])
    }

    fn push(&mut self, pass: FramePass) {
        if let Some(slot) = self.passes.get_mut(usize::from(self.len)) {
            *slot = pass;
            self.len = self.len.saturating_add(1);
        }
    }

    /// The neutral resources supplied by target construction rather than another planned pass.
    #[must_use]
    pub const fn external_inputs() -> &'static [FrameInput] {
        &[
            FrameInput::NeutralAmbientOcclusion,
            FrameInput::NeutralBloom,
            FrameInput::NeutralExposure,
        ]
    }

    /// Whether the checked plan measures automatic exposure.
    #[must_use]
    pub fn has_auto_exposure(&self) -> bool {
        self.passes()
            .iter()
            .any(|pass| matches!(pass, FramePass::Exposure(_)))
    }

    /// Whether `pass` is scheduled.
    #[must_use]
    pub fn contains(&self, pass: FramePass) -> bool {
        self.passes.contains(&pass)
    }

    /// Whether the plan reuses prepass depth for opaque shading.
    #[must_use]
    pub fn reuses_prepass_depth(&self) -> bool {
        self.opaque_depth == OpaqueDepth::FromPrepass
    }

    /// The depth source selected for opaque drawing.
    #[must_use]
    pub const fn opaque_depth(&self) -> OpaqueDepth {
        self.opaque_depth
    }

    fn validate(&self) -> Result<(), RenderError> {
        let mut available = [None; FrameTarget::COUNT];
        for input in Self::external_inputs() {
            let target = input.target();
            let Some(slot) = available.get_mut(target.index()) else {
                continue;
            };
            *slot = Some(FrameSource::Input);
        }
        for pass in self.passes() {
            for target in pass.reads() {
                let Some(source) = available.get(target.index()) else {
                    return Err(RenderError::InvalidFramePlan {
                        pass: *pass,
                        target: *target,
                    });
                };
                if source.is_none() {
                    return Err(RenderError::InvalidFramePlan {
                        pass: *pass,
                        target: *target,
                    });
                }
            }
            for target in pass.writes() {
                let Some(slot) = available.get_mut(target.index()) else {
                    return Err(RenderError::InvalidFramePlan {
                        pass: *pass,
                        target: *target,
                    });
                };
                *slot = Some(FrameSource::Pass);
            }
        }
        Ok(())
    }
}

/// The distinct origin of a target available to frame-plan validation.
#[derive(Clone, Copy)]
enum FrameSource {
    /// Target construction supplied a neutral input.
    Input,
    /// An earlier planned pass wrote the target.
    Pass,
}

#[cfg(test)]
mod tests;
