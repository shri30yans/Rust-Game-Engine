//! sRGB 8-bit colours and WCAG 2.x contrast helpers.

use crate::error::UiError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A colour with alpha, written `"#rrggbb"` or `"#rrggbbaa"`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    /// The sRGB colour.
    pub rgb: Srgb8,
    /// Opacity, 0 (clear) to 255 (solid).
    pub a: u8,
}

impl Color {
    /// Parses `#rrggbb` or `#rrggbbaa`.
    ///
    /// # Errors
    ///
    /// Returns [`UiError::InvalidColor`] for any other text.
    pub fn parse(text: &str) -> Result<Self, UiError> {
        if text.len() == 9 && text.is_ascii() {
            let (rgb, alpha) = text.split_at(7);
            let a = u8::from_str_radix(alpha, 16)
                .map_err(|_| UiError::InvalidColor(text.to_owned()))?;
            return Ok(Self {
                rgb: Srgb8::from_hex(rgb)?,
                a,
            });
        }
        Ok(Self {
            rgb: Srgb8::from_hex(text)?,
            a: 255,
        })
    }

    /// Opacity in `[0, 1]`.
    #[must_use]
    pub fn alpha(self) -> f32 {
        f32::from(self.a) / 255.0
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            self.rgb.r, self.rgb.g, self.rgb.b, self.a
        ))
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Opaque sRGB colour in 8 bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Srgb8 {
    /// Red channel, 0–255.
    pub r: u8,
    /// Green channel, 0–255.
    pub g: u8,
    /// Blue channel, 0–255.
    pub b: u8,
}

impl Srgb8 {
    /// Parses `#RRGGBB` (ASCII hex digits, case-insensitive). The `#` prefix is required.
    pub fn from_hex(hex: &str) -> Result<Self, UiError> {
        let Some(body) = hex.strip_prefix('#') else {
            return Err(UiError::InvalidColor(hex.to_owned()));
        };
        let bytes = body.as_bytes();
        if bytes.len() != 6 {
            return Err(UiError::InvalidColor(hex.to_owned()));
        }
        let pairs = bytes.as_chunks::<2>();
        if !pairs.1.is_empty() {
            return Err(UiError::InvalidColor(hex.to_owned()));
        }
        let mut channels = [0u8; 3];
        for (channel, pair) in channels.iter_mut().zip(pairs.0) {
            let [high, low] = *pair;
            *channel =
                decode_hex_pair(high, low).ok_or_else(|| UiError::InvalidColor(hex.to_owned()))?;
        }
        Ok(Self {
            r: channels[0],
            g: channels[1],
            b: channels[2],
        })
    }

    /// WCAG relative luminance for this sRGB colour.
    #[must_use]
    pub fn relative_luminance(self) -> f32 {
        fn channel(c: u8) -> f32 {
            let s = f32::from(c) / 255.0;
            if s <= 0.039_28 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// WCAG contrast ratio between two sRGB colours (always ≥ 1).
    #[must_use]
    pub fn contrast_ratio(a: Self, b: Self) -> f32 {
        let l1 = a.relative_luminance();
        let l2 = b.relative_luminance();
        let (light, dark) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        (light + 0.05) / (dark + 0.05)
    }
}

fn decode_hex_pair(high: u8, low: u8) -> Option<u8> {
    let h = decode_hex_digit(high)?;
    let l = decode_hex_digit(low)?;
    Some(h * 16 + l)
}

fn decode_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl<'de> Deserialize<'de> for Srgb8 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::Srgb8;
    use crate::error::UiError;
    use proptest::prelude::*;

    #[test]
    fn white_on_black_contrast() {
        let white = Srgb8::from_hex("#FFFFFF").unwrap();
        let black = Srgb8::from_hex("#000000").unwrap();
        let ratio = Srgb8::contrast_ratio(white, black);
        assert!((ratio - 21.0).abs() < 0.1);
    }

    #[test]
    fn requires_hash_prefix() {
        assert!(matches!(
            Srgb8::from_hex("FFFFFF"),
            Err(UiError::InvalidColor(_))
        ));
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(matches!(
            Srgb8::from_hex("#FFF"),
            Err(UiError::InvalidColor(_))
        ));
        assert!(matches!(
            Srgb8::from_hex("#FFFFFFF"),
            Err(UiError::InvalidColor(_))
        ));
    }

    #[test]
    fn rejects_non_ascii_hex() {
        assert!(matches!(
            Srgb8::from_hex("#GGGGGG"),
            Err(UiError::InvalidColor(_))
        ));
        assert!(matches!(
            Srgb8::from_hex("#ffffffé"),
            Err(UiError::InvalidColor(_))
        ));
    }

    #[test]
    fn accepts_upper_and_lower_hex() {
        let upper = Srgb8::from_hex("#AABBCC").unwrap();
        let lower = Srgb8::from_hex("#aabbcc").unwrap();
        assert_eq!(upper, lower);
        assert_eq!(upper.r, 0xAA);
        assert_eq!(upper.g, 0xBB);
        assert_eq!(upper.b, 0xCC);
    }

    proptest! {
        #[test]
        fn arbitrary_strings_parse_safely(s in any::<String>()) {
            match Srgb8::from_hex(&s) {
                Ok(color) => {
                    prop_assert!(s.starts_with('#'));
                    let body = s.strip_prefix('#').expect("prefix checked");
                    prop_assert_eq!(body.len(), 6);
                    prop_assert!(body.as_bytes().iter().all(u8::is_ascii_hexdigit));
                    let formatted = format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b);
                    let again = Srgb8::from_hex(&formatted).expect("canonical hex");
                    prop_assert_eq!(color, again);
                }
                Err(UiError::InvalidColor(_)) => {}
                Err(other) => prop_assert!(false, "unexpected error: {other:?}"),
            }
        }
    }
}
