//! Shared comparison and recording path for PNG screenshot goldens.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::image::{Image, difference};
use super::plausible::Plausible;

const RECORD_ENV: &str = "GOLDEN_RECORD";

/// Error returned when a screenshot cannot be decoded, judged, compared, or recorded.
#[derive(Debug, Error)]
pub enum GoldenError {
    /// A file operation failed.
    #[error("{operation} {path}: {source}")]
    Io {
        /// The operation being attempted.
        operation: &'static str,
        /// The file involved.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A PNG could not be decoded.
    #[error("could not decode PNG {path}: {message}")]
    Decode {
        /// The PNG file involved.
        path: PathBuf,
        /// The decoder error.
        message: String,
    },
    /// The actual and reference images have different dimensions.
    #[error("golden {path}: dimensions differ: actual {actual:?}, reference {reference:?}")]
    Dimensions {
        /// The reference file.
        path: PathBuf,
        /// Actual image dimensions.
        actual: (u32, u32),
        /// Reference image dimensions.
        reference: (u32, u32),
    },
    /// The image failed its plausibility policy.
    #[error("golden {path}: {message}")]
    Implausible {
        /// The image file involved.
        path: PathBuf,
        /// The measured plausibility failure.
        message: String,
    },
    /// Pixel differences exceed the configured tolerance.
    #[error(
        "golden {path}: mean RGB error {mean_error:.5} (limit {mean_limit:.5}), maximum RGB channel error {max_error} (limit {max_limit})"
    )]
    Difference {
        /// The reference file.
        path: PathBuf,
        /// Measured mean per-channel RGB difference.
        mean_error: f64,
        /// Allowed mean per-channel RGB difference.
        mean_limit: f64,
        /// Measured maximum RGB channel difference.
        max_error: u8,
        /// Allowed maximum RGB channel difference.
        max_limit: u8,
    },
}

/// Mean and maximum RGB channel error accepted by a screenshot comparison.
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    /// Maximum mean absolute RGB channel error in byte values.
    pub mean_rgb_error: f64,
    /// Maximum absolute RGB channel error in byte values.
    pub max_rgb_channel_error: u8,
}

/// Shared screenshot golden policy and comparison path.
#[derive(Clone, Debug)]
pub struct Golden {
    name: String,
    plausible: Plausible,
}

impl Golden {
    /// Creates a named screenshot golden with at least one required content expectation.
    #[must_use]
    pub fn new(name: impl Into<String>, plausible: Plausible) -> Self {
        Self {
            name: name.into(),
            plausible,
        }
    }

    /// Validates one rendered PNG without comparing it to a reference.
    pub fn check(&self, path: &Path) -> Result<(), GoldenError> {
        let bytes = read(path)?;
        let image = Image::decode(path, &bytes)?;
        self.plausible.check(&image, path)
    }

    /// Validates, records when selected by GOLDEN_RECORD, otherwise compares to a reference.
    pub fn compare(
        &self,
        actual_path: &Path,
        reference_path: &Path,
        tolerance: Tolerance,
    ) -> Result<(), GoldenError> {
        self.compare_or_record(
            actual_path,
            reference_path,
            tolerance,
            record_matches(&self.name),
        )
    }

    pub(super) fn compare_or_record(
        &self,
        actual_path: &Path,
        reference_path: &Path,
        tolerance: Tolerance,
        record: bool,
    ) -> Result<(), GoldenError> {
        let actual_bytes = read(actual_path)?;
        let actual = Image::decode(actual_path, &actual_bytes)?;
        if let Err(error) = self.plausible.check(&actual, actual_path) {
            save_actual_beside_reference(reference_path, &actual_bytes)?;
            return Err(error);
        }

        if record {
            if let Some(parent) = reference_path.parent() {
                fs::create_dir_all(parent).map_err(|source| GoldenError::Io {
                    operation: "create directory for golden",
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::write(reference_path, &actual_bytes).map_err(|source| GoldenError::Io {
                operation: "record golden",
                path: reference_path.to_path_buf(),
                source,
            })?;
            return remove_actual(actual_path);
        }

        let reference_bytes = read(reference_path)?;
        let reference = Image::decode(reference_path, &reference_bytes)?;
        if let Err(error) = self.plausible.check(&reference, reference_path) {
            save_actual_beside_reference(reference_path, &actual_bytes)?;
            return Err(error);
        }
        if actual.dimensions != reference.dimensions {
            save_actual_beside_reference(reference_path, &actual_bytes)?;
            return Err(GoldenError::Dimensions {
                path: reference_path.to_path_buf(),
                actual: actual.dimensions,
                reference: reference.dimensions,
            });
        }

        let (mean_error, max_error) = difference(&actual.pixels, &reference.pixels);
        if mean_error > tolerance.mean_rgb_error || max_error > tolerance.max_rgb_channel_error {
            save_actual_beside_reference(reference_path, &actual_bytes)?;
            return Err(GoldenError::Difference {
                path: reference_path.to_path_buf(),
                mean_error,
                mean_limit: tolerance.mean_rgb_error,
                max_error,
                max_limit: tolerance.max_rgb_channel_error,
            });
        }
        remove_actual(actual_path)
    }
}

fn record_matches(name: &str) -> bool {
    std::env::var(RECORD_ENV)
        .ok()
        .is_some_and(|pattern| wildcard_matches(&pattern, name))
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == value;
    }
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    if !value.starts_with(first) {
        return false;
    }
    let mut remaining = value.get(first.len()..).unwrap_or_default();
    let parts: Vec<&str> = parts.collect();
    for (index, part) in parts.iter().enumerate() {
        if index + 1 == parts.len() && !pattern.ends_with('*') {
            return remaining.ends_with(part);
        }
        let Some(found) = remaining.find(part) else {
            return false;
        };
        remaining = remaining.get(found + part.len()..).unwrap_or_default();
    }
    pattern.ends_with('*') || remaining.is_empty()
}

pub(super) fn read(path: &Path) -> Result<Vec<u8>, GoldenError> {
    fs::read(path).map_err(|source| GoldenError::Io {
        operation: "read golden image",
        path: path.to_path_buf(),
        source,
    })
}

fn remove_actual(path: &Path) -> Result<(), GoldenError> {
    fs::remove_file(path).map_err(|source| GoldenError::Io {
        operation: "remove rendered image",
        path: path.to_path_buf(),
        source,
    })
}

fn save_actual_beside_reference(reference_path: &Path, bytes: &[u8]) -> Result<(), GoldenError> {
    let stem = reference_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("golden");
    let actual_path = reference_path.with_file_name(format!("{stem}.actual.png"));
    fs::write(&actual_path, bytes).map_err(|source| GoldenError::Io {
        operation: "write actual image",
        path: actual_path,
        source,
    })
}
