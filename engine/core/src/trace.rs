//! Tracing subscriber setup for binaries and tests.
//!
//! The subscriber is process-wide by nature of `tracing`, so this is the only
//! global state in engine-core. It is installed at most once: the first result
//! is cached and returned by every later call, so repeated calls are safe.

use std::sync::OnceLock;

use tracing_subscriber::EnvFilter;

use crate::error::CoreError;

/// Cached outcome of the first installation attempt.
static INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// Installs a formatting tracing subscriber that honours `RUST_LOG` (default
/// level `info`). Safe to call repeatedly; every call returns the outcome of
/// the first attempt.
///
/// # Errors
/// Returns [`CoreError::TracingInit`] if the global subscriber could not be
/// installed, for example because another subscriber was set first.
pub fn init_tracing() -> Result<(), CoreError> {
    INIT.get_or_init(|| {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .try_init()
            .map_err(|e| e.to_string())
    })
    .clone()
    .map_err(CoreError::TracingInit)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        let first = init_tracing();
        let second = init_tracing();
        assert_eq!(first, second);
        assert!(init_tracing().is_ok() || first.is_err());
    }
}
