//! One page: a square texture of one class with a CPU copy of every mip
//! level, a shelf packer, and a dirty mask of 16 by 16 cells that serves
//! every level at once.

use geometry::Rect;

use crate::{AtlasId, Bitmap, Class};

/// A page's side in pixels.
pub const PAGE: u32 = 2048;
/// A dirty cell's side at level 0. It halves per level.
pub const CELL: u32 = 128;
/// Cells per side, at every level.
pub const CELLS: u32 = 16;

/// One square of a page's dirty grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub col: u8,
    pub row: u8,
}

/// One row of the packer. `x` is where the next rectangle goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shelf {
    y: u32,
    height: u32,
    x: u32,
}

/// One texture of one class.
#[derive(Debug)]
pub struct Page {
    id: AtlasId,
    class: Class,
    /// `levels[k]` is `(PAGE >> k)^2 * bytes` bytes, row-major.
    levels: Vec<Vec<u8>>,
    /// 256 bits, row-major: bit `row * 16 + col`. `Cell` so a backend
    /// takes it through a shared reference: `resource_mut` would count
    /// the drain as a write and re-arm `OnChanged<Atlas>`.
    dirty: [std::cell::Cell<u64>; 4],
    shelves: Vec<Shelf>,
}

fn align_up(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}

impl Page {
    /// A zero page with every cell dirty and no shelf.
    pub(crate) fn new(id: AtlasId, class: Class) -> Page {
        let bytes = class.format().bytes();
        let levels = (0..class.levels())
            .map(|k| {
                let side = Self::dims(k) as usize;
                vec![0u8; side * side * bytes]
            })
            .collect();
        Page {
            id,
            class,
            levels,
            dirty: [const { std::cell::Cell::new(u64::MAX) }; 4],
            shelves: Vec::new(),
        }
    }

    pub fn id(&self) -> AtlasId {
        self.id
    }

    pub fn class(&self) -> Class {
        self.class
    }

    /// The side, `PAGE`.
    pub fn size(&self) -> u32 {
        PAGE
    }

    /// `Class::levels`.
    pub fn levels(&self) -> u32 {
        self.class.levels()
    }

    /// The side of level `k`.
    pub(crate) fn dims(k: u32) -> u32 {
        PAGE >> k
    }

    /// Level `k`, row-major, tightly packed; premultiplied RGBA on an
    /// `Image` page. Panics past `levels()`.
    pub fn level(&self, k: u32) -> &[u8] {
        &self.levels[k as usize]
    }

    pub(crate) fn level_mut(&mut self, k: u32) -> &mut [u8] {
        &mut self.levels[k as usize]
    }

    /// The slot a `width` by `height` rectangle of this class takes, border
    /// included, and the border on each side.
    fn slot(class: Class, width: u32, height: u32) -> (u32, u32, u32) {
        let a = class.align();
        (align_up(width, a) + 2 * a, align_up(height, a) + 2 * a, a)
    }

    /// Would an empty page of `class` take the rectangle.
    pub(crate) fn fits(class: Class, width: u32, height: u32) -> bool {
        let (w, h, _) = Self::slot(class, width, height);
        w <= PAGE && h <= PAGE
    }

    /// Reserve a `width` by `height` rectangle. Returns its content rect,
    /// border excluded, or `None` if no shelf and no new shelf can take
    /// it. The shelf of matching height with the least waste wins; a
    /// shelf's height is fixed when it opens; no shelf ever closes.
    pub(crate) fn pack(&mut self, width: u32, height: u32) -> Option<Rect> {
        let (w, h, border) = Self::slot(self.class, width, height);
        let mut best: Option<(usize, u32)> = None;
        for (i, s) in self.shelves.iter().enumerate() {
            if s.height >= h && s.x + w <= PAGE {
                let waste = s.height - h;
                if best.map_or(true, |(_, b)| waste < b) {
                    best = Some((i, waste));
                }
            }
        }
        let (x, y) = match best {
            Some((i, _)) => {
                let s = &mut self.shelves[i];
                let x = s.x;
                s.x += w;
                (x, s.y)
            }
            None => {
                let y = self.shelves.last().map_or(0, |s| s.y + s.height);
                if y + h > PAGE || w > PAGE {
                    return None;
                }
                self.shelves.push(Shelf { y, height: h, x: w });
                (0, y)
            }
        };
        Some(Rect::new(
            (x + border) as f32,
            (y + border) as f32,
            width as f32,
            height as f32,
        ))
    }

    /// Copy `bitmap` into level 0 at `rect` and mark the cells it touches.
    /// `rect` is what `pack` returned for the bitmap's size; the format is
    /// the class's.
    pub(crate) fn write(&mut self, rect: Rect, bitmap: &Bitmap) {
        debug_assert_eq!(bitmap.format, self.class.format());
        debug_assert_eq!(bitmap.width as f32, rect.width());
        debug_assert_eq!(bitmap.height as f32, rect.height());
        let bytes = self.class.format().bytes();
        let stride = PAGE as usize * bytes;
        let x0 = rect.x() as usize * bytes;
        let y0 = rect.y() as usize;
        let row = bitmap.width as usize * bytes;
        let level = &mut self.levels[0];
        for r in 0..bitmap.height as usize {
            let dst = (y0 + r) * stride + x0;
            level[dst..dst + row].copy_from_slice(&bitmap.pixels[r * row..(r + 1) * row]);
        }
        self.mark(rect);
    }

