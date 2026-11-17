//! Auto fire is a weapon-side trigger policy resolved during the authoritative tick.

mod common;

use app::ViewAngles;
use common::{Arena, TestResult, actor_count, aim_pitch, aim_yaw, centre_of, eye_of};
use glam::Vec2;
use imagine::rules::PLAYER_SLOT;
use imagine::sim::actor_id;
use kit_fps::{MoveCommand, Trigger, Weapon};

const SEED: u64 = 7;

#[test]
fn on_target_consumes_ammunition_in_the_same_tick_as_the_command() -> TestResult {
    let mut arena = Arena::new(SEED)?;
    let player = actor_id(&arena.world, PLAYER_SLOT)?;
    let initial_ammo = arena.world.get::<Weapon>(player)?.state().ammo();
    let eye = eye_of(&arena.world, PLAYER_SLOT)?;
    let mut fired = false;

    let actor_count = actor_count(&arena.world)?;
    for tick in 0..600_usize {
        let slot = 1 + tick % actor_count.saturating_sub(1);
        let target = centre_of(&arena.world, slot)?;
        let view = ViewAngles::new(aim_yaw(eye, target), aim_pitch(eye, target))?;
        let command = MoveCommand::new(Vec2::ZERO, view, false, false, Trigger::OnTarget, false)?;
        arena.command_player(command)?;
        arena.step()?;
        let ammo = arena.world.get::<Weapon>(player)?.state().ammo();
        if ammo < initial_ammo {
            fired = true;
            break;
        }
    }

    assert!(fired, "an aimed living enemy should fire immediately");
    Ok(())
}

#[test]
fn on_target_does_not_fire_into_empty_air() -> TestResult {
    let mut arena = Arena::new(SEED)?;
    let player = actor_id(&arena.world, PLAYER_SLOT)?;
    let initial_ammo = arena.world.get::<Weapon>(player)?.state().ammo();
    let command = MoveCommand::new(
        Vec2::ZERO,
        ViewAngles::new(0.0, 1.4)?,
        false,
        false,
        Trigger::OnTarget,
        false,
    )?;
    arena.command_player(command)?;
    arena.step()?;
    assert_eq!(
        arena.world.get::<Weapon>(player)?.state().ammo(),
        initial_ammo
    );
    Ok(())
}
