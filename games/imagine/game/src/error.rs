//! Error plumbing for the game: maps a failure from an engine or kit crate into a world error
//! without losing it, so the schedule reports the original error as the source.

use engine_world::WorldError;

/// Wraps an error from a crate above the world (physics, for example) as a world error. The
/// original error is kept whole as the source.
#[must_use]
pub fn downstream<E>(error: E) -> WorldError
where
    E: std::error::Error + Send + Sync + 'static,
{
    WorldError::Downstream {
        source: Box::new(error),
    }
}
