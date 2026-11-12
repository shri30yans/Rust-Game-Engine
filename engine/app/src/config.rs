//! Start-up settings for an [`App`](crate::App).
//!
//! Plain data with public fields. Validation happens where the values are used:
//! the tick rate and catch-up cap in [`FixedClock::new`](crate::FixedClock::new).

use crate::clock::TickRate;

/// Settings the game or host chooses before the app is built.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Fixed simulation rate.
    pub tick_rate: TickRate,
    /// Seed of the world's root RNG. The same seed and inputs give the same state hashes.
    pub seed: u64,
    /// Most ticks one `advance` call may run; the rest of the owed time is dropped.
    pub max_catch_up_ticks: u32,
    /// Overrides the device profile's job worker count (phones can report core counts wrong).
    /// `None` uses the profile.
    pub job_workers: Option<usize>,
}
