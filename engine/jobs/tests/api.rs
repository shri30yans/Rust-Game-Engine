//! Public API behaviour and error paths.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use jobs::{BackgroundPool, ChunkSize, JobError, JobPool, chunk_count};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

#[test]
fn chunk_size_zero_errors() {
    let err = ChunkSize::new(0).unwrap_err();
    assert_eq!(err, JobError::ZeroChunkSize);
}

#[test]
fn chunk_count_cases() {
    let one = ChunkSize::new(10).unwrap();
    assert_eq!(chunk_count(0, one), 0);
    assert_eq!(chunk_count(1, one), 1);
    assert_eq!(chunk_count(10, one), 1);
    assert_eq!(chunk_count(11, one), 2);
    assert_eq!(chunk_count(20, one), 2);
    assert_eq!(chunk_count(21, one), 3);
}

#[test]
fn map_chunks_wrong_output_length() {
    let pool = JobPool::new(1).unwrap();
    let chunk = ChunkSize::new(3).unwrap();
    let items = [1_u32, 2, 3, 4, 5];
    let mut out = [0_u32; 1];
    let err = pool
        .map_chunks(&items, chunk, &mut out, |_, slice| slice.iter().sum())
        .unwrap_err();
    assert_eq!(
        err,
        JobError::OutputLength {
            expected: 2,
            got: 1
        }
    );
}

#[test]
fn map_chunks_empty_input_ok() {
    let pool = JobPool::new(2).unwrap();
    let chunk = ChunkSize::new(4).unwrap();
    let items: [u32; 0] = [];
    let mut out: [u32; 0] = [];
    pool.map_chunks(&items, chunk, &mut out, |_, _| 0).unwrap();
}

#[test]
fn install_returns_value_and_uses_pool_threads() {
    let inline = JobPool::new(0).unwrap();
    assert_eq!(inline.install(|| 42), 42);

    let pool = JobPool::new(3).unwrap();
    assert_eq!(pool.workers(), 3);
    let threads = pool.install(rayon::current_num_threads);
    assert_eq!(threads, 3);
}

#[test]
fn job_pool_workers_reported() {
    assert_eq!(JobPool::new(0).unwrap().workers(), 0);
    assert_eq!(JobPool::new(3).unwrap().workers(), 3);
}

#[test]
fn background_wait_returns_result() {
    let pool = BackgroundPool::new(2).unwrap();
    let task = pool.spawn(|| 7_u32);
    assert_eq!(task.wait().unwrap(), 7);
}

#[test]
fn background_try_take_before_and_after_completion() {
    let pool = BackgroundPool::new(1).unwrap();
    let (release_tx, release_rx) = mpsc::channel::<()>();

    let task = pool.spawn(move || {
        let _ = release_rx.recv();
        99_u32
    });

    let mut task = task;
    assert_eq!(task.try_take().unwrap(), None);

    release_tx.send(()).unwrap();
    let mut got = None;
    for _ in 0..10_000 {
        match task.try_take() {
            Ok(Some(v)) => {
                got = Some(v);
                break;
            }
            Ok(None) => std::thread::yield_now(),
            Err(err) => panic!("unexpected error: {err:?}"),
        }
    }
    assert_eq!(got, Some(99));
}

#[test]
fn background_panic_yields_task_lost_and_pool_keeps_serving() {
    let pool = BackgroundPool::new(1).unwrap();
    let task = pool.spawn(|| {
        panic!("job failed");
    });
    assert_eq!(task.wait(), Err(JobError::TaskLost));

    let follow_up = pool.spawn(|| "ok");
    assert_eq!(follow_up.wait().unwrap(), "ok");
}

#[test]
fn background_zero_workers_runs_inline_before_spawn_returns() {
    let pool = BackgroundPool::new(0).unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let ran_flag = Arc::clone(&ran);
    let task = pool.spawn(move || {
        ran_flag.store(true, Ordering::SeqCst);
        1_u8
    });
    assert!(ran.load(Ordering::SeqCst));
    assert_eq!(task.wait().unwrap(), 1);
}

#[test]
fn dropping_background_pool_with_queued_jobs_returns() {
    let pool = BackgroundPool::new(1).unwrap();
    let (start_tx, start_rx) = mpsc::channel::<()>();
    let (done_tx, done_rx) = mpsc::channel::<()>();

    let _task = pool.spawn(move || {
        let _ = start_rx.recv();
        let _ = done_tx.send(());
    });

    start_tx.send(()).unwrap();
    let _ = done_rx.recv();

    for _ in 0..32 {
        let _queued = pool.spawn(|| ());
    }

    drop(pool);
}

#[test]
fn try_take_after_take_errors_task_taken() {
    let pool = BackgroundPool::new(0).unwrap();
    let mut task = pool.spawn(|| 5_i32);
    assert_eq!(task.try_take().unwrap(), Some(5));
    assert_eq!(task.try_take(), Err(JobError::TaskTaken));
}

#[test]
fn map_chunks_with_huge_chunk_size_does_not_overflow() {
    let chunk = ChunkSize::new(usize::MAX).unwrap();
    let items = [1_u32, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let mut out = [0_u32; 1];

    for workers in [0_usize, 3] {
        let pool = JobPool::new(workers).unwrap();
        assert_eq!(chunk_count(items.len(), chunk), 1);
        pool.map_chunks(&items, chunk, &mut out, |index, chunk_items| {
            assert_eq!(index, 0);
            chunk_items.iter().sum::<u32>()
        })
        .unwrap();
        assert_eq!(out[0], 55);
    }
}

#[test]
fn try_take_twice_reports_taken() {
    let pool = BackgroundPool::new(0).unwrap();
    let mut task = pool.spawn(|| 12_i64);
    assert_eq!(task.try_take().unwrap(), Some(12));
    assert_eq!(task.try_take(), Err(JobError::TaskTaken));
    assert_eq!(task.try_take(), Err(JobError::TaskTaken));
}

#[test]
fn task_dropped_before_finish_does_not_hang_pool() {
    let pool = BackgroundPool::new(1).unwrap();
    let (release_tx, release_rx) = mpsc::channel::<()>();

    {
        let _task = pool.spawn(move || {
            let _ = release_rx.recv();
            1_u32
        });
    }

    release_tx.send(()).unwrap();
    let follow_up = pool.spawn(|| 2_u32);
    assert_eq!(follow_up.wait().unwrap(), 2);
}
