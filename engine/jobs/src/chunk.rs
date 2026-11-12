//! Fixed chunk sizing for deterministic parallel splits.

use std::num::NonZeroUsize;

use crate::JobError;

/// Fixed chunk length in items. Never derived from the worker count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkSize(NonZeroUsize);

impl ChunkSize {
    /// Builds a chunk size of `items` elements per chunk.
    ///
    /// # Errors
    ///
    /// Returns [`JobError::ZeroChunkSize`] when `items` is zero.
    pub fn new(items: usize) -> Result<Self, JobError> {
        let Some(nonzero) = NonZeroUsize::new(items) else {
            return Err(JobError::ZeroChunkSize);
        };
        Ok(Self(nonzero))
    }

    /// Number of items in each full chunk.
    #[must_use]
    pub fn get(self) -> usize {
        self.0.get()
    }
}

/// Number of chunks `len` items split into: `ceil(len / chunk)`. Pure; uses `len.div_ceil(chunk)`.
#[must_use]
pub fn chunk_count(len: usize, chunk: ChunkSize) -> usize {
    len.div_ceil(chunk.get())
}
