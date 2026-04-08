use anyhow::{Context, Result};

use crate::atlas::{AtlasKey, GlyphAtlas};
use crate::primitives::{MonoSprite, Rect};
use crate::scene::{PrimitiveId, Scene};

pub struct TextMetrics {
    pub width:   f32,
    pub ascent:  f32, // distance from baseline to top (positive)
    pub descent: f32, // distance from baseline to bottom (negative)
}

impl TextMetrics {
    /// Total height from top of ascenders to bottom of descenders.
    pub fn height(&self) -> f32 { self.ascent - self.descent }
}

pub struct TextSystem {
    fonts:      Vec<fontdue::Font>,
    pub atlas:  GlyphAtlas,
}

impl TextSystem {
    pub fn new(gl: &glow::Context, atlas_size: u32) -> Result<Self> {
        let atlas = GlyphAtlas::new(gl, atlas_size)?;
        Ok(Self { fonts: Vec::new(), atlas })
    }

    /// Load a font from bytes; returns a `font_id` for subsequent calls.
    pub fn load_font(&mut self, bytes: &[u8]) -> Result<u32> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!("font load: {e}"))?;
        let id = self.fonts.len() as u32;
        self.fonts.push(font);
        Ok(id)
    }

    /// Returns width and line metrics for `text` at `px`. Does not touch the atlas.
    pub fn measure_text(&self, text: &str, font_id: u32, px: f32) -> TextMetrics {
        let font  = &self.fonts[font_id as usize];
        let width = text.chars()
            .map(|ch| font.metrics_indexed(font.lookup_glyph_index(ch), px).advance_width)
            .sum();
        let lm = font.horizontal_line_metrics(px).unwrap();
        TextMetrics { width, ascent: lm.ascent, descent: lm.descent }
    }

    /// Lay out `text` and push one [`MonoSprite`] per glyph into `scene`.
    /// `origin` is the baseline-left anchor in physical pixels.
    pub fn draw_text(
        &mut self,
        scene:   &mut Scene,
        gl:      &glow::Context,
        text:    &str,
        font_id: u32,
        px:      f32,
        color:   [f32; 4],
        origin:  [f32; 2],
    ) -> Result<Vec<PrimitiveId>> {
        let atlas_size = (self.atlas.width, self.atlas.height);
        let atlas_tex  = self.atlas.texture;

        let font = self.fonts.get(font_id as usize)
            .context("invalid font_id")?;

        let mut ids    = Vec::new();
        let mut pen_x  = origin[0];
        let font_size_x64 = (px * 64.0) as u32;

        for ch in text.chars() {
            let glyph_id = font.lookup_glyph_index(ch);

            let key = AtlasKey { font_id, glyph_id: glyph_id as u32, font_size_x64 };

            let tile = match self.atlas.get(&key) {
                Some(t) => t,
                None => {
                    // Need to re-borrow font after mutable atlas call
                    let font = self.fonts.get(font_id as usize).unwrap();
                    let (metrics, bitmap) = font.rasterize_indexed(glyph_id, px);
                    self.atlas.insert(
                        gl,
                        key,
                        &bitmap,
                        metrics.width  as u16,
                        metrics.height as u16,
                        metrics.xmin   as i16,
                        metrics.ymin   as i16,
                    )?
                }
            };

            // Advance width from metrics (re-fetch after potential atlas insert).
            let font = self.fonts.get(font_id as usize).unwrap();
            let advance = font.metrics_indexed(glyph_id, px).advance_width;

            if tile.w > 0 && tile.h > 0 {
                let sprite = MonoSprite {
                    bounds: Rect {
                        x: pen_x + tile.bearing_x as f32,
                        y: origin[1] - tile.bearing_y as f32 - tile.h as f32,
                        w: tile.w as f32,
                        h: tile.h as f32,
                    },
                    tile,
                    atlas_size,
                    atlas_tex,
                    color,
                    clip: None,
                };
                use crate::primitives::RenderablePrimitive as _;
                ids.push(sprite.add_to_scene(scene));
            }

            pen_x += advance;
        }

        Ok(ids)
    }
}
