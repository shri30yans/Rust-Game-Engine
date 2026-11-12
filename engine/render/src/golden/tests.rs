//! Unit tests for the screenshot golden invariants.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use png::{BitDepth, ColorType, Encoder};

use super::{Golden, GoldenError, ImageRegion, Plausible, Tolerance};

const TOLERANCE: Tolerance = Tolerance {
    mean_rgb_error: 0.05,
    max_rgb_channel_error: 4,
};
static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct TempImages {
    directory: PathBuf,
    actual: PathBuf,
    reference: PathBuf,
}

impl TempImages {
    fn new() -> Self {
        let number = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "engine-render-golden-{}-{number}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("temporary golden directory");
        Self {
            actual: directory.join("actual.png"),
            reference: directory.join("reference.png"),
            directory,
        }
    }

    fn write(&self, path: &Path, pixels: &[[u8; 4]], width: u32, height: u32) {
        let file = fs::File::create(path).expect("create temporary PNG");
        let mut encoder = Encoder::new(file, width, height);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG header");
        let bytes: Vec<u8> = pixels
            .iter()
            .flat_map(|pixel| pixel.iter().copied())
            .collect();
        writer.write_image_data(&bytes).expect("write PNG pixels");
    }
}

impl Drop for TempImages {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn varied_pixels(width: usize, height: usize) -> Vec<[u8; 4]> {
    (0..width * height)
        .map(|index| {
            let value = if index % 2 == 0 { 32 } else { 220 };
            [value, value, value, 255]
        })
        .collect()
}

#[test]
fn uniform_image_is_rejected_during_comparison_and_recording() {
    let temp = TempImages::new();
    let uniform = vec![[255, 255, 255, 255]; 100];
    temp.write(&temp.actual, &uniform, 10, 10);
    temp.write(&temp.reference, &varied_pixels(10, 10), 10, 10);
    let golden = Golden::new(
        "unit/uniform",
        Plausible::luminance_range("lit region", ImageRegion::new(0, 0, 10, 10), 0.03, 0.95),
    );
    let comparison = golden.compare_or_record(&temp.actual, &temp.reference, TOLERANCE, false);
    assert!(matches!(comparison, Err(GoldenError::Implausible { .. })));

    temp.write(&temp.actual, &uniform, 10, 10);
    let record_path = temp.directory.join("recorded.png");
    let recording = golden.compare_or_record(&temp.actual, &record_path, TOLERANCE, true);
    assert!(matches!(recording, Err(GoldenError::Implausible { .. })));
    assert!(!record_path.exists());
}

#[test]
fn one_pixel_difference_inside_tolerance_passes() {
    let temp = TempImages::new();
    let reference = varied_pixels(10, 10);
    let mut actual = reference.clone();
    if let Some(pixel) = actual.first_mut() {
        pixel[0] = pixel[0].saturating_add(1);
    }
    temp.write(&temp.actual, &actual, 10, 10);
    temp.write(&temp.reference, &reference, 10, 10);
    Golden::new(
        "unit/one-pixel",
        Plausible::luminance_range("scene", ImageRegion::new(0, 0, 10, 10), 0.03, 0.95),
    )
    .compare(&temp.actual, &temp.reference, TOLERANCE)
    .expect("one-channel difference is within tolerance");
}

#[test]
fn difference_error_names_reference_file_and_reports_measurements() {
    let temp = TempImages::new();
    let reference = varied_pixels(10, 10);
    let actual: Vec<[u8; 4]> = reference
        .iter()
        .map(|[red, _green, _blue, _alpha]| {
            if *red < 100 {
                [200, 200, 200, 255]
            } else {
                [40, 40, 40, 255]
            }
        })
        .collect();
    temp.write(&temp.actual, &actual, 10, 10);
    temp.write(&temp.reference, &reference, 10, 10);
    let error = Golden::new(
        "unit/difference",
        Plausible::luminance_range("scene", ImageRegion::new(0, 0, 10, 10), 0.03, 0.95),
    )
    .compare(&temp.actual, &temp.reference, TOLERANCE)
    .expect_err("large image difference must fail");
    let message = error.to_string();
    assert!(message.contains("reference.png"));
    assert!(message.contains("mean RGB error"));
    assert!(temp.directory.join("reference.actual.png").exists());
}

#[test]
fn white_frame_fails_the_named_luminance_expectation() {
    let temp = TempImages::new();
    let white = vec![[255, 255, 255, 255]; 100];
    temp.write(&temp.actual, &white, 10, 10);
    let golden = Golden::new(
        "unit/white-floor",
        Plausible::luminance_range(
            "lit arena floor",
            ImageRegion::new(0, 0, 10, 10),
            0.03,
            0.95,
        ),
    );
    let error = golden
        .check(&temp.actual)
        .expect_err("white frame cannot satisfy a lit floor expectation");
    assert!(error.to_string().contains("lit arena floor"));
    assert!(error.to_string().contains("mean luminance 1.0000"));
}

#[test]
fn clear_colour_in_the_required_region_fails_its_expectation() {
    let temp = TempImages::new();
    let mut pixels = varied_pixels(10, 10);
    for index in [0, 1, 10, 11] {
        if let Some(pixel) = pixels.get_mut(index) {
            *pixel = [0, 0, 0, 255];
        }
    }
    temp.write(&temp.actual, &pixels, 10, 10);
    let golden = Golden::new(
        "unit/missing-center-content",
        Plausible::not_clear_color(
            "center scene",
            ImageRegion::new(0, 0, 2, 2),
            [0, 0, 0],
            10,
            8,
        ),
    );
    let error = golden
        .check(&temp.actual)
        .expect_err("clear region cannot satisfy the content expectation");
    assert!(error.to_string().contains("center scene"));
    assert!(error.to_string().contains("0.00%"));
}
