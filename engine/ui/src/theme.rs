//! Design tokens loaded from a game-supplied theme file (RON).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::color::Srgb8;
use crate::error::UiError;

/// Serial passed by the game when loading; stamped on every token id from that load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThemeSerial(pub u32);

macro_rules! token_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name {
            index: u16,
            serial: ThemeSerial,
        }

        impl $name {
            pub(crate) fn new(index: u16, serial: ThemeSerial) -> Self {
                Self { index, serial }
            }

            pub(crate) fn index(self) -> u16 {
                self.index
            }

            pub(crate) fn serial(self) -> ThemeSerial {
                self.serial
            }
        }
    };
}

token_id!(
    #[doc = "Resolved colour token."]
    ColorId
);
token_id!(
    #[doc = "Resolved typographic style token."]
    TypeId
);
token_id!(
    #[doc = "Spacing token."]
    SpaceId
);
token_id!(
    #[doc = "Corner radius token."]
    RadiusId
);
token_id!(
    #[doc = "Named layout length."]
    SizeId
);
token_id!(
    #[doc = "Named glyph string token."]
    GlyphId
);
token_id!(
    #[doc = "Motion timing token."]
    MotionId
);

/// Raw theme file shape (serde mirror). Unknown sections are rejected.
#[derive(Debug, Deserialize)]
#[serde(rename = "Theme")]
#[serde(deny_unknown_fields)]
pub(crate) struct ThemeFile {
    pub name: String,
    pub colors: BTreeMap<String, String>,
    pub contrast_rules: Vec<ContrastRule>,
    pub scrim: ScrimFile,
    pub fonts: BTreeMap<String, FontFile>,
    pub type_scale: BTreeMap<String, TypeScaleEntry>,
    pub space: BTreeMap<String, f32>,
    pub radius: BTreeMap<String, f32>,
    pub sizes: BTreeMap<String, f32>,
    pub glyphs: BTreeMap<String, String>,
    pub motion: BTreeMap<String, MotionEntry>,
    #[serde(default)]
    pub touch: Option<TouchFile>,
}

/// One contrast rule: foreground name, background name, minimum ratio.
#[derive(Debug, Clone, Deserialize)]
pub struct ContrastRule(pub String, pub String, pub f32);

/// Scrim overlay settings from the theme file.
#[derive(Debug, Clone, Deserialize)]
pub struct ScrimFile {
    /// Colour token name for the scrim fill.
    pub color: String,
    /// Alpha when compositing in sRGB space.
    pub alpha: f32,
    /// Alpha when a linear colour pipeline is active.
    pub alpha_if_linear_blend: f32,
}

/// Font family entry from the theme file.
#[derive(Debug, Clone, Deserialize)]
pub struct FontFile {
    /// Human-readable family name.
    pub family: String,
    /// Weights shipped for this family.
    pub weights: Vec<u32>,
    /// Glob or path under the content root.
    pub file: String,
    /// SPDX or licence id.
    pub license: String,
}

/// One step on the typographic scale.
#[derive(Debug, Clone, Deserialize)]
pub struct TypeScaleEntry {
    /// Font token name.
    pub font: String,
    /// OpenType weight.
    pub weight: u32,
    /// Font size in logical px.
    pub size: f32,
    /// Line height in logical px.
    pub line: f32,
    /// Use tabular figures.
    #[serde(default)]
    pub tabular: bool,
    /// Uppercase transformation.
    #[serde(default)]
    pub caps: bool,
    /// Letter-spacing in em.
    #[serde(default)]
    pub tracking: f32,
}

/// Motion curve and duration.
#[derive(Debug, Clone, Deserialize)]
pub struct MotionEntry {
    /// Duration in milliseconds.
    pub ms: u32,
    /// Cubic-bezier control points.
    pub curve: (f32, f32, f32, f32),
}

/// Optional touch-target guidelines.
#[derive(Debug, Clone, Deserialize)]
pub struct TouchFile {
    /// Minimum control size in millimetres.
    pub min_target_mm: f32,
    /// Combat controls minimum in millimetres.
    pub combat_target_mm: f32,
    /// Minimum gap between targets in logical px.
    pub min_gap_px: f32,
}

