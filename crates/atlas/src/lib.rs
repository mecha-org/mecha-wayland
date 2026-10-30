#![forbid(unsafe_code)]
//! The atlas: every pixel a sprite can name, and the ids that name them.
//!
//! # Model
//!
//! - One [`Atlas`] resource, no module, no system, no signal of its own.
//!   A caller asks it for a glyph, an icon or an image and gets an
//!   [`AtlasTile`] back in the same call. A backend listens for the
//!   core's `OnChanged<Atlas>`, sent at the `PostTick` of a tick that took
//!   `resource_mut::<Atlas>()`, and drains the cells that changed since
//!   it last asked through `resource::<Atlas>()`: every method it calls
//!   takes `&self`, so the drain is not a write.
//! - Four [`Class`]es of texture over one page implementation. `Glyph`
//!   and `Icon` pages are `R8` coverage, `Image` pages `Rgba8` straight
//!   alpha; `External` names a texture the atlas does not hold, a dmabuf
//!   a producer describes with an [`External`]. Pages are 1024 square.
//! - The atlas has a scale, 1 until [`Atlas::set_scale`] says otherwise.
//!   Pages hold device pixels; everything a caller lays out with is
//!   logical: [`Glyph`] placement and advance, [`Line`], kerning,
//!   [`Sprite::size`]. A tile's bounds are device pixels of its page.
//! - A glyph is rasterized once per font, glyph id and whole logical
//!   pixel size, at `round(px * scale)` device pixels, with fontdue, and
//!   found again through index vectors, no hashing. An icon or image is
//!   inserted once at its master size and drawn at that size or smaller:
//!   its page holds four mip levels the CPU generates.
//! - The atlas keeps the source of everything it packed: each glyph's
//!   key, each SVG's bytes and logical size, each raw bitmap. A scale
//!   change scraps every page and packs it all again at the new scale:
//!   SVGs and glyphs are rasterized anew, bitmaps re-packed as they are.
//!   [`FontId`] and [`SpriteId`] name what and survive it; tiles name
//!   where and do not. [`Atlas::generation`] moves on each rebuild, and a
//!   caller that kept a tile asks again when it does.
//! - Nothing is removed. A [`SpriteId`] and a [`FontId`] are dense
//!   indices a caller holds for the app's life. An [`AtlasId`] of an
//!   external lives as long; a page's id is freed by a rebuild and reused
//!   by the next page opened.
//! - Dirty tracking is a 16 by 16 grid of [`Cell`]s per page that serves
//!   every mip level; a backend uploads each dirty cell once per level and,
//!   between rebuilds, a tile is never sent twice.
//!
//! # Quick start
//!
//! ```
//! use atlas::prelude::*;
//!
//! let mut atlas = Atlas::new();
//! let inter = atlas.add_font(include_bytes!("../tests/fixtures/Inter-Regular.ttf")).unwrap();
//! atlas.warm(inter, 14, ' '..='~');
//!
//! // A 2 by 2 opaque grey image, as a caller who decoded a PNG would hand in.
//! let art = atlas
//!     .insert(Class::Image, &Bitmap { width: 2, height: 2, format: Format::Rgba8, pixels: vec![128; 16] })
//!     .unwrap();
//! assert_eq!(atlas.class(atlas.sprite(art).tile.atlas), Class::Image);
//!
//! // A text widget resolving 'a' at 14 px: the glyph is warm, so this packs nothing.
//! let (font, id) = atlas.lookup(&[inter], 'a').unwrap();
//! let g = atlas.glyph(font, id, 14);
//! assert!(g.advance > 0.0);
//!
//! // A backend's drain: two new pages, every cell of each.
//! let mut cells = 0;
//! atlas.drain_dirty(|_, _| cells += 1);
//! assert_eq!(cells, 512);
//! atlas.drain_dirty(|_, _| cells += 1);
//! assert_eq!(cells, 512, "a second drain has nothing");
//! ```

use std::borrow::Cow;

use app::{Event, Resource};
use geometry::{Rect, Size};

