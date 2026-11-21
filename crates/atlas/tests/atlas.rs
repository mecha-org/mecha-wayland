//! The atlas as a caller and a backend see it, end to end.

use atlas::prelude::*;
use atlas::{CELL, CELLS, PAGE};
use geometry::Rect;

fn solid(width: u32, height: u32, format: Format, value: u8) -> Bitmap {
    Bitmap {
        width,
        height,
        format,
        pixels: vec![value; (width * height) as usize * format.bytes()],
    }
}

fn icon(side: u32) -> Bitmap {
    solid(side, side, Format::R8, 255)
}

fn image(w: u32, h: u32) -> Bitmap {
    solid(w, h, Format::Rgba8, 128)
}

#[test]
fn ids_are_dense_across_classes_and_class_answers_each() {
    let mut atlas = Atlas::new();
    let a = atlas.insert(Class::Icon, &icon(32)).unwrap();
    let b = atlas.insert(Class::Image, &image(32, 16)).unwrap();
    assert_eq!(
        (a, b),
        (SpriteId(0), SpriteId(1)),
        "sprites count icons and images together"
    );
    let ta = atlas.sprite(a).tile;
    let tb = atlas.sprite(b).tile;
    assert_eq!(ta.atlas, AtlasId(0), "the first page made");
    assert_eq!(tb.atlas, AtlasId(1), "the second, of another class");
    assert_eq!(atlas.class(AtlasId(0)), Class::Icon);
    assert_eq!(atlas.class(AtlasId(1)), Class::Image);
    assert_eq!(atlas.pages().count(), 2);
    assert_eq!(
        atlas.pages().map(|p| p.id()).collect::<Vec<_>>(),
        vec![AtlasId(0), AtlasId(1)]
    );
}

#[test]
fn a_sprite_records_its_master_size_and_padded_bounds() {
    let mut atlas = Atlas::new();
    let a = atlas.insert(Class::Image, &image(100, 50)).unwrap();
    let s = atlas.sprite(a);
    assert_eq!(s.size, geometry::Size::new(100.0, 50.0));
    assert_eq!(s.tile.bounds, Rect::new(16.0, 16.0, 100.0, 50.0));
}

#[test]
fn insert_rejects_the_wrong_class_or_format_and_too_large() {
    let mut atlas = Atlas::new();
    assert!(
        matches!(atlas.insert(Class::Icon, &image(8, 8)), Err(Error::Class)),
        "Rgba8 into Icon"
    );
    assert!(
        matches!(atlas.insert(Class::Image, &icon(8)), Err(Error::Class)),
        "R8 into Image"
    );
    assert!(matches!(
        atlas.insert(Class::Glyph, &icon(8)),
        Err(Error::Class)
    ));
    assert!(matches!(
        atlas.insert(Class::External, &image(8, 8)),
        Err(Error::Class)
    ));
    // An image keeps a 16 px border each side, so the page takes PAGE - 32.
    let max = PAGE - 32;
    let big = max + 8;
    match atlas.insert(Class::Image, &image(big, big)) {
        Err(Error::TooLarge {
            width,
            height,
            max: m,
        }) if (width, height, m) == (big, big, max) => {}
        other => panic!("expected TooLarge, got {other:?}"),
    }
    assert!(
        atlas
            .insert(Class::Image, &image(big, big).fit(max))
            .is_ok()
    );
    assert_eq!(
        atlas.pages().count(),
        1,
        "the rejected inserts made no page"
    );
}

#[test]
fn a_zero_sized_bitmap_inserts_as_an_empty_tile() {
    let mut atlas = Atlas::new();
    let a = atlas.insert(Class::Image, &image(0, 0)).unwrap();
    let b = atlas.insert(Class::Image, &image(0, 8)).unwrap();
    assert_eq!(atlas.sprite(a).tile.bounds, Rect::ZERO);
    assert_eq!(atlas.sprite(a).size, geometry::Size::new(0.0, 0.0));
    assert_eq!(atlas.sprite(b).tile.bounds, Rect::ZERO);
    assert_eq!(atlas.sprite(b).size, geometry::Size::new(0.0, 8.0));
    assert_eq!(atlas.pages().count(), 0, "no page was opened");
    let mut cells = 0;
    atlas.drain_dirty(|_, _| cells += 1);
    assert_eq!(cells, 0, "nothing to drain for an empty tile");
}

