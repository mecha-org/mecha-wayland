pub struct SpriteRegion {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct AtlasData {
    pub png_bytes: &'static [u8],
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphInfo {
    pub x:         f32,
    pub y:         f32,
    pub w:         f32,
    pub h:         f32,
    pub bearing_x: f32, // xmin: pen → left edge of bitmap
    pub bearing_y: f32, // ymin: baseline → bottom of bitmap (positive = above baseline)
    pub advance:   f32,
}

#[derive(Debug)]
pub struct BakedFont {
    pub size:        f32,
    pub line_height: f32,
    pub glyphs:      [GlyphInfo; 95], // index = char as u8 - 32, covers ' '..='~'
}

#[cfg(feature = "builder")]
pub mod builder;
