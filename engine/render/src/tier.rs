//! Render quality tiers and feature selections.
//!
//! [`Prepass`] makes screen-space dependencies explicit: requesting GTAO or TAA necessarily
//! enables the depth, normal, and velocity producer. The renderer consumes the resulting checked
//! [`crate::FramePlan`], not independent flags.

/// Desktop cooked-resource upload allowance per frame, in bytes.
pub const DESKTOP_UPLOAD_BUDGET_BYTES: u64 = 8 * 1024 * 1024;
/// Phone cooked-resource upload allowance per frame, in bytes.
pub const PHONE_UPLOAD_BUDGET_BYTES: u64 = 2 * 1024 * 1024;

/// Discrete quality preset from stress-test through ultra desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderTier {
    /// Fastest path for automated tests and CI captures.
    Test,
    /// Phones and low-power GPUs: MSAA, no screen-space extras.
    Low,
    /// Default laptops: MSAA, higher shadow resolution.
    Medium,
    /// Desktop default: GTAO + auto exposure.
    High,
    /// Showcase: full-resolution GTAO, larger shadows, full internal resolution.
    Ultra,
}

impl RenderTier {
    /// Maximum cooked mesh and texture bytes uploaded during one frame on this tier.
    #[must_use]
    pub const fn upload_budget_bytes(self) -> u64 {
        match self {
            Self::Low => PHONE_UPLOAD_BUDGET_BYTES,
            Self::Test | Self::Medium | Self::High | Self::Ultra => DESKTOP_UPLOAD_BUDGET_BYTES,
        }
    }
}

/// Ground-truth ambient-occlusion quality, which requires a depth prepass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gtao {
    /// Half-resolution ambient occlusion for the High tier and diagnostics.
    Half,
    /// Full-resolution ambient occlusion for the Ultra tier.
    Full,
}

impl Gtao {
    /// GTAO buffer resolution relative to the HDR target.
    #[must_use]
    pub const fn resolution_scale(self) -> f32 {
        match self {
            Self::Half => 0.5,
            Self::Full => 1.0,
        }
    }
}

/// The depth producer and the passes that consume it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prepass {
    /// No screen-space consumer requires the prepass.
    Off,
    /// Depth, normal, and velocity are produced before their listed consumers run.
    On {
        /// GTAO quality, when ambient occlusion is requested.
        gtao: Option<Gtao>,
        /// Whether temporal anti-aliasing reads velocity and depth.
        taa: bool,
    },
}

impl Prepass {
    /// Whether the depth, normal, and velocity producer runs.
    #[must_use]
    pub const fn enabled(self) -> bool {
        matches!(self, Self::On { .. })
    }

    /// Requested GTAO quality, if any.
    #[must_use]
    pub const fn gtao(self) -> Option<Gtao> {
        match self {
            Self::Off => None,
            Self::On { gtao, .. } => gtao,
        }
    }

    /// Whether TAA consumes the prepass outputs.
    #[must_use]
    pub const fn taa(self) -> bool {
        match self {
            Self::Off => false,
            Self::On { taa, .. } => taa,
        }
    }

    /// Selects TAA while preserving any GTAO request. Enabling it creates its producer.
    #[must_use]
    pub const fn with_taa(self, taa: bool) -> Self {
        match (self, taa) {
            (Self::Off, false) => Self::Off,
            (Self::Off, true) => Self::On {
                gtao: None,
                taa: true,
            },
            (Self::On { gtao, .. }, taa) => Self::On { gtao, taa },
        }
    }

    /// Selects a sampled depth producer while preserving its existing consumers.
    #[must_use]
    pub const fn with_depth(self) -> Self {
        match self {
            Self::Off => Self::On {
                gtao: None,
                taa: false,
            },
            Self::On { gtao, taa } => Self::On { gtao, taa },
        }
    }

    /// Selects GTAO and its required depth producer while preserving TAA.
    #[must_use]
    pub const fn with_gtao(self, gtao: Gtao) -> Self {
        match self {
            Self::Off => Self::On {
                gtao: Some(gtao),
                taa: false,
            },
            Self::On { taa, .. } => Self::On {
                gtao: Some(gtao),
                taa,
            },
        }
    }
}

/// Optional renderer passes selected by [`RenderTier::features`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderFeatures {
    /// The prepass and its consumers. GTAO and TAA cannot exist without `On`.
    pub prepass: Prepass,
    /// HDR luminance adaptation written to a 1×1 exposure texture each frame.
    pub auto_exposure: bool,
}

impl RenderFeatures {
    /// Whether the prepass writes depth, normals, and velocity.
    #[must_use]
    pub const fn prepass_enabled(self) -> bool {
        self.prepass.enabled()
    }

    /// Whether GTAO runs.
    #[must_use]
    pub const fn gtao(self) -> bool {
        self.prepass.gtao().is_some()
    }