    /// Mark every cell `rect` touches, at level 0 coordinates.
    pub(crate) fn mark(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        let c0 = (rect.x() as u32 / CELL).min(CELLS - 1);
        let r0 = (rect.y() as u32 / CELL).min(CELLS - 1);
        let c1 = ((rect.right().ceil() as u32).saturating_sub(1) / CELL).min(CELLS - 1);
        let r1 = ((rect.bottom().ceil() as u32).saturating_sub(1) / CELL).min(CELLS - 1);
        for row in r0..=r1 {
            for col in c0..=c1 {
                let bit = row * CELLS + col;
                let word = &self.dirty[(bit / 64) as usize];
                word.set(word.get() | 1 << (bit % 64));
            }
        }
    }

    /// The mask, cleared. `&self`: see the field.
    pub(crate) fn take_dirty(&self) -> [u64; 4] {
        [
            self.dirty[0].take(),
            self.dirty[1].take(),
            self.dirty[2].take(),
            self.dirty[3].take(),
        ]
    }

    /// The cells set in `mask`, rows then columns.
    pub(crate) fn cells(mask: [u64; 4]) -> impl Iterator<Item = Cell> {
        (0..CELLS * CELLS)
            .filter(move |bit| mask[(bit / 64) as usize] & (1 << (bit % 64)) != 0)
            .map(|bit| Cell {
                col: (bit % CELLS) as u8,
                row: (bit / CELLS) as u8,
            })
    }

