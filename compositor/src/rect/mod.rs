//! Rectangle and region geometry.
//!
//! Algorithms adapted from Jay (`jay_algorithms::rect` / `RegionBuilder`).

mod algo;

use std::mem;
use std::ops::Deref;
use std::rc::Rc;

use smallvec::SmallVec;

#[allow(unused_imports)]
pub use algo::{extents, intersect, rects_to_bands, subtract, union, RectRaw};

/// Half-open rectangle: [x1, x2) × [y1, y2).
#[derive(Copy, Clone, Eq, PartialEq, Default, Debug)]
#[repr(transparent)]
pub struct Rect {
    pub raw: RectRaw,
}

impl Rect {
    #[inline]
    pub fn new(x1: i32, y1: i32, x2: i32, y2: i32) -> Option<Self> {
        if x2 < x1 || y2 < y1 {
            return None;
        }
        Some(Self {
            raw: RectRaw { x1, y1, x2, y2 },
        })
    }

    #[inline]
    pub fn new_sized_saturating(x1: i32, y1: i32, width: i32, height: i32) -> Self {
        Self {
            raw: RectRaw {
                x1,
                y1,
                x2: x1.saturating_add(width.max(0)),
                y2: y1.saturating_add(height.max(0)),
            },
        }
    }

    #[inline]
    pub fn x1(self) -> i32 {
        self.raw.x1
    }
    #[inline]
    pub fn y1(self) -> i32 {
        self.raw.y1
    }
    #[inline]
    pub fn x2(self) -> i32 {
        self.raw.x2
    }
    #[inline]
    pub fn y2(self) -> i32 {
        self.raw.y2
    }
    #[inline]
    pub fn width(self) -> i32 {
        self.raw.x2 - self.raw.x1
    }
    #[inline]
    pub fn height(self) -> i32 {
        self.raw.y2 - self.raw.y1
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.raw.x1 == self.raw.x2 || self.raw.y1 == self.raw.y2
    }

