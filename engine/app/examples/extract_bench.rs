//! Measures warmed extraction cost for regular 40 × 40 and 80 × 80 scene grids.
use std::time::Instant;

use app::{Extractor, RenderFrame};
use engine_core::Tick;
use engine_scene::{Look, MaterialKind, MeshKind, Visual, register_components};
use engine_world::World;

const WARMUP: u32 = 30;
const SAMPLES: u32 = 240;

fn measure(side: u32) -> Result<(), Box<dyn std::error::Error>> {
    let capacity = usize::try_from(side * side)?;
    let mut world = World::new(1)?;
    register_components(&mut world)?;
    for _ in 0..capacity {
        let id = world.spawn()?;
        world.insert(
            id,
            Visual {
                mesh: MeshKind::Box,
                look: Look {
                    material: MaterialKind::Wall,
                    tint: None,
                },
            },
        )?;
    }
    let mut extractor = Extractor::with_capacity(capacity);
    let mut frame = RenderFrame::with_capacity(capacity);
    for tick in 0..WARMUP {
        extractor.extract(&world, Tick::new(tick), &mut frame)?;
    }
    let mut elapsed = Vec::with_capacity(usize::try_from(SAMPLES)?);
    for tick in WARMUP..WARMUP + SAMPLES {
        let started = Instant::now();
        extractor.extract(&world, Tick::new(tick), &mut frame)?;
        elapsed.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    elapsed.sort_by(f64::total_cmp);
    let p50 = elapsed.get(elapsed.len() / 2).copied().unwrap_or_default();
    let p99 = elapsed
        .get(elapsed.len() * 99 / 100)
        .copied()
        .unwrap_or_default();
    println!("extract-grid{side}: entities={capacity} p50={p50:.4}ms p99={p99:.4}ms");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    measure(40)?;
    measure(80)?;
    Ok(())
}
