//! Ordered chunked parallelism and a background worker pool.
//!
//! `engine-jobs` is the only engine crate that names rayon. Simulation crates split work into
//! fixed-size chunks, write one result slot per chunk, and combine slots in index order on one thread.
//!
//! Invariants: no global rayon pool; zero job-pool workers run fork-join work inline; chunk size is
//! never derived from the worker count; this crate does not reduce floats across threads.
#![forbid(unsafe_code)]

mod background;
mod chunk;
mod error;
mod pool;

pub use background::{BackgroundPool, Task};
pub use chunk::{ChunkSize, chunk_count};
pub use error::JobError;
pub use pool::JobPool;