#[test]
fn inserts_share_a_page_until_it_is_full_and_earlier_tiles_stay() {
    let mut atlas = Atlas::new();
    let a = atlas.insert(Class::Icon, &icon(64)).unwrap();
    let b = atlas.insert(Class::Icon, &icon(64)).unwrap();
    let (ta, tb) = (atlas.sprite(a).tile, atlas.sprite(b).tile);
    assert_eq!(ta.atlas, tb.atlas);
    assert_ne!(ta.bounds, tb.bounds);
    // A 64 icon takes a 96 slot: PAGE / 96 per shelf and as many shelves.
    let per_page = (PAGE / 96) * (PAGE / 96);
    let mut last = b;
    for _ in 0..per_page - 2 {
        last = atlas.insert(Class::Icon, &icon(64)).unwrap();
    }
    assert_eq!(
        atlas.sprite(last).tile.atlas,
        AtlasId(0),
        "the last of a page fits page 0"
    );
    let spill = atlas.insert(Class::Icon, &icon(64)).unwrap();
    assert_eq!(
        atlas.sprite(spill).tile.atlas,
        AtlasId(1),
        "the next opens a page"
    );
    assert_eq!(atlas.sprite(a).tile, ta, "nothing moved");
    assert_eq!(atlas.pages().count(), 2);
}

#[test]
fn drain_dirty_yields_a_new_page_whole_then_only_what_changed() {
    let mut atlas = Atlas::new();
    atlas.insert(Class::Image, &image(8, 8)).unwrap();
    let mut seen: Vec<(AtlasId, Cell)> = Vec::new();
    atlas.drain_dirty(|p, c| seen.push((p.id(), c)));
    assert_eq!(
        seen.len(),
        (CELLS * CELLS) as usize,
        "a new page is all cells"
    );
    assert!(seen.iter().all(|(id, _)| *id == AtlasId(0)));
    seen.clear();
    atlas.drain_dirty(|p, c| seen.push((p.id(), c)));
    assert!(seen.is_empty(), "a second drain has nothing");

    // A second image lands at (16 + 48, 16) on the same shelf... whatever the
    // exact spot, its cells are the only ones dirty now.
    let b = atlas.insert(Class::Image, &image(8, 8)).unwrap();
    let r = atlas.sprite(b).tile.bounds;
    atlas.drain_dirty(|p, c| seen.push((p.id(), c)));
    assert!(!seen.is_empty());
    for (_, c) in &seen {
        let side = CELL as f32;
        let cx = c.col as f32 * side;
        let cy = c.row as f32 * side;
        assert!(
            cx < r.right() && cx + side > r.x() && cy < r.bottom() && cy + side > r.y(),
            "cell {c:?} touches {r:?}"
        );
    }
}

#[test]
fn a_backend_reads_a_dirty_cell_at_every_level() {
    let mut atlas = Atlas::new();
    let a = atlas.insert(Class::Image, &image(64, 64)).unwrap();
    let r = atlas.sprite(a).tile.bounds; // (16,16) 64x64
    let mut cells = Vec::new();
    atlas.drain_dirty(|p, c| cells.push((p.id(), c)));
    assert_eq!(cells.len(), (CELLS * CELLS) as usize);
    let page = atlas.pages().next().unwrap();
    let mut out = Vec::new();
    for k in 0..page.levels() {
        page.cell(k, Cell { col: 0, row: 0 }, &mut out);
        let side = (CELL >> k) as usize;
        assert_eq!(out.len(), side * side * 4, "level {k}");
        // The image's centre in this cell: (48, 48) at level 0, halving per level.
        let cx = (r.x() as usize + 32) >> k;
        let cy = (r.y() as usize + 32) >> k;
        let i = (cy * side + cx) * 4;
        assert_eq!(
            &out[i..i + 4],
            &[64, 64, 64, 128],
            "level {k} holds the image's grey, premultiplied"
        );
    }
}

use std::os::fd::OwnedFd;
use std::sync::Arc;

fn a_fd() -> Arc<OwnedFd> {
    Arc::new(OwnedFd::from(std::fs::File::open("/dev/null").unwrap()))
}