/// Validated design tokens ready for runtime lookup.
#[derive(Debug, Clone)]
pub struct Theme {
    serial: ThemeSerial,
    name: String,
    colors: Vec<Srgb8>,
    color_names: BTreeMap<String, ColorId>,
    scrim: ScrimFile,
    fonts: BTreeMap<String, FontFile>,
    type_styles: Vec<TypeScaleEntry>,
    type_names: BTreeMap<String, TypeId>,
    spaces: Vec<f32>,
    space_names: BTreeMap<String, SpaceId>,
    radii: Vec<f32>,
    radius_names: BTreeMap<String, RadiusId>,
    sizes: Vec<f32>,
    size_names: BTreeMap<String, SizeId>,
    glyphs: Vec<String>,
    glyph_names: BTreeMap<String, GlyphId>,
    motions: Vec<MotionEntry>,
    motion_names: BTreeMap<String, MotionId>,
    touch: Option<TouchFile>,
}

impl Theme {
    /// Parses and validates a theme from RON text. Token ids carry `serial`.
    pub fn from_ron(text: &str, serial: ThemeSerial) -> Result<Self, UiError> {
        let file: ThemeFile = ron::from_str(text).map_err(|err| UiError::Parse(err.to_string()))?;
        Self::from_file(file, serial)
    }

    fn from_file(file: ThemeFile, serial: ThemeSerial) -> Result<Self, UiError> {
        let mut parsed_colors = BTreeMap::new();
        for (name, hex) in file.colors {
            let colour = Srgb8::from_hex(&hex)?;
            parsed_colors.insert(name, colour);
        }
        let (colors, color_names) = index_tokens(parsed_colors, serial, ColorId::new)?;
        for rule in &file.contrast_rules {
            let fg_id = color_names
                .get(&rule.0)
                .copied()
                .ok_or_else(|| UiError::UnknownToken {
                    group: "colors",
                    name: rule.0.clone(),
                })?;
            let bg_id = color_names
                .get(&rule.1)
                .copied()
                .ok_or_else(|| UiError::UnknownToken {
                    group: "colors",
                    name: rule.1.clone(),
                })?;
            let fg = colors
                .get(usize::from(fg_id.index()))
                .copied()
                .ok_or(UiError::TokenOutOfRange)?;
            let bg = colors
                .get(usize::from(bg_id.index()))
                .copied()
                .ok_or(UiError::TokenOutOfRange)?;
            let ratio = Srgb8::contrast_ratio(fg, bg);
            if ratio + f32::EPSILON < rule.2 {
                return Err(UiError::ContrastViolation {
                    fg: rule.0.clone(),
                    bg: rule.1.clone(),
                    ratio,
                    min: rule.2,
                });
            }
        }
        let (type_styles, type_names) = index_tokens(file.type_scale, serial, TypeId::new)?;
        let (spaces, space_names) = index_tokens(file.space, serial, SpaceId::new)?;
        let (radii, radius_names) = index_tokens(file.radius, serial, RadiusId::new)?;
        let (sizes, size_names) = index_tokens(file.sizes, serial, SizeId::new)?;
        let (glyphs, glyph_names) = index_tokens(file.glyphs, serial, GlyphId::new)?;
        let (motions, motion_names) = index_tokens(file.motion, serial, MotionId::new)?;
        Ok(Self {
            serial,
            name: file.name,
            colors,
            color_names,
            scrim: file.scrim,
            fonts: file.fonts,
            type_styles,
            type_names,
            spaces,
            space_names,
            radii,
            radius_names,
            sizes,
            size_names,
            glyphs,
            glyph_names,
            motions,
            motion_names,
            touch: file.touch,
        })
    }

    /// Display name from the theme file.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Resolves a colour name to an id.
    pub fn color_id(&self, name: &str) -> Result<ColorId, UiError> {
        lookup(&self.color_names, "colors", name)
    }

    /// Colour value for an id created by this theme.
    pub fn color(&self, id: ColorId) -> Result<Srgb8, UiError> {
        self.check_serial(id.serial())?;
        self.colors
            .get(usize::from(id.index()))
            .copied()
            .ok_or(UiError::TokenOutOfRange)
    }

    /// Resolves a type-scale name.
    pub fn type_id(&self, name: &str) -> Result<TypeId, UiError> {
        lookup(&self.type_names, "type_scale", name)
    }

    /// Type-scale entry for an id from this theme.
    pub fn type_style(&self, id: TypeId) -> Result<&TypeScaleEntry, UiError> {
        self.check_serial(id.serial())?;
        self.type_styles
            .get(usize::from(id.index()))
            .ok_or(UiError::TokenOutOfRange)
    }

