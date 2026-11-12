//! The mixer: a Kira-backed `Audio` that routes one-shots to buses and bounds the voice count.
//!
//! `output` owns the manager and its public surface, `buses` holds volume and routing, `spatial`
//! installs the bank and its pooled spatial tracks, and `playback` admits one-shots as voices.

mod buses;
mod output;
mod playback;
mod spatial;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests;

pub use output::{Audio, DEFAULT_GLOBAL_VOICE_LIMIT, new_mock};
