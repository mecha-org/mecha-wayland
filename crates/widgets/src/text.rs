//! `Text`: one line of glyphs, shaped once at build and re-shaped by its
//! `Context` setters. Single-line only.

use app::{Build, Context, Handle, Spawner, Widget};
use atlas::{Atlas, FontId, Rescaled};
use geometry::{Color, Point, Size};
use layout::{LayoutStyle, Measure};
use paint::{MonochromeSprite, Paint, PaintContext};

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalTrim {
    #[default]
    Normal,
    CapHeight,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
    Visible,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDecoration {
    #[default]
    None,
    Underline,
    Strikethrough,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextWrap {
    #[default]
    NoWrap,
    Word,
    BreakAll, // or Char
}

/// The configurable properties of a native text widget.
#[derive(Debug, Clone, PartialEq)]
pub struct TextProps {
    pub font: FontId,
    pub px: u16,
    pub color: Color,
    pub string: String,
    pub line_height: Option<f32>,
    pub letter_spacing: Option<f32>,
    pub vertical_trim: VerticalTrim,
    pub align: TextAlign,
    pub overflow: TextOverflow,
    pub decoration: TextDecoration,
    pub wrap: TextWrap,
    pub max_lines: Option<usize>,
}

impl TextProps {
    pub fn new(font: FontId, s: impl Into<String>) -> Self {
        Self {
            font,
            string: s.into(),
            px: 16,
            color: Color::WHITE,
            line_height: None,
            letter_spacing: None,
            vertical_trim: VerticalTrim::Normal,
            align: TextAlign::Left,
            overflow: TextOverflow::Clip,
            decoration: TextDecoration::None,
            wrap: TextWrap::NoWrap,
            max_lines: None,
        }
    }
}

pub struct Text {
    pub props: TextProps,
}

impl std::ops::Deref for Text {
    type Target = TextProps;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.props
    }
}

pub fn text(font: FontId, s: impl Into<String>) -> TextBuilder {
    TextBuilder {
        props: TextProps::new(font, s),
        style: LayoutStyle::default(),
    }
}

pub struct TextBuilder {
    pub props: TextProps,
    pub style: LayoutStyle,
}

impl std::ops::Deref for TextBuilder {
    type Target = TextProps;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.props
    }
}

impl From<TextProps> for TextBuilder {
    fn from(props: TextProps) -> Self {
        Self {
            props,
            style: LayoutStyle::default(),
        }
    }
}

impl TextBuilder {
    pub fn font(mut self, font: FontId) -> Self {
        self.props.font = font;
        self
    }

    pub fn style(mut self, style: LayoutStyle) -> Self {
        self.style = style;
        self
    }

    pub fn size(mut self, px: u16) -> Self {
        self.props.px = px;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.props.color = color;
        self
    }

    pub fn line_height(mut self, height: f32) -> Self {
        self.props.line_height = Some(height);
        self
    }

    pub fn letter_spacing(mut self, spacing: f32) -> Self {
        self.props.letter_spacing = Some(spacing);
        self
    }

    pub fn vertical_trim(mut self, trim: VerticalTrim) -> Self {
        self.props.vertical_trim = trim;
        self
    }

    pub fn align(mut self, align: TextAlign) -> Self {
        self.props.align = align;
        self
    }

    pub fn overflow(mut self, overflow: TextOverflow) -> Self {
        self.props.overflow = overflow;
        self
    }

    pub fn decoration(mut self, decoration: TextDecoration) -> Self {
        self.props.decoration = decoration;
        self
    }

    pub fn wrap(mut self, wrap: TextWrap) -> Self {
        self.props.wrap = wrap;
        self
    }

    pub fn max_lines(mut self, max_lines: usize) -> Self {
        self.props.max_lines = Some(max_lines);
        self
    }
}

impl Build for TextBuilder {
    type Widget = Text;
}