#[test]
fn an_external_is_named_described_and_replaced() {
    let mut atlas = Atlas::new();
    atlas.insert(Class::Icon, &icon(8)).unwrap(); // page 0
    let first = a_fd();
    let feed = atlas.external(External {
        size: geometry::Size::new(640.0, 480.0),
        fourcc: 0x3231564e, // NV12
        modifier: 0,
        planes: vec![Plane {
            fd: first.clone(),
            offset: 0,
            stride: 640,
        }],
        generation: 0,
    });
    assert_eq!(feed, AtlasId(1), "ids count pages and externals together");
    assert_eq!(atlas.class(feed), Class::External);
    assert_eq!(
        atlas.external_tile(feed),
        AtlasTile {
            atlas: feed,
            bounds: Rect::new(0.0, 0.0, 640.0, 480.0)
        }
    );
    assert_eq!(atlas.pages().count(), 1, "an external is not a page");
    assert_eq!(Arc::strong_count(&first), 2);

    let second = a_fd();
    {
        let e = atlas.external_mut(feed);
        e.planes = vec![Plane {
            fd: second.clone(),
            offset: 0,
            stride: 640,
        }];
        e.generation += 1;
    }
    assert_eq!(Arc::strong_count(&first), 1, "the old buffer was released");
    let all: Vec<(AtlasId, u64)> = atlas
        .externals()
        .map(|(id, e)| (id, e.generation))
        .collect();
    assert_eq!(all, vec![(feed, 1)]);
    let mut cells = 0;
    atlas.drain_dirty(|_, _| cells += 1);
    assert_eq!(
        cells,
        (CELLS * CELLS) as usize,
        "only the icon page has cells; an external has none"
    );
}

const INTER: &[u8] = include_bytes!("fixtures/Inter-Regular.ttf");

#[test]
fn a_glyph_miss_dirties_only_the_cells_its_bitmap_crossed() {
    let mut atlas = Atlas::new();
    let inter = atlas.add_font(INTER).unwrap();
    let (_, a) = atlas.lookup(&[inter], 'a').unwrap();
    let g = atlas.glyph(inter, a, 14);
    let mut cells = Vec::new();
    atlas.drain_dirty(|_, c| cells.push(c));
    assert_eq!(
        cells.len(),
        (CELLS * CELLS) as usize,
        "the glyph page is new"
    );
    cells.clear();
    let (_, b) = atlas.lookup(&[inter], 'b').unwrap();
    let gb = atlas.glyph(inter, b, 14);
    atlas.drain_dirty(|_, c| cells.push(c));
    assert!(
        !cells.is_empty() && cells.len() <= 2,
        "one or two cells: {cells:?}"
    );
    assert_eq!(gb.tile.atlas, g.tile.atlas);
    assert_eq!(atlas.class(g.tile.atlas), Class::Glyph);
    assert_eq!(
        atlas.pages().next().unwrap().levels(),
        1,
        "glyph pages have no mips"
    );
}

use std::cell::RefCell;

use app::prelude::*;
use paint::prelude::*;

thread_local! {
    static UPLOADED: RefCell<Vec<(AtlasId, Cell)>> = const { RefCell::new(Vec::new()) };
    static RUNS: RefCell<u32> = const { RefCell::new(0) };
}

/// A backend's upload system: runs when the core reports that a tick took
/// `resource_mut::<Atlas>()`. Reads through `resource`, so the drain is
/// not a write and the signal does not re-arm.
fn upload(app: &mut App, _: &OnChanged<Atlas>) {
    RUNS.with(|r| *r.borrow_mut() += 1);
    let atlas = app.resource::<Atlas>();
    atlas.drain_dirty(|p, c| UPLOADED.with(|u| u.borrow_mut().push((p.id(), c))));
}

/// A widget that asks for a glyph while it is built.
struct Letter(char);
impl Build for Letter {
    type Widget = Letter;
}
impl Widget for Letter {
    type Builder = Letter;
    fn build(b: Letter, _: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let mut atlas = s.resource_mut::<Atlas>();
        let (font, id) = atlas.lookup(&[FontId(0)], b.0).unwrap();
        atlas.glyph(font, id, 14);
        b
    }
}

