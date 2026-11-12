//! The frame exchange hands whole frames between the simulation and render threads.
//! Threads are joined and signalled over channels; nothing here sleeps.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use app::{AppError, Extractor, RenderEnd, RenderFrame, SimEnd, frame_exchange};
use engine_core::Tick;
use engine_world::{Transform, World};

/// A transform whose translation is `(x, 0, 0)`. Built from the identity so no glam import is needed.
fn at(x: f32) -> Transform {
    let mut transform = Transform::IDENTITY;
    let moved = transform.translation().with_x(x);
    transform
        .set_translation(moved)
        .expect("finite translation");
    transform
}

/// Fills `frame` at `tick` from a world with one entity whose x equals the tick.
fn fill(frame: &mut RenderFrame, tick: u32, extractor: &mut Extractor, world: &mut World) {
    let id = world.net_ids().next().expect("one entity");
    world
        .insert(id, at(f32::from(u16::try_from(tick).expect("small tick"))))
        .expect("insert");
    extractor
        .extract(world, Tick::new(tick), frame)
        .expect("extract");
}

fn one_entity_world() -> (World, Extractor) {
    let mut world = World::new(11).expect("world");
    world.spawn().expect("spawn");
    (world, Extractor::with_capacity(4))
}

#[test]
fn single_thread_cycle_never_blocks() {
    let (mut sim, mut render) = frame_exchange(4);
    let (mut world, mut extractor) = one_entity_world();

    for tick in 0..4 {
        let mut frame = sim.acquire().expect("acquire");
        fill(&mut frame, tick, &mut extractor, &mut world);
        sim.publish(frame).expect("publish");
        let drawn = render.next_frame().expect("next");
        assert_eq!(drawn.tick(), Tick::new(tick));
        render.recycle(drawn).expect("recycle");
    }
}

#[test]
fn two_frames_are_available_back_to_back_and_a_third_waits_for_recycle() {
    let (sim, mut render) = frame_exchange(2);
    let (ready_tx, ready_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let mut sim = sim;
    let first = sim.acquire().expect("first");
    let second = sim.acquire().expect("second");

    let worker = thread::spawn(move || {
        ready_tx.send(()).expect("signal ready");
        let third = sim.acquire().expect("third acquire");
        acquired_tx.send(()).expect("signal acquired");
        third
    });

    ready_rx.recv().expect("worker started");
    // The third acquire cannot succeed before a frame is recycled, so no signal yet.
    assert!(acquired_rx.try_recv().is_err());

    render.recycle(first).expect("recycle first");
    acquired_rx
        .recv()
        .expect("third acquire unblocks after recycle");
    let third = worker.join().expect("worker joined");
    drop(third);
    drop(second);
}

#[test]
fn two_threads_deliver_200_frames_in_order_without_gaps() {
    const FRAMES: u32 = 200;
    let (sim, mut render) = frame_exchange(4);

    let (done_tx, done_rx) = mpsc::channel();
    let simulation = thread::spawn(move || -> Result<(), AppError> {
        let mut sim = sim;
        let (mut world, mut extractor) = one_entity_world();
        for tick in 0..FRAMES {
            let mut frame = sim.acquire()?;
            fill(&mut frame, tick, &mut extractor, &mut world);
            sim.publish(frame)?;
        }
        // Keep the sim end alive until the renderer has recycled the last frame.
        done_rx.recv().expect("render side finished");
        Ok(())
    });

    for expected in 0..FRAMES {
        let frame = render.next_frame().expect("frame");
        assert_eq!(frame.tick(), Tick::new(expected));
        let only = frame.transforms().first().expect("one transform");
        assert_eq!(
            only.current.translation().x,
            f32::from(u16::try_from(expected).expect("small"))
        );
        render.recycle(frame).expect("recycle");
    }
    done_tx.send(()).expect("signal render finished");
    simulation
        .join()
        .expect("simulation joined")
        .expect("simulation ok");
}

#[test]
fn frames_in_flight_never_exceed_two() {
    const FRAMES: u32 = 200;
    let (sim, mut render) = frame_exchange(4);
    let in_flight = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));

    let sim_in_flight = Arc::clone(&in_flight);
    let sim_maximum = Arc::clone(&maximum);
    let (done_tx, done_rx) = mpsc::channel();
    let simulation = thread::spawn(move || -> Result<(), AppError> {
        let mut sim = sim;
        for _ in 0..FRAMES {
            let frame = sim.acquire()?;
            let now = sim_in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            sim_maximum.fetch_max(now, Ordering::SeqCst);
            sim.publish(frame)?;
        }
        // Keep the sim end alive until the renderer has recycled the last frame.
        done_rx.recv().expect("render side finished");
        Ok(())
    });

    for _ in 0..FRAMES {
        let frame = render.next_frame().expect("frame");
        in_flight.fetch_sub(1, Ordering::SeqCst);
        render.recycle(frame).expect("recycle");
    }
    done_tx.send(()).expect("signal render finished");
    simulation
        .join()
        .expect("simulation joined")
        .expect("simulation ok");

    assert!(
        maximum.load(Ordering::SeqCst) <= 2,
        "more than two frames in flight"
    );
}

#[test]
fn dropping_the_render_end_unblocks_a_waiting_acquire_with_closed() {
    let (sim, render) = frame_exchange(2);
    let (ready_tx, ready_rx) = mpsc::channel();

    let worker = thread::spawn(move || -> Result<(), AppError> {
        let mut sim = sim;
        let _first = sim.acquire()?;
        let _second = sim.acquire()?;
        ready_tx.send(()).expect("signal ready");
        // Both frames are held, so this waits until the render end goes away.
        sim.acquire().map(drop)
    });

    ready_rx.recv().expect("worker holds both frames");
    drop(render);
    let outcome = worker.join().expect("worker joined");
    assert!(matches!(outcome, Err(AppError::ExchangeClosed)));
}

#[test]
fn dropping_the_sim_end_still_delivers_published_frames_then_closes() {
    let (mut sim, mut render) = frame_exchange(2);
    let (mut world, mut extractor) = one_entity_world();
    let mut first = sim.acquire().expect("first");
    fill(&mut first, 0, &mut extractor, &mut world);
    let mut second = sim.acquire().expect("second");
    fill(&mut second, 1, &mut extractor, &mut world);
    sim.publish(first).expect("publish first");
    sim.publish(second).expect("publish second");
    drop(sim);

    assert_eq!(
        render.next_frame().expect("published frame 0").tick(),
        Tick::new(0)
    );
    assert_eq!(
        render.next_frame().expect("published frame 1").tick(),
        Tick::new(1)
    );
    assert!(matches!(render.next_frame(), Err(AppError::ExchangeClosed)));
}

#[test]
fn recycle_after_the_sim_end_is_dropped_reports_closed() {
    let (mut sim, mut render) = frame_exchange(2);
    let frame = sim.acquire().expect("acquire");
    drop(sim);

    assert!(matches!(
        render.recycle(frame),
        Err(AppError::ExchangeClosed)
    ));
}

#[test]
fn publish_after_the_render_end_is_dropped_reports_closed() {
    let (mut sim, render): (SimEnd, RenderEnd) = frame_exchange(2);
    let frame = sim.acquire().expect("acquire");
    drop(render);

    assert!(matches!(sim.publish(frame), Err(AppError::ExchangeClosed)));
}
