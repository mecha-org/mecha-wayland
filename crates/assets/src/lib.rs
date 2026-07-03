#[derive(Clone, Copy, Debug)]
pub struct SpriteRegion {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AtlasId(pub u32);

pub struct AtlasData {
    pub id: AtlasId,
    pub png_bytes: &'static [u8],
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphInfo {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub bearing_x: f32, // xmin: pen → left edge of bitmap
    pub bearing_y: f32, // ymin: baseline → bottom of bitmap (positive = above baseline)
    pub advance: f32,
}

#[derive(Debug)]
pub struct BakedFont {
    pub atlas_id: AtlasId,
    pub size: f32,
    pub line_height: f32,
    /// Distance from the top of the text bounding box to the baseline, in pixels.
    pub ascent: f32,
    pub glyphs: [GlyphInfo; 95], // index = char as u8 - 32, covers ' '..='~'
}

impl BakedFont {
    /// Measure the horizontal layout width of a string of text.
    pub fn measure_width(&self, text: &str) -> f32 {
        let mut width = 0.0;
        for ch in text.chars() {
            let byte = ch as u32;
            if byte < 32 || byte > 126 {
                continue;
            }
            let glyph = &self.glyphs[(byte - 32) as usize];
            width += glyph.advance;
        }
        width
    }

    /// Calculate the baseline offset (from the top of a bounding box of height `box_h`)
    /// needed to vertically center the typical capital letters and digits of this font.
    pub fn get_baseline_offset(&self, box_h: f32) -> f32 {
        // We use the character '0' (ASCII 48) as the visual height reference for vertical centering.
        let glyph = &self.glyphs[(48 - 32) as usize];
        box_h / 2.0 + glyph.bearing_y + glyph.h / 2.0
    }
}

#[cfg(feature = "builder")]
pub mod builder;
