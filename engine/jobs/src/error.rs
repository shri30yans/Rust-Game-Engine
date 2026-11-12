//! Errors returned by `engine-jobs`. All failure modes are explicit values, not panics.

use thiserror::Error;

/// Errors from building pools or running chunked work.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JobError {
    /// The rayon worker pool could not be constructed.
    #[error("failed to build the worker pool with {workers} workers: {reason}")]
    PoolBuild {
        /// Requested worker count.
        workers: usize,
        /// Reason from the thread pool builder.
        reason: String,
    },
    /// `ChunkSize` must be at least one item.
    #[error("chunk size must be at least 1")]
    ZeroChunkSize,
    /// The output buffer does not hold one slot per chunk.
    #[error("output slice has {got} slots but {expected} chunks need slots")]
    OutputLength {
        /// Required number of chunk result slots.
        expected: usize,
        /// Length of the caller-supplied output slice.
        got: usize,
    },
    /// A background worker ended without sending a result (panic or dropped sender).
    #[error("the background task ended without a result")]
    TaskLost,
    /// `try_take` or `wait` was called after the result was already consumed.
    #[error("the task result was already taken")]
    TaskTaken,
}
