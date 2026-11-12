//! Engine-owned live scene updates split into comparison, application, and transactions.

mod apply;
mod diff;
mod state;
mod transaction;

#[cfg(test)]
mod tests;

pub use apply::apply_level_diff;
pub use diff::{LevelDiff, diff_levels};
pub use state::LiveScene;
