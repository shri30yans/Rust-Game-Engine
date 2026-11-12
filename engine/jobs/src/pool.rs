//! Fork-join job pool: inline at zero workers, rayon above.

use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use std::sync::Arc;

use crate::JobError;
use crate::chunk::{ChunkSize, chunk_count};

/// Fork-join pool for deterministic chunked work inside a simulation step.
#[derive(Clone)]
pub struct JobPool {
    backend: Backend,
    workers: usize,
}

#[derive(Clone)]
enum Backend {
    Inline,
    Rayon(Arc<rayon::ThreadPool>),
}

impl JobPool {
    /// Builds a pool with `workers` threads. Zero workers runs all work inline on the caller thread (web builds).
    ///
    /// # Errors
    ///
    /// Returns [`JobError::PoolBuild`] when rayon cannot spawn the requested threads.
    pub fn new(workers: usize) -> Result<Self, JobError> {
        if workers == 0 {
            return Ok(Self {
                backend: Backend::Inline,
                workers: 0,
            });
        }

        let pool = ThreadPoolBuilder::new()
            .num_threads(workers)
            .thread_name(|index| format!("job-worker-{index}"))
            .build()
            .map_err(|err| JobError::PoolBuild {
                workers,
                reason: err.to_string(),
            })?;

        Ok(Self {
            backend: Backend::Rayon(Arc::new(pool)),
            workers,
        })
    }

    /// Configured worker count (zero means inline-only).
    #[must_use]
    pub fn workers(&self) -> usize {
        self.workers
    }

    /// Schedules one independent background job on this pool.
    ///
    /// The zero-worker backend runs the job inline, matching the pool's web behavior.
    pub fn spawn(&self, job: impl FnOnce() + Send + 'static) {
        match &self.backend {
            Backend::Inline => job(),
            Backend::Rayon(pool) => pool.spawn(job),
        }
    }

    /// Splits `items` into consecutive chunks of `chunk.get()` elements and calls `f` once per chunk.
    ///
    /// The inline backend runs chunks in index order on the calling thread; the rayon backend runs chunks
    /// in parallel but preserves the same chunk boundaries and indices.
    pub fn for_each_chunk_mut<T, F>(&self, items: &mut [T], chunk: ChunkSize, f: F)
    where
        T: Send,
        F: Fn(usize, &mut [T]) + Sync,
    {
        let n = chunk.get();
        match &self.backend {
            Backend::Inline => {
                for (chunk_index, chunk_items) in items.chunks_mut(n).enumerate() {
                    f(chunk_index, chunk_items);
                }
            }
            Backend::Rayon(pool) => {
                pool.install(|| {
                    items
                        .par_chunks_mut(n)
                        .enumerate()
                        .for_each(|(chunk_index, chunk_items)| {
                            f(chunk_index, chunk_items);
                        });
                });
            }
        }
    }

    /// Maps each chunk to one result slot. The caller combines `out` in index order on one thread.
    ///
    /// # Errors
    ///
    /// Returns [`JobError::OutputLength`] when `out.len()` does not equal [`chunk_count`] for `items`.
    pub fn map_chunks<T, R, F>(
        &self,
        items: &[T],
        chunk: ChunkSize,
        out: &mut [R],
        f: F,
    ) -> Result<(), JobError>
    where
        T: Sync,
        R: Send,
        F: Fn(usize, &[T]) -> R + Sync,
    {
        let expected = chunk_count(items.len(), chunk);
        let got = out.len();
        if got != expected {
            return Err(JobError::OutputLength { expected, got });
        }
        if expected == 0 {
            return Ok(());
        }

        let n = chunk.get();
        // `out.len() == chunk_count(items.len(), chunk)` guarantees the zip below is exact.
        match &self.backend {
            Backend::Inline => {
                for (slot, (index, chunk_items)) in out.iter_mut().zip(items.chunks(n).enumerate())
                {
                    *slot = f(index, chunk_items);
                }
            }
            Backend::Rayon(pool) => {
                pool.install(|| {
                    out.par_iter_mut()
                        .zip(items.par_chunks(n))
                        .enumerate()
                        .for_each(|(index, (slot, chunk_items))| {
                            *slot = f(index, chunk_items);
                        });
                });
            }
        }
        Ok(())
    }

    /// Runs `f` on this pool's rayon workers so nested `par_iter` calls (for example Rapier's `parallel`
    /// feature) share the same thread count instead of the global rayon pool.
    #[must_use]
    pub fn install<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        match &self.backend {
            Backend::Inline => f(),
            Backend::Rayon(pool) => pool.install(f),
        }
    }
}
