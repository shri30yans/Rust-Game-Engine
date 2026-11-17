//! Touch input on menu screens reaches the retained menu runtime without installing match buttons.

use glam::Vec2;
use imagine::ImagineEntry;
use imagine::rules::PLAYER_SLOT;
use imagine::sim::actor_id;
use kit_fps::Locomotion;
use platform::Input;
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;

fn front_end() -> Result<
    (
        app::ClientApp<imagine::ImagineGame, imagine::ArenaPresenter>,
        Input,
    ),
    Box<dyn Error>,
> {
    let mut client = app::headless::start::<ImagineEntry>()?;
    let mut input = Input::default();
    input.screen = Vec2::new(1600.0, 900.0);
    client.prepare_input(&mut input)?;
    assert!(input.touch_regions().is_empty());
    Ok((client, input))
}

#[test]
fn touch_on_the_front_end_does_not_send_a_match_command() -> TestResult {
    let (mut client, mut input) = front_end()?;
    input.touch_start(1, Vec2::new(1376.0, 540.0));
    client.prepare_input(&mut input)?;
    client.simulate(1.0 / 60.0)?;
    let player = actor_id(client.app().world(), PLAYER_SLOT)?;
    let command = client.app().world().get::<Locomotion>(player)?.command();
    assert!(!command.fire());
    assert!(!command.jump_pressed());
    assert!(!command.reload());
    Ok(())
}

#[test]
fn raw_touch_is_kept_for_menu_hit_testing() -> TestResult {
    let (mut client, mut input) = front_end()?;
    let touch_position = Vec2::new(900.0, 450.0);
    input.touch_start(1, touch_position);
    client.prepare_input(&mut input)?;
    assert_eq!(input.touches().len(), 1);
    assert_eq!(
        input.touches().first().map(|touch| touch.pos),
        Some(touch_position)
    );
    Ok(())
}