impl Widget for Text {
    type Builder = TextBuilder;
    fn build(b: TextBuilder, me: Handle<Text>, s: &mut Spawner<'_, Text>) -> Text {
        *s.component_mut::<LayoutStyle>(me).unwrap() = b.style;
        let (sprites, size) = {
            let mut atlas = s.resource_mut::<Atlas>();
            shape(
                &mut atlas,
                b.props.font,
                b.props.px,
                &b.props.string,
                b.props.color,
            )
        };
        *s.component_mut::<Paint>(me).unwrap() = Paint::Monochrome(sprites);
        *s.component_mut::<Measure>(me).unwrap() = Measure::fixed(size);

        s.on::<Rescaled>(me, |ctx: &mut Context<'_, Text>, _| reshape_text(ctx));

        Text { props: b.props }
    }
}

/// One glyph of a laid-out line: the byte offset of its char, the font
/// and glyph it resolved to, and its pen origin.
struct Placed {
    byte: usize,
    font: FontId,
    glyph: u16,
    x: f32,
}

/// The single pen loop behind `measure`, `hit_position` and `shape`: a
/// `Placed` per character found in `font`, and the final pen as the
/// width. Kerns against the previous placed glyph when it is in the same
/// font. A character missing from the font's cmap is skipped entirely —
/// no width, no entry, as if it were never in the string. One present but
/// with no ink (such as a space) still advances the pen. Reads advances
/// and kerning without rasterizing.
fn layout(atlas: &Atlas, font: FontId, px: u16, text: &str) -> (Vec<Placed>, f32) {
    let mut placed: Vec<Placed> = Vec::new();
    let mut pen = 0.0f32;
    for (byte, ch) in text.char_indices() {
        let Some((f, id)) = atlas.lookup(&[font], ch) else {
            continue;
        };
        if let Some(prev) = placed.last().filter(|p| p.font == f) {
            pen += atlas.kern(f, prev.glyph, id, px);
        }
        placed.push(Placed {
            byte,
            font: f,
            glyph: id,
            x: pen,
        });
        pen += atlas.advance(f, id, px);
    }
    (placed, pen)
}

/// The byte offset of the glyph boundary closest to `x` pixels into
/// `text`: where a click at `x` lands the cursor. `0` for an empty
/// string or `x` at or before the first glyph's midpoint; `text.len()`
/// for `x` past the end. Boundaries come from `layout`, kerning
/// included, so they line up with a drawn glyph's edge.
pub fn hit_position(atlas: &Atlas, font: FontId, px: u16, text: &str, x: f32) -> usize {
    let (placed, _) = layout(atlas, font, px, text);
    let mut best = 0usize;
    let mut best_d = x.abs(); // distance from the boundary at pen = 0
    for p in &placed {
        let end = p.byte + text[p.byte..].chars().next().map_or(0, char::len_utf8);
        let d = (p.x + atlas.advance(p.font, p.glyph, px) - x).abs();
        if d < best_d {
            best_d = d;
            best = end;
        }
    }
    best
}

/// The pen advance of `text` in `font` at `px`: how far the pen moves
/// left to right, which is the width a `Text` of it reports. The width
/// of `layout`, so it agrees with `shape` and `hit_position`.
pub fn measure(atlas: &Atlas, font: FontId, px: u16, text: &str) -> f32 {
    layout(atlas, font, px, text).1
}

/// One line of `text` in `font` at `px`, tinted `color`: a sprite per
/// glyph with ink, placed by `layout`, baselined by the font's ascent,
/// and the line's natural size (`pen` wide, `ascent - descent + gap`
/// tall). Sprites are logical size; their x is snapped to the device
/// grid so a glyph rasterized at its device size lands on whole pixels.
fn shape(
    atlas: &mut Atlas,
    font: FontId,
    px: u16,
    text: &str,
    color: Color,
) -> (Vec<MonochromeSprite>, Size) {
    let line = atlas.line(font, px);
    let baseline = line.ascent;
    let s = atlas.scale();
    let snap = |v: f32| (v * s).round() / s;
    let (placed, width) = layout(atlas, font, px, text);
    let mut sprites = Vec::new();
    for p in &placed {
        let g = atlas.glyph(p.font, p.glyph, px);
        if g.tile.bounds.width() > 0.0 {
            sprites.push(MonochromeSprite::new(
                g.tile,
                Point::new(snap(p.x + g.left), baseline - g.top),
                g.size,
                color,
            ));
        }
    }
    let height = line.extent() + line.gap;
    (sprites, Size::new(width, height))
}

pub trait TextContext {
    fn set_text(&mut self, text: impl Into<String>);
    fn set_size(&mut self, px: u16);
    fn set_color(&mut self, color: Color);
    fn set_font(&mut self, font: FontId);
    fn set_line_height(&mut self, height: f32);
    fn set_letter_spacing(&mut self, spacing: f32);
    fn set_vertical_trim(&mut self, trim: VerticalTrim);
    fn set_align(&mut self, align: TextAlign);
    fn set_overflow(&mut self, overflow: TextOverflow);
    fn set_decoration(&mut self, decoration: TextDecoration);
    fn set_wrap(&mut self, wrap: TextWrap);
    fn set_max_lines(&mut self, max_lines: Option<usize>);
}

impl TextContext for Context<'_, Text> {
    fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        if self.me().string == text {
            return;
        }
        self.me().props.string = text;
        reshape_text(self);
    }

    fn set_size(&mut self, px: u16) {
        if self.me().px == px {
            return;
        }
        self.me().props.px = px;
        reshape_text(self);
    }

    fn set_color(&mut self, color: Color) {
        if self.me().color == color {
            return;
        }
        self.me().props.color = color;
        let mut paint = self.paint().clone();
        if let Paint::Monochrome(sprites) = &mut paint {
            for sprite in sprites.iter_mut() {
                sprite.color = color;
            }
        }
        self.set_paint(paint);
    }

    // Placeholder
    fn set_font(&mut self, font: FontId) {
        if self.me().font == font {
            return;
        }
        self.me().props.font = font;
    }

    // Placeholder
    fn set_line_height(&mut self, height: f32) {
        if self.me().line_height == Some(height) {
            return;
        }
        self.me().props.line_height = Some(height);
    }

    // Placeholder
    fn set_letter_spacing(&mut self, spacing: f32) {
        if self.me().letter_spacing == Some(spacing) {
            return;
        }
        self.me().props.letter_spacing = Some(spacing);
    }

    // Placeholder
    fn set_vertical_trim(&mut self, trim: VerticalTrim) {
        if self.me().vertical_trim == trim {
            return;
        }
        self.me().props.vertical_trim = trim;
    }

    // Placeholder
    fn set_align(&mut self, align: TextAlign) {
        if self.me().align == align {
            return;
        }
        self.me().props.align = align;
    }

    // Placeholder
    fn set_overflow(&mut self, overflow: TextOverflow) {
        if self.me().overflow == overflow {
            return;
        }
        self.me().props.overflow = overflow;
    }

    // Placeholder
    fn set_decoration(&mut self, decoration: TextDecoration) {
        if self.me().decoration == decoration {
            return;
        }
        self.me().props.decoration = decoration;
    }

    // Placeholder
    fn set_wrap(&mut self, wrap: TextWrap) {
        if self.me().wrap == wrap {
            return;
        }
        self.me().props.wrap = wrap;
    }

    // Placeholder
    fn set_max_lines(&mut self, max_lines: Option<usize>) {
        if self.me().max_lines == max_lines {
            return;
        }
        self.me().props.max_lines = max_lines;
    }
}