    /// GTAO buffer resolution, or 1.0 where GTAO is absent.
    #[must_use]
    pub const fn gtao_resolution_scale(self) -> f32 {
        match self.prepass.gtao() {
            Some(gtao) => gtao.resolution_scale(),
            None => 1.0,
        }
    }

    /// Whether TAA runs.
    #[must_use]
    pub const fn taa(self) -> bool {
        self.prepass.taa()
    }

    /// Applies a TAA request while retaining the required prepass.
    #[must_use]
    pub const fn with_taa(self, taa: bool) -> Self {
        Self {
            prepass: self.prepass.with_taa(taa),
            ..self
        }
    }

    /// Adds the producers required by the selected terminal renderer view.
    ///
    /// The developer depth and ambient-occlusion views work on every tier. Depth selects the
    /// sampled prepass, while ambient occlusion additionally selects GTAO at half resolution.
    #[must_use]
    pub const fn with_view_mode(self, view_mode: crate::ViewMode) -> Self {
        match view_mode {
            crate::ViewMode::AmbientOcclusion => Self {
                prepass: self.prepass.with_gtao(Gtao::Half),
                ..self
            },
            crate::ViewMode::Depth => Self {
                prepass: self.prepass.with_depth(),
                ..self
            },
            crate::ViewMode::OpaqueCoverage { prepass } => Self {
                prepass: if prepass {
                    Prepass::On {
                        gtao: None,
                        taa: false,
                    }
                } else {
                    Prepass::Off
                },
                auto_exposure: false,
            },
            crate::ViewMode::Lit
            | crate::ViewMode::BaseColour
            | crate::ViewMode::LightingOnly
            | crate::ViewMode::Shadow => self,
        }
    }
}

impl RenderTier {
    /// Screen-space and prepass features for this tier.
    #[must_use]
    pub const fn features(self) -> RenderFeatures {
        match self {
            RenderTier::Test | RenderTier::Low | RenderTier::Medium => RenderFeatures {
                prepass: Prepass::Off,
                auto_exposure: false,
            },
            RenderTier::High => RenderFeatures {
                prepass: Prepass::On {
                    gtao: Some(Gtao::Half),
                    taa: true,
                },
                auto_exposure: true,
            },
            RenderTier::Ultra => RenderFeatures {
                prepass: Prepass::On {
                    gtao: Some(Gtao::Full),
                    taa: true,
                },
                auto_exposure: true,
            },
        }
    }

    /// Suggested [`crate::Settings`] for this tier. Callers may override individual fields.
    #[must_use]
    pub fn settings(self) -> crate::Settings {
        let mut settings = crate::Settings {
            tier: self,
            ..crate::Settings::default()
        };
        match self {
            RenderTier::Test => {
                settings.msaa = 1;
                settings.shadow_size = 1024;
                settings.render_scale = 0.75;
                settings.max_lights = 16;
                settings.bloom = false;
            }
            RenderTier::Low => {
                settings.msaa = 2;
                settings.shadow_size = 1024;
                settings.render_scale = 0.85;
                settings.max_lights = 16;
            }
            RenderTier::Medium => {
                settings.msaa = 4;
                settings.shadow_size = 2048;
            }
            RenderTier::High => {
                settings.msaa = 1;
                settings.shadow_size = 2048;
            }
            RenderTier::Ultra => {
                settings.msaa = 1;
                settings.shadow_size = 4096;
                settings.pcf = true;
            }
        }
        settings.taa = self.features().taa();
        settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gtao_tiers_disable_msaa() {
        assert_eq!(RenderTier::High.settings().msaa, 1);
        assert_eq!(RenderTier::Ultra.settings().msaa, 1);
        assert_eq!(RenderTier::Medium.settings().msaa, 4);
    }

    #[test]
    fn requested_taa_creates_its_prepass() {
        let features = RenderTier::Medium.features().with_taa(true);
        assert!(features.prepass_enabled());
        assert!(features.taa());
    }

    #[test]
    fn debug_views_create_low_and_medium_producers() {
        let low_depth = RenderTier::Low
            .features()
            .with_view_mode(crate::ViewMode::Depth);
        assert!(low_depth.prepass_enabled());
        assert!(!low_depth.gtao());

        let medium_ao = RenderTier::Medium
            .features()
            .with_view_mode(crate::ViewMode::AmbientOcclusion);
        assert!(medium_ao.prepass_enabled());
        assert!(medium_ao.gtao());
    }

    #[test]
    fn high_has_gtao_and_exposure() {
        let features = RenderTier::High.features();
        assert!(features.prepass_enabled() && features.gtao() && features.auto_exposure);
        assert!((features.gtao_resolution_scale() - 0.5).abs() < f32::EPSILON);
    }
}
