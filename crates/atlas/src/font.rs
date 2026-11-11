//! Fonts and glyph records: fontdue behind a per-font, per-size index
//! vector, so a glyph is rasterized once per logical pixel size and found
//! again with no hashing.

use std::borrow::Cow;

use geometry::Size;

use crate::{Atlas, AtlasId, AtlasTile, Bitmap, Class, Error, FontId, Format, PAGE, Page};

/// A glyph's tile and what places it. Placement is logical pixels at the
/// size it was asked for; the tile is device pixels at the atlas's scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    /// Empty bounds for a glyph with no ink, such as a space.
    pub tile: AtlasTile,
    /// Pen to the bitmap's left edge.
    pub left: f32,
    /// Baseline to the bitmap's top edge, positive above.
    pub top: f32,
    /// Pen to the next pen, the same at every scale.
    pub advance: f32,
    /// The ink, logical.
    pub size: Size,
    /// The tile's width in device pixels.
    pub physical_width: u32,
    /// The tile's height in device pixels.
    pub physical_height: u32,
}

/// A glyph record and the key it was made from, to make it again at
/// another scale under the same index.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Record {
    pub(crate) font: FontId,
    pub(crate) id: u16,
    pub(crate) px: u16,
    pub(crate) glyph: Glyph,
}

/// Line metrics at one pixel size, as the font gives them: `descent` is
/// negative.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub ascent: f32,
    pub descent: f32,
    pub gap: f32,
}

impl Line {
    /// From the top of the tallest letter to the bottom of the lowest
    /// descender: the letters' own box, without the gap to the next line.
    pub fn extent(&self) -> f32 {
        self.ascent - self.descent
    }
}

const NONE: u32 = u32::MAX;

/// One loaded font and its glyph index vectors.
pub(crate) struct Font {
    inner: fontdue::Font,
    /// Per pixel size asked for: glyph id to record index, `NONE` if not
    /// yet rasterized. As long as the font's glyph count.
    sizes: Vec<(u16, Vec<u32>)>,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("glyphs", &self.inner.glyph_count())
            .field("sizes", &self.sizes.len())
            .finish()
    }
}

impl Font {
    fn new(bytes: Cow<'static, [u8]>) -> Result<Font, Error> {
        let inner = fontdue::Font::from_bytes(bytes.as_ref(), fontdue::FontSettings::default())
            .map_err(Error::Font)?;
        Ok(Font {
            inner,
            sizes: Vec::new(),
        })
    }

    /// The slot for `glyph` at `px`, opening the size's vector on first
    /// sight.
    fn slot(&mut self, px: u16, glyph: u16) -> &mut u32 {
        let i = match self.sizes.iter().position(|(p, _)| *p == px) {
            Some(i) => i,
            None => {
                self.sizes
                    .push((px, vec![NONE; self.inner.glyph_count() as usize]));
                self.sizes.len() - 1
            }
        };
        &mut self.sizes[i].1[glyph as usize]
    }
}

impl Atlas {
    /// Parse and keep a font. `include_bytes!` and a `Vec<u8>` both work.
    /// `Error::Font` if fontdue rejects it.
    pub fn add_font(&mut self, bytes: impl Into<Cow<'static, [u8]>>) -> Result<FontId, Error> {
        let font = Font::new(bytes.into())?;
        let id = FontId(self.fonts.len() as u16);
        self.fonts.push(font);
        Ok(id)
    }

    /// The first font in `chain` whose cmap has `ch`, with its glyph id.
    /// `None` if none has it.
    pub fn lookup(&self, chain: &[FontId], ch: char) -> Option<(FontId, u16)> {
        chain.iter().copied().find_map(|f| {
            let font = &self.fonts[f.0 as usize].inner;
            font.has_glyph(ch).then(|| (f, font.lookup_glyph_index(ch)))
        })
    }

    /// Pen to the next pen for `glyph` at logical `px`, from the font's
    /// metrics: nothing is rasterized. `glyph(..).advance` is this.
    pub fn advance(&self, font: FontId, glyph: u16, px: u16) -> f32 {
        self.fonts[font.0 as usize]
            .inner
            .metrics_indexed(glyph, px as f32)
            .advance_width
    }

