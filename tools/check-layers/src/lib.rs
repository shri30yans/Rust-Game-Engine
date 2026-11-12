//! `check-layers`: CI gate for numeric layers and folder zones (engine and kits never depend
//! on a game; games never depend on each other).
//!
//! The judgement is pure in [`rules`], [`check`] and [`scan::find_forbidden`], so
//! it is unit-tested with fixture metadata and files. [`metadata`] runs `cargo`;
//! [`scan::scan_zone_sources`] reads crate sources; [`run`] glues the pieces.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

pub mod check;
pub mod metadata;
pub mod rules;
pub mod scan;

use std::fs;
use std::path::{Path, PathBuf};

pub use check::{CheckError, Violation, find_violations};
pub use rules::{ConfigError, LayerConfig};
pub use scan::{find_forbidden, scan_zone_sources};

/// Failures that stop the check before it can judge the workspace graph.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The layer table could not be read from disk.
    #[error("cannot read layer table {}: {source}", path.display())]
    ReadConfig {
        /// Path that was read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The layer table is not valid JSON for [`LayerConfig`].
    #[error("invalid layer table {}: {source}", path.display())]
    ParseConfig {
        /// Path that was parsed.
        path: PathBuf,
        /// Underlying JSON error.
        source: serde_json::Error,
    },
    /// The layer table failed semantic validation.
    #[error("invalid layer table {}: {source}", path.display())]
    InvalidConfig {
        /// Path that was validated.
        path: PathBuf,
        /// What was wrong with the table.
        #[source]
        source: ConfigError,
    },
    /// A crate source tree could not be read.
    #[error("cannot read sources under {}: {source}", path.display())]
    ReadSources {
        /// Directory being walked.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// `cargo` could not be started.
    #[error("cannot run `cargo metadata`: {0}")]
    Spawn(#[source] std::io::Error),
    /// `cargo metadata` exited with a failure status.
    #[error("`cargo metadata` failed: {0}")]
    CargoMetadata(String),
    /// The output of `cargo metadata` did not match the expected shape.
    #[error("cannot parse `cargo metadata` output: {0}")]
    ParseMetadata(#[source] serde_json::Error),
    /// Workspace paths from metadata or scanning could not be resolved.
    #[error(transparent)]
    Check(#[from] CheckError),
}

/// Reads the layer table at `config_path`, asks cargo for the workspace
/// graph (from the current directory), and returns every violation found.
///
/// An empty vector means the graph obeys the layer table.
pub fn run(config_path: &Path) -> Result<Vec<Violation>, Error> {
    let text = fs::read_to_string(config_path).map_err(|source| Error::ReadConfig {
        path: config_path.to_path_buf(),
        source,
    })?;
    let config = LayerConfig::parse(&text).map_err(|source| Error::ParseConfig {
        path: config_path.to_path_buf(),
        source,
    })?;
    config.validate().map_err(|source| Error::InvalidConfig {
        path: config_path.to_path_buf(),
        source,
    })?;
    let metadata = metadata::load()?;
    let mut violations = find_violations(&metadata, &config)?;
    let scan_crates: Vec<(String, rules::ZoneName, Vec<String>)> =
        check::crate_zones_for_scan(&metadata, &config)?
            .into_iter()
            .map(|entry| (entry.dir, entry.zone, entry.forbid_text))
            .collect();
    let sources = scan_zone_sources(&metadata.workspace_root, &scan_crates)?;
    violations.extend(find_forbidden(&sources));
    Ok(violations)
}
