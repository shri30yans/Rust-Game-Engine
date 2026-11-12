//! PNG decoding and pixel operations shared by screenshot validation.

use std::path::Path;

use png::{ColorType, Decoder, Transformations};

use super::compare::GoldenError;

/// A decoded RGBA8 screenshot. Every test that reads rendered pixels decodes through this type.
pub struct Image {
    /// Width and height in pixels.
    pub dimensions: (u32, u32),
    /// Pixels in row-major order.
    pub pixels: Vec<[u8; 4]>,
}

impl Image {
    /// Reads and decodes a PNG file.
    pub fn read(path: &Path) -> Result<Self, GoldenError> {
        Self::decode(path, &super::compare::read(path)?)
    }

    pub(super) fn decode(path: &Path, bytes: &[u8]) -> Result<Self, GoldenError> {
        let mut decoder = Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|error| GoldenError::Decode {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        let mut frame = vec![0; reader.output_buffer_size().unwrap_or(0)];
        let info = reader
            .next_frame(&mut frame)
            .map_err(|error| GoldenError::Decode {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
        let channel_count = match info.color_type {
            ColorType::Grayscale => 1,
            ColorType::GrayscaleAlpha => 2,
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
            ColorType::Indexed => {
                return Err(GoldenError::Decode {
                    path: path.to_path_buf(),
                    message: "palette was not expanded".to_owned(),
                });
            }
        };
        let pixel_count = usize::try_from(info.width)
            .ok()
            .and_then(|width| {
                usize::try_from(info.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| GoldenError::Decode {
                path: path.to_path_buf(),
                message: "image dimensions overflow addressable memory".to_owned(),
            })?;
        let frame = frame
            .get(..info.buffer_size())
            .ok_or_else(|| GoldenError::Decode {
                path: path.to_path_buf(),
                message: "decoded frame buffer is shorter than its metadata".to_owned(),
            })?;
        let mut pixels = Vec::with_capacity(pixel_count);
        for channels in frame.chunks_exact(channel_count) {
            pixels.push(to_rgba(channels).ok_or_else(|| GoldenError::Decode {
                path: path.to_path_buf(),
                message: "unexpected PNG channel count".to_owned(),
            })?);
        }
        if pixels.len() != pixel_count {
            return Err(GoldenError::Decode {
                path: path.to_path_buf(),
                message: "decoded pixel count does not match dimensions".to_owned(),
            });
        }
        Ok(Self {
            dimensions: (info.width, info.height),
            pixels,
        })
    }

    pub(super) fn region_mean_luminance(
        &self,
        region: super::plausible::ImageRegion,
    ) -> Option<f64> {
        let (width, height) = self.dimensions;
        if region.left >= region.right
            || region.top >= region.bottom
            || region.right > width
            || region.bottom > height
        {
            return None;
        }
        let mut sum = 0.0;
        let mut count = 0_usize;
        for y in region.top..region.bottom {
            let row_start = usize::try_from(y)
                .ok()?
                .checked_mul(usize::try_from(width).ok()?)?;
            for x in region.left..region.right {
                let index = row_start.checked_add(usize::try_from(x).ok()?)?;
                sum += luminance(self.pixels.get(index)?);
                count += 1;
            }
        }
        (count > 0).then_some(sum / count as f64)
    }

    pub(super) fn region_non_clear_percent(
        &self,
        region: super::plausible::ImageRegion,
        clear: [u8; 3],
        channel_threshold: u8,
    ) -> Option<f64> {
        let (width, height) = self.dimensions;
        if region.left >= region.right
            || region.top >= region.bottom
            || region.right > width
            || region.bottom > height
        {
            return None;
        }
        let mut count = 0_usize;
        let mut different = 0_usize;
        for y in region.top..region.bottom {
            let row_start = usize::try_from(y)
                .ok()?
                .checked_mul(usize::try_from(width).ok()?)?;
            for x in region.left..region.right {
                let index = row_start.checked_add(usize::try_from(x).ok()?)?;
                let [red, green, blue, _alpha] = self.pixels.get(index)?;
                count += 1;
                if red.abs_diff(clear[0]) > channel_threshold
                    || green.abs_diff(clear[1]) > channel_threshold
                    || blue.abs_diff(clear[2]) > channel_threshold
                {
                    different += 1;
                }
            }
        }
        (count > 0).then_some(different as f64 * 100.0 / count as f64)
    }
}

pub(super) fn difference(actual: &[[u8; 4]], reference: &[[u8; 4]]) -> (f64, u8) {
    let mut sum = 0_u64;
    let mut channels = 0_u64;
    let mut maximum = 0_u8;
    for (actual_pixel, reference_pixel) in actual.iter().zip(reference) {
        for (actual, reference) in actual_pixel
            .iter()
            .take(3)
            .zip(reference_pixel.iter().take(3))
        {
            let delta = actual.abs_diff(*reference);
            sum += u64::from(delta);
            channels += 1;
            maximum = maximum.max(delta);
        }
    }
    (sum as f64 / channels as f64, maximum)
}

pub(super) fn luminance(pixel: &[u8; 4]) -> f64 {
    let [red, green, blue, _alpha] = pixel;
    (0.2126 * f64::from(*red) + 0.7152 * f64::from(*green) + 0.0722 * f64::from(*blue)) / 255.0
}

fn to_rgba(channels: &[u8]) -> Option<[u8; 4]> {
    match channels {
        [gray] => Some([*gray, *gray, *gray, 255]),
        [gray, alpha] => Some([*gray, *gray, *gray, *alpha]),
        [red, green, blue] => Some([*red, *green, *blue, 255]),
        [red, green, blue, alpha] => Some([*red, *green, *blue, *alpha]),
        _ => None,
    }
}