pub mod prelude {
    pub use crate::{
        Atlas, AtlasId, AtlasTile, Bitmap, Cell, Class, Error, External, FontId, Format, Glyph,
        Line, Page, Plane, Rescaled, Sprite, SpriteId,
    };
}

mod bitmap;
mod external;
mod font;
mod mip;
mod page;

pub use external::{External, Plane};
pub use font::{Glyph, Line};
pub use page::{CELL, CELLS, Cell, PAGE, Page};

/// Which texture a tile is in: a dense index the atlas mints across every
/// page of every class and every external, so a backend keys its textures
/// by it with a `Vec`. A new page takes the lowest id a rebuild freed,
/// else the next. `default()` is id zero, the first page ever made.
/// `repr(C)` because a render command embeds it and is uploaded as is.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct AtlasId(pub u32);

/// One rectangle of a texture: `bounds` in whole pixels of that texture,
/// padding excluded, held as a `Rect` because that is the shared type. A
/// tile holds until the next rebuild, when [`Atlas::generation`] moves.
/// A renderer divides `bounds` by the size of the texture it uploaded.
/// `repr(C)` because a render command embeds it and is uploaded as is.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AtlasTile {
    pub atlas: AtlasId,
    pub bounds: Rect,
}

/// What a page holds and how. `Glyph` and `Icon` are coverage, `Image` is
/// colour, `External` is a texture the atlas names but does not hold.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Glyph = 0,
    Icon = 1,
    Image = 2,
    External = 3,
}

/// The pixel format of a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    R8,
    Rgba8,
}

impl Format {
    /// Bytes per pixel.
    pub const fn bytes(self) -> usize {
        match self {
            Format::R8 => 1,
            Format::Rgba8 => 4,
        }
    }
}

impl Class {
    /// `Glyph` and `Icon` are `R8`; `Image` is `Rgba8`; `External` is
    /// nominally `Rgba8`: a backend reads its descriptor's fourcc.
    pub const fn format(self) -> Format {
        match self {
            Class::Glyph | Class::Icon => Format::R8,
            Class::Image | Class::External => Format::Rgba8,
        }
    }

    /// Mip levels a page of this class holds, the base included: one for
    /// `Glyph` and `External`, five for `Icon` and `Image`.
    pub const fn levels(self) -> u32 {
        match self {
            Class::Icon | Class::Image => 5,
            Class::Glyph | Class::External => 1,
        }
    }

    /// The grid a tile's origin and size sit on, which is also the zero
    /// border around it: 16 for the mipmapped classes, 1 otherwise.
    pub const fn align(self) -> u32 {
        match self {
            Class::Icon | Class::Image => 16,
            Class::Glyph | Class::External => 1,
        }
    }
}

/// An icon or an image: an index into the atlas's sprite list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpriteId(pub u32);

/// A font: an index into the atlas's font list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(pub u16);

/// Pixels to insert: `width * height * format.bytes()` bytes, row-major,
/// tightly packed, straight alpha. An `Image` page stores it premultiplied.
#[derive(Debug, Clone, PartialEq)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub pixels: Vec<u8>,
}

/// An icon or an image: its tile, in device pixels at the atlas's scale,
/// and its logical size. It is drawn at that size or smaller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    pub tile: AtlasTile,
    /// Logical, fixed at insert; a rebuild keeps it.
    pub size: Size,
    /// The tile's width in device pixels.
    pub physical_width: u32,
    /// The tile's height in device pixels.
    pub physical_height: u32,
}

/// The window scale the atlas follows changed and it was rebuilt. The
/// atlas never sends it; whoever calls `set_scale` for a window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rescaled;
impl Event for Rescaled {}

/// What an `AtlasId` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    /// A page: its class and its index in that class's list.
    Page(Class, u32),
    /// An external: its index in the external list.
    External(u32),
    /// A page id a rebuild freed, for the next page to take.
    Free,
}

