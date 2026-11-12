//! Client logic without a GPU: fixed ticks, extraction, the alpha weight and the blend between
//! the last two ticks, driven through `ClientApp::simulate`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use app::{
    AppError, ClientApp, ClientAssets, ExtractedTransform, Presenter, RenderFrame, ViewAngles,
};
use common::{Mover, config, profile};
use platform::Input;
use proptest::prelude::*;
use render::{Camera, Gfx, Settings};

/// A presenter for logic tests. Drawing needs a GPU, so none of its methods do any work.
struct Silent;

impl Presenter for Silent {
    type Command = ();
    type Settings = ();

    fn setup(&mut self, _gfx: &mut Gfx, _assets: &mut ClientAssets<'_>) -> Result<(), AppError> {
        Ok(())
    }

    fn command(&mut self, _input: &Input, _look: ViewAngles) -> Result<Self::Command, AppError> {
        Ok(())
    }

    fn grab_mouse(&self) -> bool {
        false
    }

    fn camera(
        &self,
        _frame: &RenderFrame,
        _alpha: f32,
        _look: ViewAngles,
    ) -> Result<Camera, AppError> {
        Ok(Camera::default())
    }

    fn draw(
        &mut self,
        _frame: &RenderFrame,
        _alpha: f32,
        _gfx: &mut Gfx,
        _assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        Ok(())
    }
}

/// The only body's transform entry in the newest frame.
fn body(client: &ClientApp<Mover, Silent>) -> ExtractedTransform {
    client
        .frame()
        .transforms()
        .first()
        .copied()
        .expect("one body")
}

fn client() -> ClientApp<Mover, Silent> {
    ClientApp::new(
        Mover,
        &(),
        Silent,
        &config(),
        &profile(),
        Settings::default(),
    )
    .expect("client builds")
}

#[test]
fn a_tiny_frame_runs_no_tick_but_still_extracts_once() {
    let mut client = client();
    let alpha = client.simulate(0.001).expect("simulate");
    assert_eq!(client.app().tick().raw(), 0);
    assert!((0.0..1.0).contains(&alpha));
    assert_eq!(client.frame().transforms().len(), 1);
    let body = body(&client);
    assert!(body.current.translation().x.abs() < 1e-6);
}

#[test]
fn one_tick_moves_the_body_one_metre_and_the_blend_lies_between() {
    let mut client = client();
    client.simulate(0.0).expect("first extract");
    let alpha = client.simulate(0.02).expect("simulate");
    assert_eq!(client.app().tick().raw(), 1);

    let body = body(&client);
    assert!(body.previous.translation().x.abs() < 1e-6);
    assert!((body.current.translation().x - 1.0).abs() < 1e-6);
    // 20 ms owes one 16.67 ms tick; the 3.33 ms left over is the weight of the next tick.
    assert!((alpha - 0.2).abs() < 1e-3, "alpha was {alpha}");

    let mid = client
        .frame()
        .blended(body.id, alpha)
        .expect("alpha in range")
        .expect("body present");
    assert!((mid.translation().x - alpha).abs() < 1e-5);
    assert!(mid.translation().x > 0.0 && mid.translation().x < 1.0);
}

#[test]
fn a_long_frame_runs_only_the_catch_up_cap() {
    let mut client = client();
    client.simulate(0.0).expect("first extract");
    let alpha = client.simulate(10.0).expect("simulate");
    assert_eq!(client.app().tick().raw(), 8);
    assert!((0.0..1.0).contains(&alpha));
    let body = body(&client);
    assert!((body.current.translation().x - 8.0).abs() < 1e-5);
    assert!(body.previous.translation().x.abs() < 1e-6);
}

#[test]
fn negative_and_non_finite_frame_times_are_errors_and_run_nothing() {
    let mut client = client();
    for bad in [-1.0, f32::NAN, f32::INFINITY] {
        assert!(
            matches!(client.simulate(bad), Err(AppError::ClockRange)),
            "dt {bad} must be rejected"
        );
    }
    assert_eq!(client.app().tick().raw(), 0);
}

#[test]
fn the_frame_capacity_does_not_change_after_warm_up() {
    let mut client = client();
    client.simulate(0.0).expect("warm-up");
    let before = client.frame().capacity();
    for _ in 0..100 {
        client.simulate(0.02).expect("simulate");
    }
    assert_eq!(client.frame().capacity(), before);
    assert_eq!(client.frame().transforms().len(), 1);
}

#[test]
fn initial_frame_capacity_is_loaded_entity_count_plus_game_budget() {
    let client = client();
    assert_eq!(client.frame().capacity(), 4);
}

proptest! {
    #[test]
    fn alpha_stays_in_range_and_the_blend_stays_between_the_ticks(
        frame_times in prop::collection::vec(0.0f32..0.5, 1..20),
    ) {
        let mut client = client();
        for dt in frame_times {
            let alpha = client.simulate(dt).expect("simulate");
            prop_assert!((0.0..1.0).contains(&alpha));
            let body = client.frame().transforms().first().copied().expect("one body");
            let mid = client
                .frame()
                .blended(body.id, alpha)
                .expect("alpha in range")
                .expect("body present");
            let (low, high) = (
                body.previous.translation().x.min(body.current.translation().x),
                body.previous.translation().x.max(body.current.translation().x),
            );
            prop_assert!(mid.translation().x >= low - 1e-4);
            prop_assert!(mid.translation().x <= high + 1e-4);
        }
    }
}
