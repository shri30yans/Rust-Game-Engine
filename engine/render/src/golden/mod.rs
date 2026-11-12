//! Test-only PNG reference comparison with shared plausibility checks.
//!
//! The module validates rendered pixels before either comparing them or recording them, so a
//! broken blank frame cannot become a passing reference image.

mod compare;
mod image;
mod plausible;

pub use compare::{Golden, GoldenError, Tolerance};
pub use image::Image;
pub use plausible::{ImageRegion, Plausible};

#[cfg(test)]
mod tests;
