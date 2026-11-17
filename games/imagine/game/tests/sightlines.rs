//! Checks that the greybox keeps ordinary sightlines short and spawn areas screened.

mod common;

use engine_physics::{PhysicsConfig, PhysicsWorld, RayCast};
use engine_world::World;
use glam::Vec3;
use imagine::data::{CENTRE_ABOVE_FEET_M, EYE_ABOVE_CENTRE_M};

const GRID_STEP_M: i32 = 2;
const RAYS_PER_POINT: u16 = 16;
const MAX_RAY_M: f32 = 100.0;
const EYE_HEIGHT_M: f32 = CENTRE_ABOVE_FEET_M + EYE_ABOVE_CENTRE_M;

#[test]
fn static_sightlines_are_short_and_spawns_cannot_see_each_other()
-> Result<(), Box<dyn std::error::Error>> {
    let content = common::content_pack()?;
    let level = imagine::scene::level_from_content(&content, 0)?;
    let mut world = World::new(1)?;
    world.insert_resource(PhysicsWorld::new(PhysicsConfig::new(
        1.0 / 60.0,
        Vec3::ZERO,
    )?))?;
    engine_scene::register_components(&mut world)?;
    engine_scene::install_level(&mut world, &level)?;
    let physics = world.resource::<PhysicsWorld>()?;
    let mut distances = Vec::new();
    for x in (-28..=28).step_by(usize::try_from(GRID_STEP_M)?) {
        for z in (-28..=28).step_by(usize::try_from(GRID_STEP_M)?) {
            let x = f32::from(i16::try_from(x)?);
            let z = f32::from(i16::try_from(z)?);
            let down = RayCast::new(Vec3::new(x, 10.0, z), -Vec3::Y, 20.0, true)?;
            let Some(floor) = physics.ray_cast(down, imagine::collision::sightline_query()) else {
                continue;
            };
            if floor.normal.y < 0.7 || floor.point.y > 0.1 {
                continue;
            }
            let eye = floor.point + Vec3::Y * EYE_HEIGHT_M;
            for ray in 0..RAYS_PER_POINT {
                let angle = std::f32::consts::TAU * f32::from(ray) / f32::from(RAYS_PER_POINT);
                let direction = Vec3::new(angle.cos(), 0.0, angle.sin());
                let request = RayCast::new(eye, direction, MAX_RAY_M, true)?;
                distances.push(
                    physics
                        .ray_cast(request, imagine::collision::sightline_query())
                        .map_or(MAX_RAY_M, |hit| hit.distance),
                );
            }
        }
    }
    distances.sort_by(f32::total_cmp);
    let ray_count = u32::try_from(distances.len())?;
    let within_forty = u32::try_from(
        distances
            .iter()
            .filter(|distance| **distance <= 40.0)
            .count(),
    )?;
    let median = distances
        .get(distances.len() / 2)
        .copied()
        .ok_or("no walkable sightline samples")?;
    println!(
        "arena sightlines: rays={ray_count}, within_40m={within_forty}, median_free_distance_m={median:.1}"
    );
    assert!(
        within_forty * 100 >= ray_count * 95,
        "fewer than 95% of rays stop within 40 m"
    );
    assert!(
        distances.iter().all(|distance| *distance <= 60.0),
        "a sightline exceeds 60 m"
    );

    let player_spawns: Vec<Vec3> = level
        .spawns()
        .iter()
        .filter(|spawn| spawn.name.starts_with("player_"))
        .map(|spawn| Vec3::from_array(spawn.translation) + Vec3::Y * EYE_HEIGHT_M)
        .collect();
    let bot_spawns: Vec<Vec3> = level
        .spawns()
        .iter()
        .filter(|spawn| spawn.name.starts_with("bot_"))
        .map(|spawn| Vec3::from_array(spawn.translation) + Vec3::Y * EYE_HEIGHT_M)
        .collect();
    for player in &player_spawns {
        for bot in &bot_spawns {
            let delta = *bot - *player;
            let distance = delta.length();
            let request = RayCast::new(*player, delta, distance, true)?;
            let hit = physics.ray_cast(request, imagine::collision::sightline_query());
            assert!(
                hit.is_some(),
                "spawn sightline is open: {player:?} to {bot:?}"
            );
        }
    }
    Ok(())
}
