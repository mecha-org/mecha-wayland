//! The atlas at a scale: device pixels in the pages, logical sizes out,
//! and a rebuild on every change that keeps ids and layout.

use atlas::prelude::*;
use geometry::Size;

const INTER: &[u8] = include_bytes!("fixtures/Inter-Regular.ttf");

#[cfg(feature = "svg")]
const SQUARE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 10 10"><rect x="0" y="0" width="10" height="10" fill="black"/></svg>"#;

fn rgba(width: u32, height: u32) -> Bitmap {
    Bitmap {
        width,
        height,
        format: Format::Rgba8,
        pixels: vec![128; (width * height * 4) as usize],
    }
}

fn g_at(scale: f32) -> Glyph {
    let mut atlas = Atlas::new();
    atlas.set_scale(scale);
    let inter = atlas.add_font(INTER).unwrap();
    let (_, g) = atlas.lookup(&[inter], 'g').unwrap();
    atlas.glyph(inter, g, 14)
}

#[test]
fn a_glyph_at_scale_two_is_twice_the_pixels_and_the_same_layout() {
    let g1 = g_at(1.0);
    let g2 = g_at(2.0);
    assert!(
        (g2.physical_width as i64 - 2 * g1.physical_width as i64).abs() <= 1,
        "{} vs 2 * {}",
        g2.physical_width,
        g1.physical_width
    );
    assert_eq!(g2.advance, g1.advance, "layout does not move with scale");
    assert!((g2.left - g1.left).abs() <= 0.5);
    assert!((g2.top - g1.top).abs() <= 0.5);
    assert_eq!(g2.tile.bounds.width(), g2.physical_width as f32);
    assert_eq!(g2.tile.bounds.height(), g2.physical_height as f32);
    assert_eq!(
        g2.size.width,
        g2.physical_width as f32 / 2.0,
        "ink size is logical"
    );
}

#[cfg(feature = "svg")]
#[test]
fn an_svg_icon_follows_the_scale() {
    let mut atlas = Atlas::new();
    let id = atlas.insert_svg(SQUARE_SVG, 20).unwrap();
    let s1 = atlas.sprite(id);
    assert_eq!(s1.physical_width, 20);
    assert_eq!(s1.size, Size::new(20.0, 20.0));
    assert_eq!(atlas.class(s1.tile.atlas), Class::Icon);
    assert!(atlas.set_scale(2.0));
    let s2 = atlas.sprite(id);
    assert_eq!((s2.physical_width, s2.physical_height), (40, 40));
    assert_eq!(s2.size, Size::new(20.0, 20.0), "the logical size stays");
    assert_eq!(s2.tile.bounds.width(), 40.0);
}

#[cfg(feature = "svg")]
#[test]
fn an_svg_image_follows_the_scale() {
    let mut atlas = Atlas::new();
    let id = atlas.insert_svg_image(SQUARE_SVG, 20).unwrap();
    let s1 = atlas.sprite(id);
    assert_eq!(s1.physical_width, 20);
    assert_eq!(s1.size, Size::new(20.0, 20.0));
    assert_eq!(atlas.class(s1.tile.atlas), Class::Image);
    assert!(atlas.set_scale(2.0));
    let s2 = atlas.sprite(id);
    assert_eq!((s2.physical_width, s2.physical_height), (40, 40));
    assert_eq!(s2.size, Size::new(20.0, 20.0), "the logical size stays");
    assert_eq!(s2.tile.bounds.width(), 40.0);
}

#[test]
fn a_raw_bitmap_keeps_its_logical_size() {
    let mut atlas = Atlas::new();
    atlas.set_scale(2.0);
    let id = atlas.insert(Class::Image, &rgba(40, 40)).unwrap();
    let s = atlas.sprite(id);
    assert_eq!(s.size, Size::new(20.0, 20.0));
    assert_eq!(s.physical_width, 40);
    assert!(atlas.set_scale(1.0));
    let s = atlas.sprite(id);
    assert_eq!(s.size, Size::new(20.0, 20.0), "fixed at insert");
    assert_eq!(
        (s.physical_width, s.physical_height),
        (40, 40),
        "re-packed, not resampled"
    );
    assert_eq!(s.tile.bounds.width(), 40.0);
}

