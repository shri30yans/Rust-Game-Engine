//! Background worker pool for file loading and other off-frame work.
//!
//! Frame code never waits on this pool. The `Mutex` around the job receiver is not simulation state:
//! only background worker threads contend on it.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::JobError;

type Job = Box<dyn FnOnce() + Send>;

/// Lower-priority pool for loading, decompression and shader builds. Not used on the simulation hot path.
pub struct BackgroundPool {
    job_tx: Option<Sender<Job>>,
    handles: Vec<JoinHandle<()>>,
}

impl BackgroundPool {
    /// Builds a background pool with `workers` threads. Zero workers runs every job inline inside
    /// [`spawn`](Self::spawn) (web builds with no OS threads).
    ///
    /// # Errors
    ///
    /// Returns [`JobError::PoolBuild`] when a worker thread cannot be spawned.
    pub fn new(workers: usize) -> Result<Self, JobError> {
        if workers == 0 {
            return Ok(Self {
                job_tx: None,
                handles: Vec::new(),
            });
        }

        let (job_tx, job_rx) = mpsc::channel::<Job>();
        let shared_rx = Arc::new(Mutex::new(job_rx));
        let mut handles = Vec::with_capacity(workers);

        for index in 0..workers {
            let receiver = Arc::clone(&shared_rx);
            let handle = thread::Builder::new()
                .name(format!("background-{index}"))
                .spawn(move || worker_loop(receiver))
                .map_err(|err| JobError::PoolBuild {
                    workers,
                    reason: err.to_string(),
                })?;
            handles.push(handle);
        }

        Ok(Self {
            job_tx: Some(job_tx),
            handles,
        })
    }

    /// Queues `job` on a background thread, or runs it inline when the pool has zero workers.
    pub fn spawn<T: Send + 'static>(&self, job: impl FnOnce() -> T + Send + 'static) -> Task<T> {
        let (result_tx, result_rx) = mpsc::channel();

        let job_boxed: Job = Box::new(move || {
            let outcome = catch_unwind(AssertUnwindSafe(job));
            match outcome {
                Ok(value) => {
                    // Task was dropped before the result arrived; discard the send error.
                    drop(result_tx.send(value));
                }
                Err(_) => drop(result_tx),
            }
        });

        match self.job_tx.as_ref() {
            Some(job_tx) => {
                // If the pool is shut down, `job_boxed` drops here and closes `result_tx` → `TaskLost`.
                drop(job_tx.send(job_boxed));
            }
            None => job_boxed(),
        }
        Task::new(result_rx)
    }
}

impl Drop for BackgroundPool {
    fn drop(&mut self) {
        drop(self.job_tx.take());
        for handle in self.handles.drain(..) {
            // Every job runs inside `catch_unwind`; `join` errors are unreachable.
            drop(handle.join());
        }
    }
}

fn worker_loop(receiver: Arc<Mutex<Receiver<Job>>>) {
    loop {
        let Some(job) = recv_job(&receiver) else {
            break;
        };
        job();
    }
}

fn recv_job(receiver: &Arc<Mutex<Receiver<Job>>>) -> Option<Job> {
    let guard = match receiver.lock() {
        Ok(guard) => guard,
        // The mutex only serialises `recv`; a poisoned lock carries no broken job-queue invariant.
        Err(poisoned) => poisoned.into_inner(),
    };
    let Ok(job) = guard.recv() else {
        return None;
    };
    Some(job)
}

/// Handle for a single background job's result. Poll with [`try_take`](Self::try_take) or block in tests with [`wait`](Self::wait).
pub struct Task<T> {
    receiver: Receiver<T>,
    taken: bool,
}

impl<T> Task<T> {
    fn new(receiver: Receiver<T>) -> Self {
        Self {
            receiver,
            taken: false,
        }
    }

    /// Non-blocking poll: [`Ok(None)`] while running, [`Ok(Some(value))`] once, then [`JobError::TaskTaken`].
    ///
    /// # Errors
    ///
    /// Returns [`JobError::TaskLost`] if the worker dropped the result without sending.
    pub fn try_take(&mut self) -> Result<Option<T>, JobError> {
        if self.taken {
            return Err(JobError::TaskTaken);
        }
        match self.receiver.try_recv() {
            Ok(value) => {
                self.taken = true;
                Ok(Some(value))
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(JobError::TaskLost),
        }
    }

    /// Blocks until the result is ready (tests and tools).
    ///
    /// # Errors
    ///
    /// Same as [`try_take`](Self::try_take) for lost or already-taken results.
    pub fn wait(self) -> Result<T, JobError> {
        if self.taken {
            return Err(JobError::TaskTaken);
        }
        match self.receiver.recv() {
            Ok(value) => Ok(value),
            Err(mpsc::RecvError) => Err(JobError::TaskLost),
        }
    }
}
