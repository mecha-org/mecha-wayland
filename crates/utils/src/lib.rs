//! # utils
//!
//! Foundational geometric and color types used across the mecha-wayland workspace.
//!
//! All types are:
//! - `Copy` + `Clone` + `Debug`
//! - `bytemuck::Pod` / `Zeroable` where the raw memory layout matters (renderer upload)
//! - Built on [`glam`] so callers can reach for SIMD math when needed

pub use glam::{Vec2, Vec3, Vec4};

// ── Color ─────────────────────────────────────────────────────────────────────

/// An sRGB color with a linear alpha channel, stored as four `f32` values in [0, 1].
///
/// The in-memory layout is `[r, g, b, a]` — identical to `(f32, f32, f32, f32)` tuples
/// used previously, so it is `bytemuck::Pod` and can be uploaded directly to the GPU.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);

    /// Construct from normalized [0, 1] components with full opacity.
    #[inline]
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Construct from normalized [0, 1] components with explicit alpha.
    #[inline]
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Construct from 8-bit sRGB components `[0, 255]` with full opacity.
    #[inline]
    pub const fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    /// Construct from 8-bit sRGB components `[0, 255]` with normalized alpha.
    #[inline]
    pub const fn from_rgba8(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a,
        }
    }

    /// Return the color as a `[f32; 4]` array — useful for uniform uploads.
    #[inline]
    pub fn as_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Return the color as a `(f32, f32, f32, f32)` tuple.
    #[inline]
    pub fn as_tuple(self) -> (f32, f32, f32, f32) {
        (self.r, self.g, self.b, self.a)
    }

    /// Return a `glam::Vec4` representation.
    #[inline]
    pub fn to_vec4(self) -> Vec4 {
        Vec4::new(self.r, self.g, self.b, self.a)
    }
}

impl From<(f32, f32, f32, f32)> for Color {
    fn from((r, g, b, a): (f32, f32, f32, f32)) -> Self {
        Self { r, g, b, a }
    }
}

impl From<Color> for (f32, f32, f32, f32) {
    fn from(c: Color) -> Self {
        (c.r, c.g, c.b, c.a)
    }
}

impl From<Vec4> for Color {
    fn from(v: Vec4) -> Self {
        Self {
            r: v.x,
            g: v.y,
            b: v.z,
            a: v.w,
        }
    }
}

// ── Point ─────────────────────────────────────────────────────────────────────

/// A 2-D point in screen-space pixels (origin = top-left, Y grows downward).
///
/// Internally a `glam::Vec2` so arithmetic operators work out of the box.
#[repr(transparent)]
#[derive(Debug, Default, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Point(pub Vec2);

impl Point {
    pub const ZERO: Self = Self(Vec2::ZERO);

    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self(Vec2::new(x, y))
    }

    #[inline]
    pub fn x(self) -> f32 {
        self.0.x
    }

    #[inline]
    pub fn y(self) -> f32 {
        self.0.y
    }

    /// Convert to a `(f32, f32)` tuple.
    #[inline]
    pub fn as_tuple(self) -> (f32, f32) {
        (self.0.x, self.0.y)
    }
}

impl std::ops::Add<Size> for Point {
    type Output = Point;
    fn add(self, rhs: Size) -> Point {
        Point(self.0 + rhs.0)
    }
}

impl From<(f32, f32)> for Point {
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(x, y)
    }
}

impl From<Point> for (f32, f32) {
    fn from(p: Point) -> Self {
        (p.0.x, p.0.y)
    }
}

impl From<Vec2> for Point {
    fn from(v: Vec2) -> Self {
        Self(v)
    }
}

impl From<Point> for Vec2 {
    fn from(p: Point) -> Self {
        p.0
    }
}

// ── Size ──────────────────────────────────────────────────────────────────────

/// A 2-D extent in screen-space pixels (width × height, both non-negative).
#[repr(transparent)]
#[derive(Debug, Default, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Size(pub Vec2);

impl Size {
    pub const ZERO: Self = Self(Vec2::ZERO);

    #[inline]
    pub const fn new(width: f32, height: f32) -> Self {
        Self(Vec2::new(width, height))
    }

    #[inline]
    pub fn width(self) -> f32 {
        self.0.x
    }

    #[inline]
    pub fn height(self) -> f32 {
        self.0.y
    }

    /// Convert to a `(f32, f32)` tuple `(width, height)`.
    #[inline]
    pub fn as_tuple(self) -> (f32, f32) {
        (self.0.x, self.0.y)
    }
}

impl From<(f32, f32)> for Size {
    fn from((w, h): (f32, f32)) -> Self {
        Self::new(w, h)
    }
}

impl From<Size> for (f32, f32) {
    fn from(s: Size) -> Self {
        (s.0.x, s.0.y)
    }
}

impl From<Vec2> for Size {
    fn from(v: Vec2) -> Self {
        Self(v)
    }
}

impl From<Size> for Vec2 {
    fn from(s: Size) -> Self {
        s.0
    }
}

// ── Rect ──────────────────────────────────────────────────────────────────────

/// An axis-aligned rectangle in screen space.
///
/// `origin` is the **top-left** corner; `size` is `(width, height)`.  
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Top-left corner.
    pub origin: Point,
    /// Width and height.
    pub size: Size,
}

impl Rect {
    pub const ZERO: Self = Self {
        origin: Point::ZERO,
        size: Size::ZERO,
    };

    #[inline]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            origin: Point::new(x, y),
            size: Size::new(width, height),
        }
    }

    /// Shorthand for `Rect::new`.
    #[inline]
    pub const fn xywh(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self::new(x, y, w, h)
    }

    #[inline]
    pub fn x(self) -> f32 {
        self.origin.x()
    }

    #[inline]
    pub fn y(self) -> f32 {
        self.origin.y()
    }

    #[inline]
    pub fn width(self) -> f32 {
        self.size.width()
    }

    #[inline]
    pub fn height(self) -> f32 {
        self.size.height()
    }

    /// Returns the right edge (`x + width`).
    #[inline]
    pub fn right(self) -> f32 {
        self.x() + self.width()
    }

    /// Returns the bottom edge (`y + height`).
    #[inline]
    pub fn bottom(self) -> f32 {
        self.y() + self.height()
    }

    /// Returns the centre of the rectangle.
    #[inline]
    pub fn center(self) -> Point {
        Point::new(
            self.x() + self.width() / 2.0,
            self.y() + self.height() / 2.0,
        )
    }

    /// Returns `true` if the pixel coordinate `(px, py)` lies inside the rectangle.
    #[inline]
    pub fn contains(self, px: f64, py: f64) -> bool {
        let px = px as f32;
        let py = py as f32;
        px >= self.x() && px <= self.right() && py >= self.y() && py <= self.bottom()
    }
}