/// What a sprite was made from, to make it again at another scale.
#[derive(Debug)]
enum Source {
    /// Rasterized at `ceil(size * scale)` on its longer side.
    #[cfg(feature = "svg")]
    Svg {
        bytes: Cow<'static, [u8]>,
        size: u32,
    },
    /// Like `Svg`, but kept in colour as a `Class::Image`.
    #[cfg(feature = "svg")]
    SvgImage {
        bytes: Cow<'static, [u8]>,
        size: u32,
    },
    /// Device pixels, re-packed as they are.
    Bitmap { class: Class, bitmap: Bitmap },
}

/// Every pixel a sprite can name, and the ids that name them. A plain
/// resource: no system, no event, no signal of its own. Writers call it
/// directly and get a tile back in the same call; a backend drains it in
/// a system on the core's `OnChanged<Atlas>`, sent at the `PostTick` of
/// any tick that took `resource_mut::<Atlas>()`, reading through
/// `resource::<Atlas>()` so the drain is not itself a write.
#[derive(Debug)]
pub struct Atlas {
    /// Device pixels per logical pixel.
    scale: f32,
    /// Effective `set_scale` calls so far.
    generation: u64,
    /// Indexed by `Class as usize` for the three page classes.
    pages: [Vec<Page>; 3],
    /// Indexed by `AtlasId`.
    owners: Vec<Owner>,
    /// Indexed by `SpriteId`.
    sprites: Vec<Sprite>,
    /// Indexed by `SpriteId`.
    sources: Vec<Source>,
    /// Indexed by `FontId`.
    fonts: Vec<font::Font>,
    /// Indexed by the value a font's per-size index vector holds.
    glyphs: Vec<font::Record>,
    /// Indexed by the index `Owner::External` carries.
    externals: Vec<External>,
}

impl Resource for Atlas {}

impl Default for Atlas {
    fn default() -> Self {
        Self::new()
    }
}

impl Atlas {
    /// Empty: no page, no font, no sprite, scale 1.
    pub fn new() -> Atlas {
        Atlas {
            scale: 1.0,
            generation: 0,
            pages: [Vec::new(), Vec::new(), Vec::new()],
            owners: Vec::new(),
            sprites: Vec::new(),
            sources: Vec::new(),
            fonts: Vec::new(),
            glyphs: Vec::new(),
            externals: Vec::new(),
        }
    }

    /// Device pixels per logical pixel.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Moves by one on every rebuild. A caller that kept a tile asks for
    /// it again when this is not what it saw.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Rasterize at `scale` from now on. `false`, and nothing done, when
    /// it is the current scale. Otherwise every page is dropped and its
    /// id freed, every sprite and glyph record is packed again at the new
    /// scale under the same id, the generation moves, and `true`. Panics
    /// on a scale that is not positive and finite, and on a sprite that no
    /// longer fits an empty page.
    pub fn set_scale(&mut self, scale: f32) -> bool {
        assert!(
            scale.is_finite() && scale > 0.0,
            "atlas: scale {scale} is not positive and finite"
        );
        if scale == self.scale {
            return false;
        }
        self.scale = scale;
        for owner in &mut self.owners {
            if let Owner::Page(..) = owner {
                *owner = Owner::Free;
            }
        }
        for list in &mut self.pages {
            list.clear();
        }
        let sources = std::mem::take(&mut self.sources);
        for (n, source) in sources.iter().enumerate() {
            let (class, bitmap) = match source {
                #[cfg(feature = "svg")]
                Source::Svg { bytes, size } => (
                    Class::Icon,
                    Cow::Owned(
                        Bitmap::from_svg(bytes, self.master(*size)).expect("it parsed at insert"),
                    ),
                ),
                #[cfg(feature = "svg")]
                Source::SvgImage { bytes, size } => (
                    Class::Image,
                    Cow::Owned(
                        Bitmap::from_svg_rgba(bytes, self.master(*size))
                            .expect("it parsed at insert"),
                    ),
                ),
                Source::Bitmap { class, bitmap } => (*class, Cow::Borrowed(bitmap)),
            };
            let size = self.sprites[n].size;
            if !Page::fits(class, bitmap.width, bitmap.height) {
                panic!(
                    "atlas: sprite {:?} ({}x{} logical) does not fit a page at scale {scale}",
                    SpriteId(n as u32),
                    size.width,
                    size.height
                );
            }
            let tile = self
                .place(class, &bitmap)
                .expect("it fits, and its format was checked at insert");
            self.sprites[n] = Sprite {
                tile,
                size,
                physical_width: bitmap.width,
                physical_height: bitmap.height,
            };
        }
        self.sources = sources;
        for i in 0..self.glyphs.len() {
            let font::Record { font, id, px, .. } = self.glyphs[i];
            self.glyphs[i].glyph = self.raster(font, id, px);
        }
        self.generation += 1;
        true
    }

