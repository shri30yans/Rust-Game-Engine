//! Tests for frame-plan validation and depth ownership.

use super::*;
use crate::RenderTier;

#[test]
fn every_tier_builds_a_valid_plan() {
    for tier in [
        RenderTier::Test,
        RenderTier::Low,
        RenderTier::Medium,
        RenderTier::High,
        RenderTier::Ultra,
    ] {
        assert!(FramePlan::build(tier.settings()).is_ok(), "{tier:?}");
    }
}

#[test]
fn plan_depth_ownership_follows_the_prepass_and_viewmodel_owns_its_depth() {
    for (tier, expected) in [
        (RenderTier::Test, OpaqueDepth::Own),
        (RenderTier::Low, OpaqueDepth::Own),
        (RenderTier::Medium, OpaqueDepth::Own),
        (RenderTier::High, OpaqueDepth::FromPrepass),
        (RenderTier::Ultra, OpaqueDepth::FromPrepass),
    ] {
        let plan = FramePlan::build(tier.settings());
        assert!(plan.is_ok(), "{tier:?}");
        let Ok(plan) = plan else {
            continue;
        };
        assert_eq!(plan.opaque_depth(), expected, "{tier:?}");
    }

    let viewmodel = OpaqueDepth::FromPrepass.state(OpaqueDrawGroup::Viewmodel);
    assert_eq!(viewmodel.compare, wgpu::CompareFunction::Less);
    assert!(viewmodel.write);
}

#[test]
fn opaque_coverage_mode_selects_both_depth_paths_without_ui_or_post_passes() {
    for prepass in [false, true] {
        for tier in [
            RenderTier::Test,
            RenderTier::Low,
            RenderTier::Medium,
            RenderTier::High,
            RenderTier::Ultra,
        ] {
            let mut settings = tier.settings();
            settings.view_mode = ViewMode::OpaqueCoverage { prepass };
            let plan = FramePlan::build(settings);
            assert!(plan.is_ok(), "{tier:?}, prepass={prepass}");
            let Ok(plan) = plan else {
                continue;
            };
            assert_eq!(plan.reuses_prepass_depth(), prepass);
            assert_eq!(settings.effective_msaa(), 1);
            assert!(matches!(
                plan.passes().last(),
                Some(FramePass::View(ViewMode::OpaqueCoverage { prepass: selected }))
                    if *selected == prepass
            ));
            assert!(!plan.passes().contains(&FramePass::Ui));
            assert!(
                !plan.passes().iter().any(|pass| matches!(
                    pass,
                    FramePass::Taa
                        | FramePass::Gtao
                        | FramePass::GtaoDenoise
                        | FramePass::Exposure(_)
                )),
                "coverage view keeps its depth and colour readback unfiltered"
            );
        }
    }
}

#[test]
fn prepass_world_reads_equal_depth_without_writing_it() {
    let depth = OpaqueDepth::FromPrepass.state(OpaqueDrawGroup::WorldOpaque);
    assert_eq!(depth.compare, wgpu::CompareFunction::LessEqual);
    assert!(!depth.write);
}

#[test]
fn depth_state_is_selected_once_for_every_draw_group() {
    for (source, group, compare, write) in [
        (
            OpaqueDepth::Own,
            OpaqueDrawGroup::WorldOpaque,
            wgpu::CompareFunction::Less,
            true,
        ),
        (
            OpaqueDepth::FromPrepass,
            OpaqueDrawGroup::WorldOpaque,
            wgpu::CompareFunction::LessEqual,
            false,
        ),
        (
            OpaqueDepth::FromPrepass,
            OpaqueDrawGroup::Viewmodel,
            wgpu::CompareFunction::Less,
            true,
        ),
        (
            OpaqueDepth::FromPrepass,
            OpaqueDrawGroup::Glow,
            wgpu::CompareFunction::Less,
            false,
        ),
        (
            OpaqueDepth::FromPrepass,
            OpaqueDrawGroup::Sky,
            wgpu::CompareFunction::LessEqual,
            false,
        ),
        (
            OpaqueDepth::FromPrepass,
            OpaqueDrawGroup::Particles,
            wgpu::CompareFunction::Less,
            false,
        ),
    ] {
        let state = source.state(group);
        assert_eq!(state.compare, compare);
        assert_eq!(state.write, write);
    }
}
#[test]
fn declared_pass_io_is_public_and_neutral_inputs_are_distinct() {
    assert_eq!(
        FramePass::Gtao.reads(),
        &[FrameTarget::Depth, FrameTarget::Normal]
    );
    assert_eq!(
        FramePass::GtaoDenoise.writes(),
        &[FrameTarget::AmbientOcclusion]
    );
    assert_eq!(
        FramePlan::external_inputs(),
        &[
            FrameInput::NeutralAmbientOcclusion,
            FrameInput::NeutralBloom,
            FrameInput::NeutralExposure,
        ]
    );
    assert_eq!(FrameInput::NeutralBloom.target(), FrameTarget::Bloom);
}