    /// The kerning between two glyphs at `px`; zero when the font has no
    /// pair.
    pub fn kern(&self, font: FontId, left: u16, right: u16, px: u16) -> f32 {
        self.fonts[font.0 as usize]
            .inner
            .horizontal_kern_indexed(left, right, px as f32)
            .unwrap_or(0.0)
    }

    /// Line metrics at `px`. A font with no horizontal metrics reports
    /// zeros.
    pub fn line(&self, font: FontId, px: u16) -> Line {
        match self.fonts[font.0 as usize]
            .inner
            .horizontal_line_metrics(px as f32)
        {
            Some(m) => Line {
                ascent: m.ascent,
                descent: m.descent,
                gap: m.line_gap,
            },
            None => Line {
                ascent: 0.0,
                descent: 0.0,
                gap: 0.0,
            },
        }
    }

    /// The record for `glyph` of `font` at logical `px`, rasterized at
    /// `round(px * scale)` and packed into the `Glyph` class on first
    /// sight, returned from the record list ever after. Panics on a font
    /// this atlas did not mint or a glyph id past the font's count; ids
    /// come from `lookup`. Panics on a glyph whose bitmap cannot fit an
    /// empty page.
    pub fn glyph(&mut self, font: FontId, glyph: u16, px: u16) -> Glyph {
        let slot = *self.fonts[font.0 as usize].slot(px, glyph);
        if slot != NONE {
            return self.glyphs[slot as usize].glyph;
        }
        let record = self.raster(font, glyph, px);
        let index = self.glyphs.len() as u32;
        self.glyphs.push(Record {
            font,
            id: glyph,
            px,
            glyph: record,
        });
        *self.fonts[font.0 as usize].slot(px, glyph) = index;
        record
    }

    /// Rasterize and pack `glyph` of `font` at logical `px` at the current
    /// scale: the body of a miss, and of a rebuild.
    pub(crate) fn raster(&mut self, font: FontId, glyph: u16, px: u16) -> Glyph {
        let scale = self.scale;
        let (m, pixels) = self.fonts[font.0 as usize]
            .inner
            .rasterize_indexed(glyph, (px as f32 * scale).round());
        let tile = if m.width == 0 || m.height == 0 {
            AtlasTile {
                atlas: AtlasId::default(),
                bounds: geometry::Rect::ZERO,
            }
        } else {
            let bitmap = Bitmap {
                width: m.width as u32,
                height: m.height as u32,
                format: Format::R8,
                pixels,
            };
            if !Page::fits(Class::Glyph, bitmap.width, bitmap.height) {
                panic!(
                    "atlas: glyph {glyph} of {font:?} at {px} px, scale {scale}, is {}x{}, \
                     larger than a {PAGE} page",
                    bitmap.width, bitmap.height
                );
            }
            self.pack(Class::Glyph, &bitmap)
                .expect("a glyph bitmap is R8 and fits a page")
        };
        Glyph {
            tile,
            left: m.xmin as f32 / scale,
            top: (m.ymin + m.height as i32) as f32 / scale,
            advance: self.advance(font, glyph, px),
            size: Size::new(m.width as f32 / scale, m.height as f32 / scale),
            physical_width: m.width as u32,
            physical_height: m.height as u32,
        }
    }