    #[inline]
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.raw.x1 && x < self.raw.x2 && y >= self.raw.y1 && y < self.raw.y2
    }

    #[inline]
    pub fn intersects(self, other: Self) -> bool {
        self.raw.x1 < other.raw.x2
            && other.raw.x1 < self.raw.x2
            && self.raw.y1 < other.raw.y2
            && other.raw.y1 < self.raw.y2
    }

    #[inline]
    pub fn union(self, other: Self) -> Self {
        Self {
            raw: RectRaw {
                x1: self.raw.x1.min(other.raw.x1),
                y1: self.raw.y1.min(other.raw.y1),
                x2: self.raw.x2.max(other.raw.x2),
                y2: self.raw.y2.max(other.raw.y2),
            },
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct Region {
    rects: SmallVec<[RectRaw; 1]>,
    extents: Rect,
}

thread_local! {
    static EMPTY: Rc<Region> = Rc::new(Region {
        rects: Default::default(),
        extents: Default::default(),
    });
}

impl Region {
    #[inline]
    pub fn empty() -> Rc<Self> {
        EMPTY.with(|e| e.clone())
    }

    pub fn from_rects(rects: &[Rect]) -> Rc<Self> {
        if rects.is_empty() {
            return Self::empty();
        }
        let region = Self::from_rects2(rects);
        if region.is_empty() {
            return Self::empty();
        }
        Rc::new(region)
    }

    pub fn from_rects2(rects: &[Rect]) -> Self {
        if rects.is_empty() {
            return Self::default();
        }
        if rects.len() == 1 {
            return Self::new(rects[0]);
        }
        // SAFETY: Rect is #[repr(transparent)] over RectRaw.
        let rects = rects_to_bands(unsafe { mem::transmute::<&[Rect], &[RectRaw]>(rects) });
        if rects.is_empty() {
            return Self::default();
        }
        Self {
            extents: Rect {
                raw: extents(&rects),
            },
            rects,
        }
    }

    pub fn new(rect: Rect) -> Self {
        if rect.is_empty() {
            return Self::default();
        }
        let mut rects = SmallVec::new();
        rects.push(rect.raw);
        Self {
            rects,
            extents: rect,
        }
    }

    #[inline]
    pub fn extents(&self) -> Rect {
        self.extents
    }

    #[inline]
    pub fn rects(&self) -> &[Rect] {
        // SAFETY: Rect is #[repr(transparent)] over RectRaw.
        unsafe { mem::transmute::<&[RectRaw], &[Rect]>(&self.rects[..]) }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty() || self.extents.is_empty()
    }

    pub fn union(self: &Rc<Self>, other: &Rc<Self>) -> Rc<Self> {
        if self.extents.is_empty() {
            return other.clone();
        }
        if other.extents.is_empty() {
            return self.clone();
        }
        let rects = union(&self.rects, &other.rects);
        if rects.is_empty() {
            return Self::empty();
        }
        Rc::new(Self {
            rects,
            extents: self.extents.union(other.extents),
        })
    }

    pub fn subtract(self: &Rc<Self>, other: &Rc<Self>) -> Rc<Self> {
        if self.extents.is_empty() || other.extents.is_empty() {
            return self.clone();
        }
        // AABB miss: nothing to remove.
        if !self.extents.intersects(other.extents) {
            return self.clone();
        }
        let rects = subtract(&self.rects, &other.rects);
        if rects.is_empty() {
            return Self::empty();
        }
        Rc::new(Self {
            extents: Rect {
                raw: extents(&rects),
            },
            rects,
        })
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        if !self.extents.contains(x, y) {
            return false;
        }
        self.rects().iter().any(|r| r.contains(x, y))
    }
}

impl Deref for Region {
    type Target = [Rect];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.rects()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
enum BuilderOp {
    #[default]
    Add,
    Sub,
}

#[derive(Debug)]
pub struct RegionBuilder {
    base: Rc<Region>,
    op: BuilderOp,
    pending: Vec<Rect>,
}

impl Default for RegionBuilder {
    fn default() -> Self {
        Self {
            base: Region::empty(),
            op: Default::default(),
            pending: Default::default(),
        }
    }
}

impl RegionBuilder {
    #[inline]
    pub fn add(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        self.set_op(BuilderOp::Add);
        self.pending.push(rect);
    }

    #[inline]
    pub fn sub(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        self.set_op(BuilderOp::Sub);
        self.pending.push(rect);
    }

    pub fn get(&mut self) -> Rc<Region> {
        self.flush();
        self.base.clone()
    }

    fn set_op(&mut self, op: BuilderOp) {
        if self.op != op {
            self.flush();
            self.op = op;
        }
    }

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let region = Region::from_rects(&self.pending);
        self.base = match self.op {
            BuilderOp::Add => self.base.union(&region),
            BuilderOp::Sub => self.base.subtract(&region),
        };
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(r: &Region) -> i64 {
        r.rects()
            .iter()
            .map(|r| r.width() as i64 * r.height() as i64)
            .sum()
    }

    fn covers(r: &Region, x: i32, y: i32) -> bool {
        r.contains(x, y)
    }

    #[test]
    fn union_overlap() {
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 10, 10));
        b.add(Rect::new_sized_saturating(5, 0, 10, 10));
        let r = b.get();
        assert_eq!(area(&r), 150);
        assert!(covers(&r, 7, 5));
        assert!(!covers(&r, 20, 5));
    }

    #[test]
    fn subtract_hole() {
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 10, 10));
        b.sub(Rect::new_sized_saturating(2, 2, 6, 6));
        let r = b.get();
        assert_eq!(area(&r), 64);
        assert!(!covers(&r, 5, 5));
        assert!(covers(&r, 0, 0));
        assert!(covers(&r, 9, 9));
    }

    #[test]
    fn multi_subtract() {
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 20, 20));
        b.sub(Rect::new_sized_saturating(0, 0, 5, 5));
        b.sub(Rect::new_sized_saturating(15, 15, 5, 5));
        let r = b.get();
        assert_eq!(area(&r), 400 - 25 - 25);
    }

    #[test]
    fn l_shape() {
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 10, 30));
        b.add(Rect::new_sized_saturating(0, 0, 30, 10));
        let r = b.get();
        assert_eq!(area(&r), 500);
    }

    #[test]
    fn subtract_disjoint_is_clone() {
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 10, 10));
        let base = b.get();
        let mut other = RegionBuilder::default();
        other.add(Rect::new_sized_saturating(100, 100, 5, 5));
        let out = base.subtract(&other.get());
        assert!(Rc::ptr_eq(&base, &out));
    }

    #[test]
    fn empty_single_rect_fast_path() {
        let r = Region::from_rects(&[Rect::new_sized_saturating(0, 0, 0, 10)]);
        assert!(r.is_empty());
        assert!(Rc::ptr_eq(&r, &Region::empty()));
        assert!(Region::new(Rect::new_sized_saturating(5, 5, 0, 0)).is_empty());
    }

    #[test]
    fn full_subtract_is_shared_empty() {
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 10, 10));
        b.sub(Rect::new_sized_saturating(0, 0, 10, 10));
        let r = b.get();
        assert!(r.is_empty());
        assert!(Rc::ptr_eq(&r, &Region::empty()));
    }

    #[test]
    fn touching_edges_merge() {
        // Two rects that touch on the right edge should form one 20×10 band.
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 10, 10));
        b.add(Rect::new_sized_saturating(10, 0, 10, 10));
        let r = b.get();
        assert_eq!(area(&r), 200);
        assert_eq!(r.rects().len(), 1);
        assert_eq!(r.rects()[0].width(), 20);
    }

    #[test]
    fn vertical_stack_coalesces() {
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 10, 10));
        b.add(Rect::new_sized_saturating(0, 10, 10, 10));
        let r = b.get();
        assert_eq!(area(&r), 200);
        assert_eq!(r.rects().len(), 1);
        assert_eq!(r.rects()[0].height(), 20);
    }

    #[test]
    fn add_sub_add_alternation() {
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 20, 20));
        b.sub(Rect::new_sized_saturating(5, 5, 10, 10));
        b.add(Rect::new_sized_saturating(7, 7, 2, 2)); // fill center of hole
        let r = b.get();
        // 400 - 100 + 4 = 304
        assert_eq!(area(&r), 304);
        assert!(covers(&r, 8, 8));
        assert!(!covers(&r, 5, 5));
    }

    #[test]
    fn many_horizontal_fragments() {
        // Subtract vertical slits → several side-by-side bands.
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 30, 10));
        b.sub(Rect::new_sized_saturating(10, 0, 5, 10));
        b.sub(Rect::new_sized_saturating(20, 0, 5, 10));
        let r = b.get();
        assert_eq!(area(&r), 300 - 50 - 50);
        assert!(covers(&r, 0, 0));
        assert!(!covers(&r, 12, 0));
        assert!(covers(&r, 16, 0));
        assert!(!covers(&r, 22, 0));
        assert!(covers(&r, 28, 0));
        // Three remaining slabs in one band.
        assert_eq!(r.rects().len(), 3);
    }

    #[test]
    fn empty_add_is_noop() {
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 0, 0));
        b.add(Rect::new_sized_saturating(0, 0, -1, 5)); // saturates to empty
        let r = b.get();
        assert!(r.is_empty());
        assert!(Rc::ptr_eq(&r, &Region::empty()));
    }

    #[test]
    fn copy_semantics_snapshot() {
        let mut b = RegionBuilder::default();
        b.add(Rect::new_sized_saturating(0, 0, 10, 10));
        let snap = b.get();
        b.add(Rect::new_sized_saturating(10, 0, 10, 10));
        let later = b.get();
        assert_eq!(area(&snap), 100);
        assert_eq!(area(&later), 200);
        assert!(!Rc::ptr_eq(&snap, &later));
    }
}
