//! Required content expectations and the shared uniform-frame rejection rule.

use std::collections::HashMap;
use std::path::Path;

use super::compare::GoldenError;
use super::image::{Image, luminance};

const MIN_LUMINANCE_STD_DEV: f64 = 0.25 / 255.0;
const MAX_DOMINANT_PIXEL_FRACTION: f64 = 0.999;

/// Pixel rectangle; right and bottom are exclusive bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageRegion {
    /// Left edge in pixels.
    pub left: u32,
    /// Top edge in pixels.
    pub top: u32,
    /// Exclusive right edge in pixels.
    pub right: u32,
    /// Exclusive bottom edge in pixels.
    pub bottom: u32,
}

impl ImageRegion {
    /// Creates a pixel region with exclusive right and bottom bounds.
    #[must_use]
    pub const fn new(left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

#[derive(Clone, Debug)]
enum Expectation {
    LuminanceRange {
        name: String,
        region: ImageRegion,
        minimum: f64,
        maximum: f64,
    },
    NotClearColor {
        name: String,
        region: ImageRegion,
        clear_color: [u8; 3],
        minimum_percent: u8,
        channel_threshold: u8,
    },
}

/// Required content expectations for one PNG screenshot golden.
///
/// Every policy starts with a named expectation. Additional expectations are appended through
/// the builder methods, so a golden cannot be constructed with only the uniform-frame guard.
#[derive(Clone, Debug)]
pub struct Plausible {
    expectations: Vec<Expectation>,
}

impl Plausible {
    /// Starts a policy by requiring the named region's mean luminance in an inclusive range.
    #[must_use]
    pub fn luminance_range(
        name: impl Into<String>,
        region: ImageRegion,
        minimum: f64,
        maximum: f64,
    ) -> Self {
        Self {
            expectations: vec![Expectation::LuminanceRange {
                name: name.into(),
                region,
                minimum,
                maximum,
            }],
        }
    }

    /// Starts a policy by requiring enough pixels in a named region to differ from clear colour.
    #[must_use]
    pub fn not_clear_color(
        name: impl Into<String>,
        region: ImageRegion,
        clear_color: [u8; 3],
        minimum_percent: u8,
        channel_threshold: u8,
    ) -> Self {
        Self {
            expectations: vec![Expectation::NotClearColor {
                name: name.into(),
                region,
                clear_color,
                minimum_percent,
                channel_threshold,
            }],
        }
    }

    /// Adds a named region's inclusive normalized mean-luminance range.
    #[must_use]
    pub fn and_luminance_range(
        mut self,
        name: impl Into<String>,
        region: ImageRegion,
        minimum: f64,
        maximum: f64,
    ) -> Self {
        self.expectations.push(Expectation::LuminanceRange {
            name: name.into(),
            region,
            minimum,
            maximum,
        });
        self
    }

    /// Adds a named region's minimum percentage of pixels that differ from clear colour.
    #[must_use]
    pub fn and_not_clear_color(
        mut self,
        name: impl Into<String>,
        region: ImageRegion,
        clear_color: [u8; 3],
        minimum_percent: u8,
        channel_threshold: u8,
    ) -> Self {
        self.expectations.push(Expectation::NotClearColor {
            name: name.into(),
            region,
            clear_color,
            minimum_percent,
            channel_threshold,
        });
        self
    }

    pub(super) fn check(&self, image: &Image, path: &Path) -> Result<(), GoldenError> {
        let mut failures = Vec::new();
        if image.pixels.is_empty() {
            failures.push("image has no pixels".to_owned());
        } else {
            let mut histogram = HashMap::<[u8; 4], usize>::new();
            let mut sum = 0.0;
            let mut sum_squared = 0.0;
            for pixel in &image.pixels {
                let value = luminance(pixel);
                sum += value;
                sum_squared += value * value;
                *histogram.entry(*pixel).or_default() += 1;
            }
            let pixel_count = image.pixels.len() as f64;
            let mean = sum / pixel_count;
            let standard_deviation = (sum_squared / pixel_count - mean * mean).max(0.0).sqrt();
            let dominant_fraction =
                histogram.values().copied().max().unwrap_or(0) as f64 / pixel_count;
            if standard_deviation < MIN_LUMINANCE_STD_DEV
                || dominant_fraction >= MAX_DOMINANT_PIXEL_FRACTION
            {
                failures.push(format!(
                    "image is uniform: luminance standard deviation {standard_deviation:.5} (minimum {MIN_LUMINANCE_STD_DEV:.5}), dominant pixel fraction {dominant_fraction:.4} (must be below {MAX_DOMINANT_PIXEL_FRACTION:.3})"
                ));
            }
        }

        self.check_expectations(image, &mut failures);
        if failures.is_empty() {
            Ok(())
        } else {
            Err(implausible(path, failures.join("; ")))
        }
    }

    fn check_expectations(&self, image: &Image, failures: &mut Vec<String>) {
        for expectation in &self.expectations {
            match expectation {
                Expectation::LuminanceRange {
                    name,
                    region,
                    minimum,
                    maximum,
                } => {
                    let Some(mean) = image.region_mean_luminance(*region) else {
                        failures.push(format!("region '{name}' is empty or outside the image"));
                        continue;
                    };
                    if !minimum.is_finite()
                        || !maximum.is_finite()
                        || minimum < &0.0
                        || maximum > &1.0
                        || minimum > maximum
                    {
                        failures.push(format!("region '{name}' has an invalid luminance range"));
                    } else if mean < *minimum || mean > *maximum {
                        failures.push(format!(
                            "region '{name}' mean luminance {mean:.4} is outside {minimum:.4}..={maximum:.4}"
                        ));
                    }
                }
                Expectation::NotClearColor {
                    name,
                    region,
                    clear_color,
                    minimum_percent,
                    channel_threshold,
                } => {
                    if *minimum_percent > 100 {
                        failures.push(format!(
                            "region '{name}' has invalid minimum non-clear percentage {minimum_percent}"
                        ));
                        continue;
                    }
                    let Some(percent) =
                        image.region_non_clear_percent(*region, *clear_color, *channel_threshold)
                    else {
                        failures.push(format!("region '{name}' is empty or outside the image"));
                        continue;
                    };
                    if percent < f64::from(*minimum_percent) {
                        failures.push(format!(
                            "region '{name}' has {percent:.2}% pixels differing from clear colour by more than {channel_threshold} per channel (minimum {minimum_percent}%)"
                        ));
                    }
                }
            }
        }
    }
}

fn implausible(path: &Path, message: String) -> GoldenError {
    GoldenError::Implausible {
        path: path.to_path_buf(),
        message,
    }
}