#[test]
fn as_a_resource_the_core_signals_a_backend_that_drains_what_a_widget_made() {
    let mut app = App::new();
    let mut atlas = Atlas::new();
    atlas.add_font(INTER).unwrap();
    app.insert_resource(atlas);
    app.system(upload);

    // The insert counts as a write: the first tick fires once and uploads
    // nothing, since no page exists yet.
    app.tick();
    assert_eq!(RUNS.with(|r| *r.borrow()), 1, "insert_resource is a write");
    assert!(UPLOADED.with(|u| u.borrow().is_empty()));

    let root = app.root();
    app.spawn(root, Letter('a'));
    app.tick();
    assert_eq!(
        RUNS.with(|r| *r.borrow()),
        2,
        "the build's resource_mut fired the signal"
    );
    let first = UPLOADED.with(|u| std::mem::take(&mut *u.borrow_mut()));
    assert_eq!(
        first.len(),
        (CELLS * CELLS) as usize,
        "the glyph page, whole"
    );

    app.tick();
    assert_eq!(
        RUNS.with(|r| *r.borrow()),
        2,
        "a tick that asked nothing does not run the system"
    );
    assert!(UPLOADED.with(|u| u.borrow().is_empty()));

    app.spawn(root, Letter('b'));
    app.tick();
    assert_eq!(RUNS.with(|r| *r.borrow()), 3);
    let second = UPLOADED.with(|u| std::mem::take(&mut *u.borrow_mut()));
    assert!(
        !second.is_empty() && second.len() <= 2,
        "only the cells 'b' crossed"
    );
}

#[test]
fn paint_builds_sprites_from_atlas_tiles() {
    let mut atlas = Atlas::new();
    let inter = atlas.add_font(INTER).unwrap();
    let (_, a) = atlas.lookup(&[inter], 'a').unwrap();
    let g = atlas.glyph(inter, a, 14);
    let run = Paint::Monochrome(vec![MonochromeSprite::new(
        g.tile,
        geometry::Point::new(g.left, 14.0 - g.top),
        geometry::Size::new(g.tile.bounds.width(), g.tile.bounds.height()),
        Color::WHITE,
    )]);
    assert!(!run.is_invisible());

    let art = atlas.insert(Class::Image, &image(32, 32)).unwrap();
    let pic = Paint::Polychrome(PolychromeSprite::new(atlas.sprite(art).tile));
    assert!(!pic.is_invisible());
    assert_eq!(atlas.class(atlas.sprite(art).tile.atlas), Class::Image);
}

#[test]
fn error_displays_its_cause() {
    let text = Error::TooLarge {
        width: 2000,
        height: 10,
        max: 992,
    }
    .to_string();
    assert!(text.contains("2000") && text.contains("992"), "{text}");
    let _: Box<dyn std::error::Error> = Error::Class.into();
}

#[test]
fn page_finds_a_page_by_id() {
    let mut atlas = Atlas::new();
    let i = atlas.insert(Class::Icon, &icon(8)).unwrap();
    atlas.insert(Class::Image, &image(8, 8)).unwrap();
    let id = atlas.sprite(i).tile.atlas;
    assert_eq!(atlas.page(id).unwrap().class(), Class::Icon);
    let feed = atlas.external(External {
        size: geometry::Size::new(64.0, 64.0),
        fourcc: 0x3231564e,
        modifier: 0,
        planes: vec![],
        generation: 0,
    });
    assert!(atlas.page(feed).is_none(), "an external is not a page");
    assert!(atlas.page(AtlasId(999)).is_none(), "never minted");
}

#[test]
fn a_glyph_larger_than_a_page_panics_with_its_size() {
    let mut atlas = Atlas::new();
    let inter = atlas.add_font(INTER).unwrap();
    let (_, w) = atlas.lookup(&[inter], 'W').unwrap();
    let px = (2 * PAGE) as u16;
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        atlas.glyph(inter, w, px);
    }))
    .expect_err("a glyph larger than a page panics");
    let text = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    assert!(
        text.contains(&format!("at {px} px"))
            && text.contains(&format!("larger than a {PAGE} page")),
        "{text}"
    );
}

#[test]
fn image_pages_store_premultiplied() {
    let mut atlas = Atlas::new();
    let bitmap = Bitmap {
        width: 1,
        height: 1,
        format: Format::Rgba8,
        pixels: vec![200, 100, 50, 128],
    };
    let id = atlas.insert(Class::Image, &bitmap).unwrap();
    let origin = |atlas: &Atlas| {
        let tile = atlas.sprite(id).tile;
        let page = atlas.page(tile.atlas).unwrap();
        let i = (tile.bounds.y() as usize * page.size() as usize + tile.bounds.x() as usize) * 4;
        let px: [u8; 4] = page.level(0)[i..i + 4].try_into().unwrap();
        px
    };
    let want = [100u8, 50, 25, 128];
    let near = |p: [u8; 4]| p.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 1);
    let px = origin(&atlas);
    assert!(near(px), "premultiplied: {px:?}");
    // the stored source stays straight: a rescale re-premultiplies once
    assert!(atlas.set_scale(2.0));
    let px = origin(&atlas);
    assert!(near(px), "not double-premultiplied after set_scale: {px:?}");
}
