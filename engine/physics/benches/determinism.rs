//! Measures one fixed match-scale physics workload and its checkpoint hashes.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::time::{Duration, Instant};

use criterion::{Criterion, black_box};
use engine_physics::{
    BodyBuilder, CharacterConfig, CharacterController, CharacterInput, CharacterState,
    ColliderBuilder, ConvexShape, PhysicsConfig, PhysicsWorld,
};
use glam::Vec3;

const CHARACTER_COUNT: u16 = 24;
const DEBRIS_COUNT: u16 = 300;
const WALL_COUNT_PER_SIDE: u16 = 50;
const STEP_COUNT: usize = 3_600;
const CHECKPOINTS: [usize; 3] = [600, 1_800, 3_600];
const CHARACTER_SPEED: f32 = 3.0;
const BOX_HALF_SIZE: f32 = 0.5;

struct RunReport {
    step_times: Vec<Duration>,
    hashes: [u64; 3],
}

fn run_match_scene() -> RunReport {
    let mut world = PhysicsWorld::new(PhysicsConfig::default());
    world
        .insert_collider(
            None,
            ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(26.0, 0.5, 26.0)).unwrap())
                .translation(Vec3::new(0.0, -0.5, 0.0))
                .unwrap(),
        )
        .unwrap();

    add_walls(&mut world);
    add_debris(&mut world);
    let (controllers, mut states, bodies) = add_characters(&mut world);
    let inputs = scripted_inputs();
    let mut step_times = Vec::with_capacity(STEP_COUNT);
    let mut hashes = [0; CHECKPOINTS.len()];

    for tick in 0..STEP_COUNT {
        let started = Instant::now();
        world.step();
        for (character_index, state) in states.iter_mut().enumerate() {
            let controller = controllers.get(character_index).unwrap();
            let body = bodies.get(character_index).unwrap();
            let input_index = (tick / 180 + character_index) % inputs.len();
            let movement =
                controller.move_character(*state, *inputs.get(input_index).unwrap(), &world, *body);
            *state = movement.state;
            world
                .set_next_kinematic_pose(*body, state.position(), glam::Quat::IDENTITY)
                .unwrap();
        }
        step_times.push(started.elapsed());
        if let Some(checkpoint_index) = CHECKPOINTS
            .iter()
            .position(|checkpoint| *checkpoint == tick + 1)
            && let Some(hash) = hashes.get_mut(checkpoint_index)
        {
            *hash = world.state_hash();
        }
    }

    RunReport { step_times, hashes }
}

fn add_walls(world: &mut PhysicsWorld) {
    for offset in 0..WALL_COUNT_PER_SIDE {
        let position = f32::from(offset) - f32::from(WALL_COUNT_PER_SIDE) / 2.0 + 0.5;
        for (x, z) in [
            (25.0, position),
            (-25.0, position),
            (position, 25.0),
            (position, -25.0),
        ] {
            world
                .insert_collider(
                    None,
                    ColliderBuilder::new(ConvexShape::cuboid(Vec3::new(0.5, 2.0, 0.5)).unwrap())
                        .translation(Vec3::new(x, 2.0, z))
                        .unwrap(),
                )
                .unwrap();
        }
    }
}

fn add_debris(world: &mut PhysicsWorld) {
    for index in 0..DEBRIS_COUNT {
        let column = index % 10;
        let row = index / 10;
        let x = f32::from(column) * 1.1 - 5.0;
        let y = f32::from(row / 10) * 1.1 + 2.0;
        let z = f32::from(row % 10) * 1.1 - 5.0;
        let body = world.insert_body(
            BodyBuilder::dynamic()
                .translation(Vec3::new(x, y, z))
                .unwrap(),
        );
        world
            .insert_collider(
                Some(body),
                ColliderBuilder::new(ConvexShape::cuboid(Vec3::splat(BOX_HALF_SIZE)).unwrap()),
            )
            .unwrap();
    }
}

fn add_characters(
    world: &mut PhysicsWorld,
) -> (
    Vec<CharacterController>,
    Vec<CharacterState>,
    Vec<engine_physics::BodyId>,
) {
    let mut controllers = Vec::with_capacity(usize::from(CHARACTER_COUNT));
    let mut states = Vec::with_capacity(usize::from(CHARACTER_COUNT));
    let mut bodies = Vec::with_capacity(usize::from(CHARACTER_COUNT));
    let config = CharacterConfig::capsule(0.5, 0.4).unwrap();

    for index in 0..CHARACTER_COUNT {
        let lane = index % 6;
        let row = index / 6;
        let position = Vec3::new(f32::from(lane) * 2.0 - 5.0, 1.0, f32::from(row) * 3.0 + 9.0);
        let body = world.insert_body(BodyBuilder::kinematic().translation(position).unwrap());
        world
            .insert_collider(Some(body), ColliderBuilder::new(config.shape()))
            .unwrap();
        controllers.push(CharacterController::new(config));
        states.push(CharacterState::new(position).unwrap());
        bodies.push(body);
    }
    (controllers, states, bodies)
}

fn scripted_inputs() -> [CharacterInput; 4] {
    [
        CharacterInput::new(Vec3::new(CHARACTER_SPEED, 0.0, 0.0), false).unwrap(),
        CharacterInput::new(Vec3::new(0.0, 0.0, CHARACTER_SPEED), false).unwrap(),
        CharacterInput::new(Vec3::new(-CHARACTER_SPEED, 0.0, 0.0), false).unwrap(),
        CharacterInput::new(Vec3::new(0.0, 0.0, -CHARACTER_SPEED), false).unwrap(),
    ]
}

fn report(c: &mut Criterion) {
    c.bench_function("physics_match_scene_3600_steps", |b| {
        b.iter_custom(|_| {
            let report = black_box(run_match_scene());
            print_report(&report);
            report.step_times.iter().copied().sum()
        });
    });
}

fn print_report(report: &RunReport) {
    let mut sorted = report.step_times.clone();
    sorted.sort_unstable();
    let total = report.step_times.iter().copied().sum::<Duration>();
    let step_count = f64::from(u32::try_from(STEP_COUNT).unwrap());
    let mean_ms = total.as_secs_f64() * 1_000.0 / step_count;
    let p50_ms = percentile_ms(&sorted, 50);
    let p99_ms = percentile_ms(&sorted, 99);
    let mode = if cfg!(feature = "simd") {
        "simd"
    } else if cfg!(feature = "cross-platform-determinism") {
        "cross-platform-determinism"
    } else {
        "scalar"
    };
    let [hash_600, hash_1_800, hash_3_600] = report.hashes;

    println!(
        "MODE={mode} STEPS={STEP_COUNT} MEAN_STEP_MS={mean_ms:.6} P50_STEP_MS={p50_ms:.6} P99_STEP_MS={p99_ms:.6} TOTAL_STEP_TIME_MS={:.3} HASHES={:016x},{:016x},{:016x}",
        total.as_secs_f64() * 1_000.0,
        hash_600,
        hash_1_800,
        hash_3_600,
    );
}

fn percentile_ms(sorted: &[Duration], percentile: usize) -> f64 {
    let last = sorted.len().saturating_sub(1);
    let rank = last.saturating_mul(percentile) / 100;
    sorted.get(rank).copied().unwrap_or_default().as_secs_f64() * 1_000.0
}

fn main() {
    if std::env::args().any(|argument| argument == "--report-once") {
        print_report(&run_match_scene());
        return;
    }

    let mut criterion = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(1));
    report(&mut criterion);
    criterion.final_summary();
}