#[test]
fn set_scale_bumps_the_generation_and_dirties_every_page() {
    let mut atlas = Atlas::new();
    assert_eq!(atlas.scale(), 1.0);
    assert_eq!(atlas.generation(), 0);
    let inter = atlas.add_font(INTER).unwrap();
    atlas.warm(inter, 14, 'a'..='z');
    atlas.insert(Class::Image, &rgba(8, 8)).unwrap();
    atlas.drain_dirty(|_, _| {});
    assert!(atlas.set_scale(2.0));
    assert_eq!(atlas.scale(), 2.0);
    assert_eq!(atlas.generation(), 1);
    assert!(!atlas.set_scale(2.0), "the same scale is a no-op");
    assert_eq!(atlas.generation(), 1);
    let pages = atlas.pages().count();
    assert!(pages >= 2);
    let mut cells = 0;
    atlas.drain_dirty(|_, _| cells += 1);
    assert_eq!(cells, 256 * pages, "every new page is dirty whole");
}

#[cfg(feature = "svg")]
#[test]
fn rescaling_many_times_keeps_ids_bounded() {
    use std::os::fd::OwnedFd;
    use std::sync::Arc;

    let mut atlas = Atlas::new();
    let inter = atlas.add_font(INTER).unwrap();
    let (_, a) = atlas.lookup(&[inter], 'a').unwrap();
    atlas.glyph(inter, a, 14);
    let icon = atlas.insert_svg(SQUARE_SVG, 24).unwrap();
    let image = atlas.insert(Class::Image, &rgba(16, 16)).unwrap();
    let feed = atlas.external(External {
        size: Size::new(64.0, 64.0),
        fourcc: 0x34325241, // AR24
        modifier: 0,
        planes: vec![Plane {
            fd: Arc::new(OwnedFd::from(std::fs::File::open("/dev/null").unwrap())),
            offset: 0,
            stride: 256,
        }],
        generation: 0,
    });
    for n in 0..50 {
        atlas.set_scale(if n % 2 == 0 { 2.0 } else { 1.0 });
        assert!(atlas.pages().map(|p| p.id().0).max().unwrap() < 4);
        assert_eq!(
            atlas.externals().map(|(id, _)| id).collect::<Vec<_>>(),
            vec![feed]
        );
        assert_eq!(atlas.class(feed), Class::External);
        assert_eq!(atlas.class(atlas.sprite(icon).tile.atlas), Class::Icon);
        assert_eq!(atlas.class(atlas.sprite(image).tile.atlas), Class::Image);
        let glyph = atlas.glyph(inter, a, 14).tile.atlas;
        assert_eq!(atlas.class(glyph), Class::Glyph);
    }
    assert_eq!(atlas.generation(), 50);
}

#[cfg(feature = "svg")]
#[test]
#[should_panic(expected = "does not fit a page at scale 2")]
fn a_sprite_too_large_at_the_new_scale_panics() {
    let mut atlas = Atlas::new();
    atlas.insert_svg(SQUARE_SVG, 600).unwrap();
    atlas.set_scale(2.0);
}

#[test]
fn advance_needs_no_mut_and_matches_glyph() {
    let mut atlas = Atlas::new();
    let inter = atlas.add_font(INTER).unwrap();
    let (_, id) = atlas.lookup(&[inter], 'g').unwrap();
    let a: &Atlas = &atlas;
    let advance = a.advance(inter, id, 14);
    assert!(advance > 0.0);
    assert_eq!(atlas.pages().count(), 0, "advance rasterizes nothing");
    assert_eq!(advance, atlas.glyph(inter, id, 14).advance);
}