    /// Resolves a spacing token name.
    pub fn space_id(&self, name: &str) -> Result<SpaceId, UiError> {
        lookup(&self.space_names, "space", name)
    }

    /// Spacing length in logical pixels.
    pub fn space(&self, id: SpaceId) -> Result<f32, UiError> {
        self.check_serial(id.serial())?;
        self.spaces
            .get(usize::from(id.index()))
            .copied()
            .ok_or(UiError::TokenOutOfRange)
    }

    /// Resolves a radius token name.
    pub fn radius_id(&self, name: &str) -> Result<RadiusId, UiError> {
        lookup(&self.radius_names, "radius", name)
    }

    /// Radius in logical pixels.
    pub fn radius(&self, id: RadiusId) -> Result<f32, UiError> {
        self.check_serial(id.serial())?;
        self.radii
            .get(usize::from(id.index()))
            .copied()
            .ok_or(UiError::TokenOutOfRange)
    }

    /// Resolves a named size token.
    pub fn size_id(&self, name: &str) -> Result<SizeId, UiError> {
        lookup(&self.size_names, "sizes", name)
    }

    /// Size length in logical pixels.
    pub fn size(&self, id: SizeId) -> Result<f32, UiError> {
        self.check_serial(id.serial())?;
        self.sizes
            .get(usize::from(id.index()))
            .copied()
            .ok_or(UiError::TokenOutOfRange)
    }

    /// Resolves a glyph token name.
    pub fn glyph_id(&self, name: &str) -> Result<GlyphId, UiError> {
        lookup(&self.glyph_names, "glyphs", name)
    }

    /// Glyph string for an id from this theme.
    pub fn glyph(&self, id: GlyphId) -> Result<&str, UiError> {
        self.check_serial(id.serial())?;
        self.glyphs
            .get(usize::from(id.index()))
            .map(String::as_str)
            .ok_or(UiError::TokenOutOfRange)
    }

    /// Resolves a motion token name.
    pub fn motion_id(&self, name: &str) -> Result<MotionId, UiError> {
        lookup(&self.motion_names, "motion", name)
    }

    /// Motion entry for an id from this theme.
    pub fn motion(&self, id: MotionId) -> Result<&MotionEntry, UiError> {
        self.check_serial(id.serial())?;
        self.motions
            .get(usize::from(id.index()))
            .ok_or(UiError::TokenOutOfRange)
    }

    /// Touch-target guidelines when present in the theme file.
    #[must_use]
    pub fn touch(&self) -> Option<&TouchFile> {
        self.touch.as_ref()
    }

    /// Scrim overlay settings from the theme file.
    #[must_use]
    pub fn scrim(&self) -> &ScrimFile {
        &self.scrim
    }

    /// Font table entry by token name.
    pub fn font(&self, name: &str) -> Result<&FontFile, UiError> {
        self.fonts.get(name).ok_or_else(|| UiError::UnknownToken {
            group: "fonts",
            name: name.to_owned(),
        })
    }

    fn check_serial(&self, serial: ThemeSerial) -> Result<(), UiError> {
        if serial == self.serial {
            Ok(())
        } else {
            Err(UiError::ForeignId)
        }
    }
}

fn lookup<I: Copy>(
    map: &BTreeMap<String, I>,
    group: &'static str,
    name: &str,
) -> Result<I, UiError> {
    map.get(name).copied().ok_or_else(|| UiError::UnknownToken {
        group,
        name: name.to_owned(),
    })
}

fn index_tokens<K, V, I>(
    source: BTreeMap<K, V>,
    serial: ThemeSerial,
    make_id: fn(u16, ThemeSerial) -> I,
) -> Result<(Vec<V>, BTreeMap<K, I>), UiError>
where
    K: Ord,
{
    let len = source.len();
    if len > usize::from(u16::MAX) {
        return Err(UiError::Parse("too many tokens in one group".to_owned()));
    }
    let mut values = Vec::with_capacity(len);
    let mut names = BTreeMap::new();
    for (idx, (key, value)) in source.into_iter().enumerate() {
        let index =
            u16::try_from(idx).map_err(|_| UiError::Parse("token index overflow".to_owned()))?;
        values.push(value);
        names.insert(key, make_id(index, serial));
    }
    Ok((values, names))
}