fn reshape_text(ctx: &mut Context<'_, Text>) {
    let (font, px, color, string) = {
        let w = ctx.me();
        (w.font, w.px, w.color, w.string.clone())
    };
    let (sprites, size) = {
        let mut atlas = ctx.resource_mut::<Atlas>();
        shape(&mut atlas, font, px, &string, color)
    };
    ctx.set_paint(Paint::Monochrome(sprites));
    *ctx.component_mut::<Measure>().unwrap() = Measure::fixed(size);
}

#[cfg(test)]
mod tests {
    use atlas::Atlas;
    use geometry::Color;

    use super::*;

    const INTER: &[u8] = include_bytes!("../../atlas/tests/fixtures/Inter-Regular.ttf");

    fn inter() -> (Atlas, FontId) {
        let mut atlas = Atlas::new();
        let font = atlas.add_font(INTER).unwrap();
        (atlas, font)
    }

    #[test]
    fn shape_places_one_sprite_per_glyph_with_ink() {
        let (mut atlas, font) = inter();
        let (sprites, size) = shape(&mut atlas, font, 14, "ab", Color::WHITE);
        assert_eq!(sprites.len(), 2);
        assert!(size.width > 0.0);
        assert!(size.height > 0.0);
        assert_eq!(sprites[0].color, Color::WHITE);
    }

    #[test]
    fn shape_skips_a_space_but_still_advances_the_pen() {
        let (mut atlas, font) = inter();
        let (with_space, wide) = shape(&mut atlas, font, 14, "a b", Color::WHITE);
        let (without_space, narrow) = shape(&mut atlas, font, 14, "ab", Color::WHITE);
        assert_eq!(with_space.len(), 2, "the space has no ink");
        assert_eq!(without_space.len(), 2);
        assert!(
            wide.width > narrow.width,
            "the space's advance still counts"
        );
    }