    /// The lowest freed id, else the next, owned by `owner`.
    fn mint(&mut self, owner: Owner) -> AtlasId {
        match self.owners.iter().position(|o| *o == Owner::Free) {
            Some(i) => {
                self.owners[i] = owner;
                AtlasId(i as u32)
            }
            None => {
                self.owners.push(owner);
                AtlasId(self.owners.len() as u32 - 1)
            }
        }
    }

    /// Pack `bitmap` into a page of `class`, opening one when no existing
    /// page takes it, write the pixels, regenerate the mips under it and
    /// mark its cells. `bitmap.format` must be the class's.
    fn pack(&mut self, class: Class, bitmap: &Bitmap) -> Result<AtlasTile, Error> {
        if bitmap.format != class.format() {
            return Err(Error::Class);
        }
        if !Page::fits(class, bitmap.width, bitmap.height) {
            return Err(Error::TooLarge {
                width: bitmap.width,
                height: bitmap.height,
                max: page::PAGE - 2 * class.align(),
            });
        }
        let list = class as usize;
        let mut placed = None;
        for (i, page) in self.pages[list].iter_mut().enumerate() {
            if let Some(rect) = page.pack(bitmap.width, bitmap.height) {
                placed = Some((i, rect));
                break;
            }
        }
        let (i, rect) = match placed {
            Some(p) => p,
            None => {
                let i = self.pages[list].len();
                let id = self.mint(Owner::Page(class, i as u32));
                let mut page = Page::new(id, class);
                let rect = page
                    .pack(bitmap.width, bitmap.height)
                    .expect("an empty page takes what fits");
                self.pages[list].push(page);
                (i, rect)
            }
        };
        let page = &mut self.pages[list][i];
        if class == Class::Image {
            // Pages hold premultiplied pixels; the stored source stays straight.
            let mut pre = bitmap.clone();
            for px in pre.pixels.chunks_exact_mut(4) {
                let a = px[3] as u32;
                for c in &mut px[..3] {
                    *c = ((*c as u32 * a + 127) / 255) as u8;
                }
            }
            page.write(rect, &pre);
        } else {
            page.write(rect, bitmap);
        }
        if class.levels() > 1 {
            page.regenerate(rect);
        }
        Ok(AtlasTile {
            atlas: page.id(),
            bounds: rect,
        })
    }

    /// Insert an icon (`Class::Icon`, an `R8` bitmap) or an image
    /// (`Class::Image`, `Rgba8`) at its master size. The bitmap is device
    /// pixels; its logical size is its pixels over the scale at this call,
    /// for good. The atlas keeps a copy to re-pack on a rebuild, never
    /// resampled. There is no key: the caller keeps the id. `Error::Class`
    /// for any other class or a format the class does not take;
    /// `Error::TooLarge` for a bitmap no empty page of the class fits,
    /// which `Bitmap::fit` cures. A zero-dimension bitmap (`width == 0 ||
    /// height == 0`) never packs: it returns `Ok` with an empty tile,
    /// `AtlasTile { atlas: AtlasId::default(), bounds: Rect::ZERO }`,
    /// mirroring a glyph with no ink.
    pub fn insert(&mut self, class: Class, bitmap: &Bitmap) -> Result<SpriteId, Error> {
        if !matches!(class, Class::Icon | Class::Image) {
            return Err(Error::Class);
        }
        let tile = self.place(class, bitmap)?;
        let size = Size::new(
            bitmap.width as f32 / self.scale,
            bitmap.height as f32 / self.scale,
        );
        let source = Source::Bitmap {
            class,
            bitmap: bitmap.clone(),
        };
        Ok(self.push(tile, size, bitmap, source))
    }