    /// `glyph` for every character of `chars` that `font` has, at `px`.
    /// The startup path: warm what the first frame will show.
    pub fn warm(&mut self, font: FontId, px: u16, chars: impl IntoIterator<Item = char>) {
        for ch in chars {
            if let Some((f, id)) = self.lookup(&[font], ch) {
                self.glyph(f, id, px);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Atlas, Error};

    const INTER: &[u8] = include_bytes!("../tests/fixtures/Inter-Regular.ttf");

    fn atlas_with_inter() -> (Atlas, FontId) {
        let mut atlas = Atlas::new();
        let f = atlas.add_font(INTER).unwrap();
        (atlas, f)
    }

    #[test]
    fn garbage_is_not_a_font() {
        let mut atlas = Atlas::new();
        assert!(matches!(
            atlas.add_font(&b"not a font"[..]),
            Err(Error::Font(_))
        ));
        let owned: Vec<u8> = INTER.to_vec();
        assert_eq!(
            atlas.add_font(owned).unwrap(),
            FontId(0),
            "a Vec is taken as is"
        );
    }

    #[test]
    fn lookup_walks_the_chain() {
        let (mut atlas, inter) = atlas_with_inter();
        let again = atlas.add_font(INTER).unwrap();
        assert_eq!(again, FontId(1));
        let (f, id) = atlas.lookup(&[inter, again], 'a').unwrap();
        assert_eq!(f, inter, "the first font that has it");
        assert_ne!(id, 0);
        assert_eq!(
            atlas.lookup(&[inter], '\u{1F600}'),
            None,
            "Inter has no emoji"
        );
        assert_eq!(atlas.lookup(&[], 'a'), None);
    }

    #[test]
    fn a_miss_rasterizes_once_and_a_hit_returns_the_same_record() {
        let (mut atlas, inter) = atlas_with_inter();
        let (_, a) = atlas.lookup(&[inter], 'a').unwrap();
        let g1 = atlas.glyph(inter, a, 14);
        assert!(!g1.tile.bounds.is_empty());
        assert!(g1.advance > 0.0);
        assert_eq!(atlas.pages().count(), 1);
        let pages_before = atlas.pages().count();
        let g2 = atlas.glyph(inter, a, 14);
        assert_eq!(g1, g2);
        assert_eq!(atlas.pages().count(), pages_before);
        let g3 = atlas.glyph(inter, a, 28);
        assert_ne!(
            g3.tile.bounds, g1.tile.bounds,
            "another size is another tile"
        );
        assert!(g3.tile.bounds.width() > g1.tile.bounds.width());
        assert_eq!(atlas.class(g1.tile.atlas), Class::Glyph);
    }

    #[test]
    fn a_space_has_no_ink_but_an_advance() {
        let (mut atlas, inter) = atlas_with_inter();
        let (_, sp) = atlas.lookup(&[inter], ' ').unwrap();
        let g = atlas.glyph(inter, sp, 14);
        assert!(g.tile.bounds.is_empty());
        assert!(g.advance > 0.0);
    }

    #[test]
    fn metrics_match_fontdue() {
        let (mut atlas, inter) = atlas_with_inter();
        let font = fontdue::Font::from_bytes(INTER, fontdue::FontSettings::default()).unwrap();
        let idx = font.lookup_glyph_index('g');
        let (m, _) = font.rasterize_indexed(idx, 20.0);
        let g = atlas.glyph(inter, idx, 20);
        assert_eq!(g.left, m.xmin as f32);
        assert_eq!(g.top, (m.ymin + m.height as i32) as f32);
        assert_eq!(g.advance, m.advance_width);
        assert_eq!(g.tile.bounds.width(), m.width as f32);
        assert_eq!(g.tile.bounds.height(), m.height as f32);
        assert!(
            g.top > 0.0 && m.ymin < 0,
            "a g has a descender and ink above the baseline"
        );
    }

    #[test]
    fn kern_and_line_are_lookups() {
        let (atlas, inter) = atlas_with_inter();
        let (_, a) = atlas.lookup(&[inter], 'a').unwrap();
        let (_, b) = atlas.lookup(&[inter], 'b').unwrap();
        let _ = atlas.kern(inter, a, b, 14); // any value; the pair may or may not exist
        assert_eq!(
            atlas.kern(inter, 0, 0, 14),
            0.0,
            "the missing-glyph pair has no entry"
        );
        let l = atlas.line(inter, 14);
        assert!(l.ascent > 0.0);
        assert!(l.descent < 0.0);
        assert!(l.gap >= 0.0);
        assert_eq!(atlas.pages().count(), 0, "neither packs anything");
    }

    #[test]
    fn warm_leaves_every_following_glyph_a_hit() {
        let (mut atlas, inter) = atlas_with_inter();
        atlas.warm(inter, 14, ' '..='~');
        let records = atlas.glyph_count();
        for ch in ' '..='~' {
            let (_, id) = atlas.lookup(&[inter], ch).unwrap();
            atlas.glyph(inter, id, 14);
        }
        assert_eq!(atlas.glyph_count(), records, "nothing new was made");
        assert_eq!(atlas.pages().count(), 1, "ASCII at 14 fits one page");
    }
}
