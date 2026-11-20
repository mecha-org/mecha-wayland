//! `Icon`: one tinted sprite the caller already inserted into the atlas.

use app::{Build, Context, Handle, Spawner, Widget};
use atlas::{Atlas, AtlasTile, Rescaled, SpriteId};
use geometry::{Color, Point, Size};
use layout::{LayoutStyle, Measure};
use paint::{MonochromeSprite, Paint, PaintContext};

pub struct Icon {
    sprite: SpriteId,
    tile: AtlasTile,
    size: Size,
    color: Color,
    opaque: bool,
}

/// The icon's one sprite.
fn sprite_paint(tile: AtlasTile, size: Size, color: Color, opaque: bool) -> Paint {
    Paint::Monochrome(vec![
        MonochromeSprite::new(tile, Point::ZERO, size, color).opaque(opaque),
    ])
}

impl Icon {
    pub fn color(&self) -> Color {
        self.color
    }
    pub fn size(&self) -> Size {
        self.size
    }
}

pub fn icon(sprite: SpriteId) -> IconBuilder {
    IconBuilder {
        sprite,
        style: LayoutStyle::default(),
        color: Color::WHITE,
        size: None,
        opaque: true,
    }
}

pub struct IconBuilder {
    sprite: SpriteId,
    style: LayoutStyle,
    color: Color,
    size: Option<Size>,
    opaque: bool,
}

impl IconBuilder {
    pub fn style(mut self, style: LayoutStyle) -> Self {
        self.style = style;
        self
    }
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
    /// Displayed size; defaults to the sprite's natural size.
    pub fn size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }
    pub fn opaque(mut self, opaque: bool) -> Self {
        self.opaque = opaque;
        self
    }
}

impl Build for IconBuilder {
    type Widget = Icon;
}

impl Widget for Icon {
    type Builder = IconBuilder;
    fn build(b: IconBuilder, me: Handle<Icon>, s: &mut Spawner<'_, Icon>) -> Icon {
        *s.component_mut::<LayoutStyle>(me).unwrap() = b.style;
        // Atlas::sprite panics on an id this atlas did not mint: a
        // caller bug, the same class as an unregistered component.
        let sprite = s.resource::<Atlas>().sprite(b.sprite);
        let size = b.size.unwrap_or(sprite.size);
        *s.component_mut::<Paint>(me).unwrap() = sprite_paint(sprite.tile, size, b.color, b.opaque);
        *s.component_mut::<Measure>(me).unwrap() = Measure::fixed(size);
        s.on::<Rescaled>(me, |ctx: &mut Context<'_, Icon>, _| {
            let id = ctx.me().sprite;
            let tile = ctx.resource::<Atlas>().sprite(id).tile;
            let (size, color, opaque) = {
                let w = ctx.me();
                (w.size, w.color, w.opaque)
            };
            ctx.set_paint(sprite_paint(tile, size, color, opaque));
            ctx.me().tile = tile;
        });
        Icon {
            sprite: b.sprite,
            tile: sprite.tile,
            size,
            color: b.color,
            opaque: b.opaque,
        }
    }
}

pub trait IconContext {
    fn set_color(&mut self, color: Color);
    /// Redraws at `size` without touching the atlas: a tile is sampled
    /// at any size up to its logical master, mipmapped down for anything
    /// smaller. Also rewrites `Measure`, since the widget's own choice
    /// of displayed size is now its natural size.
    fn set_size(&mut self, size: Size);
    /// Swaps to a different already-inserted sprite, keeping `color`.
    fn set_sprite(&mut self, sprite: SpriteId);
}

impl IconContext for Context<'_, Icon> {
    fn set_color(&mut self, color: Color) {
        if self.me().color == color {
            return;
        }
        let (tile, size, opaque) = {
            let w = self.me();
            (w.tile, w.size, w.opaque)
        };
        self.set_paint(sprite_paint(tile, size, color, opaque));
        self.me().color = color;
    }

    fn set_size(&mut self, size: Size) {
        if self.me().size == size {
            return;
        }
        let (tile, color, opaque) = {
            let w = self.me();
            (w.tile, w.color, w.opaque)
        };
        self.set_paint(sprite_paint(tile, size, color, opaque));
        *self.component_mut::<Measure>().unwrap() = Measure::fixed(size);
        self.me().size = size;
    }

    fn set_sprite(&mut self, id: SpriteId) {
        let sprite = self.resource::<Atlas>().sprite(id);
        if self.me().tile == sprite.tile && self.me().size == sprite.size {
            return;
        }
        let (color, opaque) = (self.me().color, self.me().opaque);
        self.set_paint(sprite_paint(sprite.tile, sprite.size, color, opaque));
        *self.component_mut::<Measure>().unwrap() = Measure::fixed(sprite.size);
        let w = self.me();
        w.sprite = id;
        w.tile = sprite.tile;
        w.size = sprite.size;
    }
}

#[cfg(test)]
mod tests {
    use geometry::Color;
    use layout::LayoutStyle;

    use super::*;

    #[test]
    fn builder_verbs_set_the_right_fields() {
        let b = icon(SpriteId(0)).color(Color::BLACK);
        assert_eq!(b.sprite, SpriteId(0));
        assert_eq!(b.color, Color::BLACK);
    }

    #[test]
    fn style_verb_replaces_the_whole_style() {
        let style = LayoutStyle::default().column();
        let b = icon(SpriteId(1)).style(style.clone());
        assert_eq!(b.style, style);
    }
}