    #[test]
    fn shape_of_empty_text_is_a_zero_width_run_at_the_line_height() {
        let (mut atlas, font) = inter();
        let (sprites, size) = shape(&mut atlas, font, 14, "", Color::WHITE);
        assert!(sprites.is_empty());
        assert_eq!(size.width, 0.0);
        assert!(
            size.height > 0.0,
            "the line height does not depend on content"
        );
    }

    #[test]
    fn shape_skips_a_character_missing_from_the_cmap_and_contributes_no_width() {
        let (mut atlas, font) = inter();
        // U+4E2D ('中') is not in Inter-Regular's cmap.
        assert!(
            atlas.lookup(&[font], '\u{4e2d}').is_none(),
            "test assumes Inter-Regular has no glyph for U+4E2D"
        );
        let (with_missing, wider) = shape(&mut atlas, font, 14, "a\u{4e2d}b", Color::WHITE);
        let (_, narrower) = shape(&mut atlas, font, 14, "ab", Color::WHITE);
        assert_eq!(
            with_missing.len(),
            2,
            "the missing character paints no sprite"
        );
        assert_eq!(
            wider.width, narrower.width,
            "a character missing from the cmap contributes zero width, unlike a space"
        );
    }

    #[test]
    fn builder_verbs_set_the_right_fields() {
        let (_, font) = inter();
        let b = text(font, "hi")
            .size(24)
            .color(Color::BLACK)
            .line_height(32.0)
            .letter_spacing(1.5)
            .vertical_trim(VerticalTrim::CapHeight)
            .align(TextAlign::Center)
            .overflow(TextOverflow::Ellipsis)
            .decoration(TextDecoration::Underline)
            .wrap(TextWrap::Word)
            .max_lines(3);
        assert_eq!(b.string, "hi");
        assert_eq!(b.px, 24);
        assert_eq!(b.color, Color::BLACK);
        assert_eq!(b.font, font);
        assert_eq!(b.line_height, Some(32.0));
        assert_eq!(b.letter_spacing, Some(1.5));
        assert_eq!(b.vertical_trim, VerticalTrim::CapHeight);
        assert_eq!(b.align, TextAlign::Center);
        assert_eq!(b.overflow, TextOverflow::Ellipsis);
        assert_eq!(b.decoration, TextDecoration::Underline);
        assert_eq!(b.wrap, TextWrap::Word);
        assert_eq!(b.max_lines, Some(3));
    }

    #[test]
    fn measure_equals_the_shaped_width() {
        let (mut atlas, font) = inter();
        let text = "AV To Yay 0123";
        let m = measure(&atlas, font, 14, text);
        let (_, size) = shape(&mut atlas, font, 14, text, Color::WHITE);
        assert_eq!(m, size.width);
    }

    #[test]
    fn hit_boundaries_match_drawn_edges() {
        let (mut atlas, font) = inter();
        let text = "Hello";
        let (sprites, _) = shape(&mut atlas, font, 14, text, Color::WHITE);
        let (placed, _) = layout(&atlas, font, 14, text);
        assert_eq!(sprites.len(), placed.len());
        for (i, ch) in text.char_indices() {
            let end = i + ch.len_utf8();
            let x = measure(&atlas, font, 14, &text[..end]);
            assert_eq!(hit_position(&atlas, font, 14, text, x), end);
            if let Some(next) = placed.iter().position(|p| p.byte == end) {
                let g = atlas.glyph(placed[next].font, placed[next].glyph, 14);
                let origin = sprites[next].offset.x - g.left;
                assert!((origin - x).abs() < 0.5, "{origin} vs {x} at {end}");
            }
        }
    }

    #[test]
    fn missing_glyphs_do_not_shift_boundaries() {
        let (atlas, font) = inter();
        let text = "a\u{1F600}b";
        assert!(atlas.lookup(&[font], '\u{1F600}').is_none());
        assert_eq!(
            measure(&atlas, font, 14, text),
            measure(&atlas, font, 14, "ab")
        );
        assert_eq!(hit_position(&atlas, font, 14, text, 1000.0), text.len());
    }

    #[test]
    fn glyph_sprites_are_logical_size_at_scale_two() {
        let (mut atlas, font) = inter();
        atlas.set_scale(2.0);
        let (sprites, _) = shape(&mut atlas, font, 14, "H", Color::WHITE);
        let (f, id) = atlas.lookup(&[font], 'H').unwrap();
        let g = atlas.glyph(f, id, 14);
        assert_eq!(sprites[0].size, g.size);
        assert!((sprites[0].tile.bounds.width() - 2.0 * g.size.width).abs() <= 1.0);
    }
}