    /// Insert an SVG as a `Class::Icon` whose longer side is `size`
    /// logical pixels, rasterized with `Bitmap::from_svg` at
    /// `ceil(size * scale)` and again on every rebuild. `Error::Svg` if it
    /// does not parse, `Error::TooLarge` if it cannot fit an empty page.
    #[cfg(feature = "svg")]
    pub fn insert_svg(
        &mut self,
        bytes: impl Into<Cow<'static, [u8]>>,
        size: u32,
    ) -> Result<SpriteId, Error> {
        let bytes = bytes.into();
        let bitmap = Bitmap::from_svg(&bytes, self.master(size))?;
        let tile = self.place(Class::Icon, &bitmap)?;
        let k = size as f32 / bitmap.width.max(bitmap.height) as f32;
        let logical = Size::new(bitmap.width as f32 * k, bitmap.height as f32 * k);
        Ok(self.push(tile, logical, &bitmap, Source::Svg { bytes, size }))
    }

    /// Insert an SVG as a coloured `Class::Image` whose longer side is
    /// `size` logical pixels, rasterized with `Bitmap::from_svg_rgba` at
    /// `ceil(size * scale)` and again on every rebuild. `Error::Svg` if it
    /// does not parse, `Error::TooLarge` if it cannot fit an empty page.
    #[cfg(feature = "svg")]
    pub fn insert_svg_image(
        &mut self,
        bytes: impl Into<Cow<'static, [u8]>>,
        size: u32,
    ) -> Result<SpriteId, Error> {
        let bytes = bytes.into();
        let bitmap = Bitmap::from_svg_rgba(&bytes, self.master(size))?;
        let tile = self.place(Class::Image, &bitmap)?;
        let k = size as f32 / bitmap.width.max(bitmap.height) as f32;
        let logical = Size::new(bitmap.width as f32 * k, bitmap.height as f32 * k);
        Ok(self.push(tile, logical, &bitmap, Source::SvgImage { bytes, size }))
    }

    /// Device pixels on an SVG's longer side for `size` logical.
    #[cfg(feature = "svg")]
    fn master(&self, size: u32) -> u32 {
        (size as f32 * self.scale).ceil() as u32
    }

    /// `pack`, but a zero-dimension bitmap takes no room: an empty tile,
    /// `AtlasTile { atlas: AtlasId::default(), bounds: Rect::ZERO }`.
    fn place(&mut self, class: Class, bitmap: &Bitmap) -> Result<AtlasTile, Error> {
        if bitmap.width == 0 || bitmap.height == 0 {
            return Ok(AtlasTile {
                atlas: AtlasId::default(),
                bounds: Rect::ZERO,
            });
        }
        self.pack(class, bitmap)
    }

    /// Record a new sprite and its source.
    fn push(&mut self, tile: AtlasTile, size: Size, bitmap: &Bitmap, source: Source) -> SpriteId {
        let id = SpriteId(self.sprites.len() as u32);
        self.sprites.push(Sprite {
            tile,
            size,
            physical_width: bitmap.width,
            physical_height: bitmap.height,
        });
        self.sources.push(source);
        id
    }

    /// The sprite `id` names. Panics on an id this atlas did not mint.
    pub fn sprite(&self, id: SpriteId) -> Sprite {
        self.sprites[id.0 as usize]
    }

    /// What `id` is: how a backend picks a format, a mip filter and an
    /// import path, and checks a tile reached the right primitive. Panics
    /// on an id this atlas did not mint or a rebuild freed.
    pub fn class(&self, id: AtlasId) -> Class {
        match self.owners[id.0 as usize] {
            Owner::Page(class, _) => class,
            Owner::External(_) => Class::External,
            Owner::Free => panic!("atlas: {id:?} was freed by a rebuild"),
        }
    }

    /// The page `id` names; `None` for an external, a freed id or an id
    /// this atlas did not mint.
    pub fn page(&self, id: AtlasId) -> Option<&Page> {
        match *self.owners.get(id.0 as usize)? {
            Owner::Page(class, i) => Some(&self.pages[class as usize][i as usize]),
            Owner::External(_) | Owner::Free => None,
        }
    }

