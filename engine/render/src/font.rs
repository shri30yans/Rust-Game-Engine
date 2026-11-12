//! TTF text through fontdue: glyphs are rasterised on first use at the exact pixel
//! size into one R8 atlas (shelf packed) and cached. Only dirty rows are uploaded.
use crate::error::RenderError;
use crate::to_u32;
use std::collections::HashMap;

/// Side length of the square R8 glyph atlas, in texels.
pub const ATLAS: u32 = 2048;

/// Handle to a font registered with [`FontAtlas::add_font`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FontId(pub u8);

/// A rasterised glyph: its atlas rectangle and layout metrics, in pixels.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    /// Atlas rectangle as normalised `[u0, v0, u1, v1]`.
    pub uv: [f32; 4],
    /// Offset of the bitmap's top-left from the pen position on the baseline (y down).
    pub off: [f32; 2],
    /// Bitmap size in pixels.
    pub size: [f32; 2],
    /// Horizontal pen advance in pixels.
    pub advance: f32,
}

/// Glyph cache and shelf packer for every registered font.
pub struct FontAtlas {
    fonts: Vec<fontdue::Font>,
    /// Atlas texels, `ATLAS * ATLAS` bytes, row-major.
    pub pixels: Vec<u8>,
    cache: HashMap<(u8, char, u16), Glyph>,
    x: u32,
    y: u32,
    row_h: u32,
    /// Dirty row range [lo, hi) waiting for upload.
    pub dirty: Option<(u32, u32)>,
    /// Bumped when the atlas is wiped, so callers holding glyphs know to re-query.
    pub generation: u32,
}

/// Pixel size used as the cache key and raster size: rounded, then clamped to [4, 400].
#[allow(clippy::cast_possible_truncation)] // The value is clamped to [4, 400] first, so it always fits in u16.
fn glyph_px(px: f32) -> u16 {
    px.round().clamp(4.0, 400.0) as u16
}

impl FontAtlas {
    /// Glyph returned for an unknown font: zero-sized, so text drawing skips it.
    const EMPTY: Glyph = Glyph {
        uv: [0.0; 4],
        off: [0.0; 2],
        size: [0.0; 2],
        advance: 0.0,
    };

    /// Empty atlas with no fonts.
    pub fn new() -> Self {
        // A 2x2 opaque block at (0,0) used for solid shapes is not needed: shapes use
        // SDF modes. Start packing at x = 1 to keep a clear border.
        FontAtlas {
            fonts: Vec::new(),
            pixels: vec![0; (ATLAS * ATLAS) as usize],
            cache: HashMap::new(),
            x: 1,
            y: 1,
            row_h: 0,
            dirty: None,
            generation: 0,
        }
    }

    /// Parses a TTF/OTF file and returns its id. Fails if the data does not parse or
    /// the atlas already holds 256 fonts.
    pub fn add_font(&mut self, ttf: &[u8]) -> Result<FontId, RenderError> {
        let f = fontdue::Font::from_bytes(
            ttf,
            fontdue::FontSettings {
                scale: 64.0,
                ..Default::default()
            },
        )
        .map_err(RenderError::BadFont)?;
        let id = u8::try_from(self.fonts.len()).map_err(|_| RenderError::TooManyFonts)?;
        self.fonts.push(f);
        Ok(FontId(id))
    }

    /// True once at least one font is registered.
    pub fn has_fonts(&self) -> bool {
        !self.fonts.is_empty()
    }

    /// Ascent in pixels at `px`. Falls back to `0.8 * px` when the font has no line
    /// metrics or `f` is unknown.
    pub fn ascent(&self, f: FontId, px: f32) -> f32 {
        self.fonts
            .get(usize::from(f.0))
            .and_then(|font| font.horizontal_line_metrics(px))
            .map(|m| m.ascent)
            .unwrap_or(px * 0.8)
    }

    /// Kerning between `a` and `b` at `px`; 0 when the pair or the font is unknown.
    pub fn kern(&self, f: FontId, a: char, b: char, px: f32) -> f32 {
        self.fonts
            .get(usize::from(f.0))
            .and_then(|font| font.horizontal_kern(a, b, px))
            .unwrap_or(0.0)
    }

    /// Cached glyph for `c` at `px`, rasterised and packed on first use. An unknown font
    /// yields a zero-sized glyph.
    pub fn glyph(&mut self, f: FontId, c: char, px: f32) -> Glyph {
        let size = glyph_px(px);
        if let Some(g) = self.cache.get(&(f.0, c, size)) {
            return *g;
        }
        let Some(font) = self.fonts.get(usize::from(f.0)) else {
            return Self::EMPTY;
        };
        let (m, bmp) = font.rasterize(c, f32::from(size));
        let (w, h) = (to_u32(m.width), to_u32(m.height));
        if self.x + w + 2 > ATLAS {
            self.x = 1;
            self.y += self.row_h + 2;
            self.row_h = 0;
        }
        if self.y + h + 2 > ATLAS {
            // Full: wipe and start again. Rare (mostly when many sizes are used).
            self.cache.clear();
            self.pixels.iter_mut().for_each(|p| *p = 0);
            self.x = 1;
            self.y = 1;
            self.row_h = 0;
            self.generation += 1;
            self.dirty = Some((0, ATLAS));
        }
        let wu = w as usize;
        for row in 0..h {
            let d = ((self.y + row) * ATLAS + self.x) as usize;
            let s = (row * w) as usize;
            if let (Some(dst), Some(src)) = (self.pixels.get_mut(d..d + wu), bmp.get(s..s + wu)) {
                dst.copy_from_slice(src);
            }
        }
        let a = ATLAS as f32;
        let g = Glyph {
            uv: [
                self.x as f32 / a,
                self.y as f32 / a,
                (self.x + w) as f32 / a,
                (self.y + h) as f32 / a,
            ],
            off: [m.xmin as f32, -(m.ymin as f32 + h as f32)],
            size: [w as f32, h as f32],
            advance: m.advance_width,
        };
        let (lo, hi) = self.dirty.unwrap_or((self.y, self.y + h));
        self.dirty = Some((lo.min(self.y), hi.max(self.y + h)));
        self.x += w + 2;
        self.row_h = self.row_h.max(h);
        self.cache.insert((f.0, c, size), g);
        g
    }

    /// Width of a string in pixels (advance + kerning), before any squash.
    pub fn measure(&mut self, f: FontId, px: f32, s: &str) -> f32 {
        let mut w = 0.0;
        let mut prev: Option<char> = None;
        for c in s.chars() {
            if let Some(p) = prev {
                w += self.kern(f, p, c, px);
            }
            w += self.glyph(f, c, px).advance;
            prev = Some(c);
        }
        w
    }
}

impl Default for FontAtlas {
    fn default() -> Self {
        Self::new()
    }
}