#[test]
fn invalid_plan_identifies_the_pass_and_missing_target() {
    let plan = FramePlan {
        passes: [FramePass::Scene(ViewMode::Lit); MAX_PASSES],
        len: 1,
        opaque_depth: OpaqueDepth::Own,
    };
    assert!(matches!(
        plan.validate(),
        Err(RenderError::InvalidFramePlan {
            pass: FramePass::Scene(ViewMode::Lit),
            target: FrameTarget::Shadow,
        })
    ));
}

#[test]
fn taa_implies_the_prepass_on_every_tier() {
    let mut settings = RenderTier::Medium.settings();
    settings.taa = true;
    let result = FramePlan::build(settings);
    assert!(result.is_ok());
    let Ok(plan) = result else {
        return;
    };
    assert!(plan.contains(FramePass::Prepass));
    assert!(plan.contains(FramePass::Taa));
}

#[test]
fn non_lit_mode_is_a_terminal_plan_pass() {
    for view_mode in [
        ViewMode::BaseColour,
        ViewMode::LightingOnly,
        ViewMode::AmbientOcclusion,
        ViewMode::Depth,
        ViewMode::Shadow,
    ] {
        let mut settings = RenderTier::Low.settings();
        settings.view_mode = view_mode;
        let result = FramePlan::build(settings);
        assert!(result.is_ok(), "{view_mode:?}");
        let Ok(plan) = result else {
            continue;
        };
        assert_eq!(plan.passes().last().copied(), Some(FramePass::Ui));
        assert_eq!(
            plan.passes()
                .get(plan.passes().len().saturating_sub(2))
                .copied(),
            Some(FramePass::View(view_mode))
        );
        assert!(
            !plan
                .passes()
                .iter()
                .any(|pass| matches!(pass, FramePass::Composite(_)))
        );
    }
}

#[test]
fn post_passes_read_the_selected_taa_source_and_composite_dependencies() {
    let mut taa = RenderTier::High.settings();
    taa.taa = true;
    let taa_plan = FramePlan::build(taa);
    assert!(taa_plan.is_ok());
    let Ok(taa_plan) = taa_plan else {
        return;
    };
    assert!(
        taa_plan
            .passes()
            .contains(&FramePass::Bloom(PostSource::History))
    );
    assert!(
        taa_plan
            .passes()
            .contains(&FramePass::Exposure(PostSource::History))
    );
    assert!(
        taa_plan
            .passes()
            .contains(&FramePass::Composite(PostSource::History))
    );
    assert_eq!(
        FramePass::Composite(PostSource::History).reads(),
        &[
            FrameTarget::History,
            FrameTarget::Bloom,
            FrameTarget::Exposure
        ]
    );

    let mut no_taa = taa;
    no_taa.taa = false;
    let no_taa_plan = FramePlan::build(no_taa);
    assert!(no_taa_plan.is_ok());
    let Ok(no_taa_plan) = no_taa_plan else {
        return;
    };
    assert!(
        no_taa_plan
            .passes()
            .contains(&FramePass::Bloom(PostSource::Hdr))
    );
    assert!(
        no_taa_plan
            .passes()
            .contains(&FramePass::Exposure(PostSource::Hdr))
    );
    assert!(
        no_taa_plan
            .passes()
            .contains(&FramePass::Composite(PostSource::Hdr))
    );
}

#[test]
fn debug_views_imply_their_low_tier_producers() {
    let mut depth = RenderTier::Low.settings();
    depth.view_mode = ViewMode::Depth;
    let depth_plan = FramePlan::build(depth);
    assert!(depth_plan.is_ok());
    if let Ok(depth_plan) = depth_plan {
        assert!(depth_plan.contains(FramePass::Prepass));
        assert!(!depth_plan.contains(FramePass::Gtao));
    }

    let mut ao = RenderTier::Medium.settings();
    ao.view_mode = ViewMode::AmbientOcclusion;
    let ao_plan = FramePlan::build(ao);
    assert!(ao_plan.is_ok());
    if let Ok(ao_plan) = ao_plan {
        assert!(ao_plan.contains(FramePass::Prepass));
        assert!(ao_plan.contains(FramePass::Gtao));
        assert!(ao_plan.contains(FramePass::GtaoDenoise));
    }
}