    /// The cell's pixels at level `k`, `(CELL >> k)^2 * bytes` of them,
    /// row-major and contiguous, into `out`, which is cleared first. What
    /// a GLES 2.0 sub-image upload needs, since it has no unpack row
    /// length.
    pub fn cell(&self, k: u32, cell: Cell, out: &mut Vec<u8>) {
        let bytes = self.class.format().bytes();
        let side = (CELL >> k) as usize;
        let stride = Self::dims(k) as usize * bytes;
        let x0 = cell.col as usize * side * bytes;
        let y0 = cell.row as usize * side;
        let level = &self.levels[k as usize];
        out.clear();
        out.reserve(side * side * bytes);
        for r in 0..side {
            let src = (y0 + r) * stride + x0;
            out.extend_from_slice(&level[src..src + side * bytes]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Format;

    fn page(class: Class) -> Page {
        Page::new(AtlasId(7), class)
    }

    fn solid(width: u32, height: u32, format: Format, value: u8) -> Bitmap {
        Bitmap {
            width,
            height,
            format,
            pixels: vec![value; (width * height) as usize * format.bytes()],
        }
    }

    #[test]
    fn a_new_page_is_zero_and_all_dirty() {
        let p = page(Class::Glyph);
        assert_eq!(p.id(), AtlasId(7));
        assert_eq!(p.class(), Class::Glyph);
        assert_eq!(p.size(), PAGE);
        assert_eq!(p.levels(), 1);
        assert_eq!(p.level(0).len(), (PAGE * PAGE) as usize);
        assert!(p.level(0).iter().all(|&b| b == 0));
        assert_eq!(p.take_dirty(), [u64::MAX; 4]);
        assert_eq!(p.take_dirty(), [0; 4], "taking clears");
    }

    #[test]
    fn an_image_page_holds_five_levels() {
        let p = page(Class::Image);
        assert_eq!(p.levels(), 5);
        for k in 0..5 {
            let side = (PAGE >> k) as usize;
            assert_eq!(p.level(k).len(), side * side * 4, "level {k}");
        }
    }

    #[test]
    fn glyph_padding_is_one_pixel_and_bounds_exclude_it() {
        let mut p = page(Class::Glyph);
        let a = p.pack(10, 12).unwrap();
        assert_eq!(a, Rect::new(1.0, 1.0, 10.0, 12.0));
        let b = p.pack(5, 12).unwrap();
        assert_eq!(
            b,
            Rect::new(13.0, 1.0, 5.0, 12.0),
            "same shelf, to the right, one pixel apart"
        );
    }

    #[test]
    fn icon_padding_rounds_to_sixteen_and_offsets_by_sixteen() {
        let mut p = page(Class::Icon);
        let a = p.pack(100, 50).unwrap();
        assert_eq!(a, Rect::new(16.0, 16.0, 100.0, 50.0));
        // slot was 112 + 32 wide, 64 + 32 tall
        let b = p.pack(20, 60).unwrap();
        assert_eq!(
            b,
            Rect::new(160.0, 16.0, 20.0, 60.0),
            "same shelf: 60 rounds to 64, fits the 64 shelf"
        );
        let c = p.pack(10, 70).unwrap();
        assert_eq!(
            c,
            Rect::new(16.0, 112.0, 10.0, 70.0),
            "taller than the shelf: a new shelf under it, 16-aligned"
        );
    }

    #[test]
    fn the_least_wasteful_shelf_wins() {
        let mut p = page(Class::Glyph);
        p.pack(10, 10).unwrap(); // shelf of height 12 at y 0
        p.pack(10, 30).unwrap(); // too tall for it: shelf of height 32 at y 12
        let r = p.pack(10, 9).unwrap();
        assert_eq!(
            r,
            Rect::new(13.0, 1.0, 10.0, 9.0),
            "the 12 shelf wastes 1, the 32 shelf 21"
        );
        let t = p.pack(10, 20).unwrap();
        assert_eq!(
            t,
            Rect::new(13.0, 13.0, 10.0, 20.0),
            "only the 32 shelf takes a 22 slot"
        );
    }

    #[test]
    fn a_full_page_says_so() {
        let mut p = page(Class::Glyph);
        // Content PAGE - 2 wide fills a shelf exactly (1 px pad each side);
        // 100 tall content takes 102 rows.
        let w = PAGE - 2;
        let shelves = PAGE / 102;
        for _ in 0..shelves {
            assert!(p.pack(w, 100).is_some());
        }
        assert!(
            p.pack(w, 100).is_none(),
            "one more shelf would end past the page"
        );
        assert!(
            PAGE - shelves * 102 >= 4,
            "the test needs 4 rows left for the small rect"
        );
        assert!(
            p.pack(2, 2).is_some(),
            "but a small one opens a last shelf in the rows left"
        );
        assert!(Page::fits(Class::Glyph, PAGE - 2, PAGE - 2));
        assert!(!Page::fits(Class::Glyph, PAGE - 1, 1));
        assert!(Page::fits(Class::Image, PAGE - 32, PAGE - 32));
        assert!(!Page::fits(Class::Image, PAGE - 31, 16));
    }

    #[test]
    fn write_copies_pixels_and_marks_the_cells_it_touches() {
        let mut p = page(Class::Glyph);
        p.take_dirty();
        let r = p.pack(3, 2).unwrap(); // at (1, 1)
        p.write(r, &solid(3, 2, Format::R8, 9));
        let l0 = p.level(0);
        assert_eq!(&l0[(PAGE as usize) + 1..(PAGE as usize) + 4], &[9, 9, 9]);
        assert_eq!(
            &l0[(2 * PAGE as usize) + 1..(2 * PAGE as usize) + 4],
            &[9, 9, 9]
        );
        assert_eq!(l0[(PAGE as usize) + 4], 0, "nothing past the rect");
        let cells: Vec<Cell> = Page::cells(p.take_dirty()).collect();
        assert_eq!(cells, vec![Cell { col: 0, row: 0 }]);
    }

    #[test]
    fn a_rect_across_cell_edges_marks_each_cell() {
        let mut p = page(Class::Glyph);
        p.take_dirty();
        // 10 px straddling the first column edge and the second row edge.
        let c = CELL as f32;
        p.mark(Rect::new(c - 4.0, 2.0 * c - 8.0, 10.0, 10.0));
        let cells: Vec<Cell> = Page::cells(p.take_dirty()).collect();
        assert_eq!(
            cells,
            vec![
                Cell { col: 0, row: 1 },
                Cell { col: 1, row: 1 },
                Cell { col: 0, row: 2 },
                Cell { col: 1, row: 2 },
            ],
            "rows then columns"
        );
    }

    #[test]
    fn cell_copies_contiguously_at_any_level() {
        let mut p = page(Class::Image);
        p.take_dirty();
        // Paint the first pixel of cell (1, 0) red at level 0 and green at level 2.
        let (c0, c2) = (CELL as usize, (CELL >> 2) as usize);
        {
            let l0 = p.level_mut(0);
            let i = c0 * 4;
            l0[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
        {
            let l2 = p.level_mut(2);
            let i = c2 * 4;
            l2[i..i + 4].copy_from_slice(&[0, 255, 0, 255]);
        }
        let mut out = Vec::new();
        p.cell(0, Cell { col: 1, row: 0 }, &mut out);
        assert_eq!(out.len(), c0 * c0 * 4);
        assert_eq!(
            &out[0..4],
            &[255, 0, 0, 255],
            "the cell's first pixel is page pixel (CELL, 0)"
        );
        p.cell(2, Cell { col: 1, row: 0 }, &mut out);
        assert_eq!(out.len(), c2 * c2 * 4, "a cell is CELL / 4 wide at level 2");
        assert_eq!(&out[0..4], &[0, 255, 0, 255]);
        assert_eq!(Page::dims(0), PAGE);
        assert_eq!(Page::dims(4), PAGE >> 4);
        assert_eq!(CELL * CELLS, PAGE, "the dirty grid covers the page");
    }
}