    /// Every page, in id order.
    pub fn pages(&self) -> impl Iterator<Item = &Page> {
        self.owners.iter().filter_map(|o| match *o {
            Owner::Page(class, i) => Some(&self.pages[class as usize][i as usize]),
            Owner::External(_) | Owner::Free => None,
        })
    }

    /// Take every page's dirty mask and call `f` once per set cell with the
    /// page: class by class, each class's pages in creation order, cells
    /// rows then columns. A clean page costs four words: `take_dirty`
    /// always reads and clears all four `Cell<u64>` words of its mask.
    /// `&self`, so a backend calls it through `resource::<Atlas>()` and the
    /// drain does not re-arm `OnChanged<Atlas>`.
    pub fn drain_dirty(&self, mut f: impl FnMut(&Page, Cell)) {
        for list in &self.pages {
            for page in list.iter() {
                let mask = page.take_dirty();
                if mask == [0; 4] {
                    continue;
                }
                for cell in Page::cells(mask) {
                    f(page, cell);
                }
            }
        }
    }

    /// How many glyph records exist. For tests.
    #[cfg(test)]
    pub(crate) fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }
}

/// What can go wrong with input from outside. Ids the atlas minted are
/// never wrong, so nothing that takes one returns this.
#[derive(Debug)]
pub enum Error {
    /// fontdue rejected the bytes.
    Font(&'static str),
    /// `insert`: the class does not take that format, or is `Glyph` or
    /// `External`.
    Class,
    /// `insert`: the bitmap cannot fit an empty page of its class.
    TooLarge { width: u32, height: u32, max: u32 },
    /// The bytes are not a PNG this crate decodes.
    Png(png::DecodingError),
    /// The bytes are not an SVG resvg parses.
    #[cfg(feature = "svg")]
    Svg(resvg::usvg::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Font(why) => write!(f, "font rejected: {why}"),
            Error::Class => {
                f.write_str("class does not take that bitmap format, or takes no inserts")
            }
            Error::TooLarge { width, height, max } => write!(
                f,
                "bitmap {width}x{height} does not fit an empty page (max side {max})"
            ),
            Error::Png(e) => write!(f, "png: {e}"),
            #[cfg(feature = "svg")]
            Error::Svg(e) => write!(f, "svg: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Png(e) => Some(e),
            #[cfg(feature = "svg")]
            Error::Svg(e) => Some(e),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_describe_their_pages() {
        assert_eq!(Class::Glyph.format(), Format::R8);
        assert_eq!(Class::Icon.format(), Format::R8);
        assert_eq!(Class::Image.format(), Format::Rgba8);
        assert_eq!(Class::External.format(), Format::Rgba8);

        assert_eq!(Class::Glyph.levels(), 1);
        assert_eq!(Class::Icon.levels(), 5);
        assert_eq!(Class::Image.levels(), 5);
        assert_eq!(Class::External.levels(), 1);

        assert_eq!(Class::Glyph.align(), 1);
        assert_eq!(Class::Icon.align(), 16);
        assert_eq!(Class::Image.align(), 16);
        assert_eq!(Class::External.align(), 1);

        assert_eq!(Format::R8.bytes(), 1);
        assert_eq!(Format::Rgba8.bytes(), 4);
    }

    #[test]
    fn the_seam_types_are_plain_data() {
        assert_eq!(std::mem::size_of::<AtlasId>(), 4);
        assert_eq!(
            std::mem::size_of::<AtlasTile>(),
            20,
            "an id and four floats, no padding"
        );
        assert_eq!(AtlasId::default(), AtlasId(0));
        let a = AtlasTile {
            atlas: AtlasId(1),
            bounds: Rect::new(0.0, 0.0, 8.0, 8.0),
        };
        let b = AtlasTile {
            atlas: AtlasId(1),
            bounds: Rect::new(8.0, 0.0, 8.0, 8.0),
        };
        assert_ne!(a, b, "different bounds are different tiles");
    }
}
