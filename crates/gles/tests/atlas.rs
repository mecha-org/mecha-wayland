mod common;

use std::os::fd::OwnedFd;
use std::sync::Arc;

use atlas::{Atlas, AtlasId, AtlasTile, Bitmap, Class, External, Format, Plane};
use common::{device, near, pixel, png, rgb};
use geometry::{Color, Corners, Insets, Rect, Size};
use gles::{Budget, Device};
use render::{Command, Pass, Queue};

const INTER: &[u8] = include_bytes!("../../atlas/tests/fixtures/Inter-Regular.ttf");

fn queue(w: f32, h: f32, clear: Color) -> Queue {
    Queue {
        size: Size::new(w, h),
        scale: 1.0,
        clear,
        depth: 2.0,
        scissor: vec![Rect::new(0.0, 0.0, w, h)],
        opaque: Pass::default(),
        translucent: Pass::default(),
    }
}

fn sprite(rect: Rect, tile: AtlasTile, color: Color, background: Option<Color>) -> Command {
    Command {
        rect,
        z: 0.0,
        color,
        border_color: color,
        background: background.unwrap_or(Color::TRANSPARENT),
        radii: Corners::all(0.0),
        border: Insets::all(0.0),
        tile,
        flags: Command::pack(Command::SPRITE, background.is_some(), false, 1.0),
        clip: rect,
    }
}

fn image(rect: Rect, tile: AtlasTile, grayscale: bool, opacity: f32) -> Command {
    Command {
        rect,
        z: 0.0,
        color: Color::WHITE,
        border_color: Color::WHITE,
        background: Color::TRANSPARENT,
        radii: Corners::all(0.0),
        border: Insets::all(0.0),
        tile,
        flags: Command::pack(Command::IMAGE, false, grayscale, opacity),
        clip: rect,
    }
}

/// A 32 by 32 opaque image: left half red, right half green.
fn halves() -> Bitmap {
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for _y in 0..32 {
        for x in 0..32 {
            let (r, g) = if x < 16 { (255, 0) } else { (0, 255) };
            pixels.extend_from_slice(&[r, g, 0, 255]);
        }
    }
    Bitmap {
        width: 32,
        height: 32,
        format: Format::Rgba8,
        pixels,
    }
}

fn draw_one(d: &mut Device, w: u32, h: u32, q: &Queue, name: &str) -> Vec<u8> {
    let t = d.target(w, h, &[]);
    d.draw(&t, q);
    let px = d.read(&t);
    png(name, w, h, &px);
    d.destroy(t);
    px
}

#[test]
fn a_glyph_sprite_renders_its_coverage_tinted() {
    let Some((_g, mut d)) = device() else { return };
    let mut atlas = Atlas::new();
    let inter = atlas.add_font(INTER).unwrap();
    let (font, id) = atlas.lookup(&[inter], 'a').unwrap();
    let g = atlas.glyph(font, id, 40);
    d.upload(&atlas);
    let (w, h) = (g.tile.bounds.width(), g.tile.bounds.height());
    assert!(w > 10.0 && h > 10.0, "a 40 px 'a' has ink: {w}x{h}");

    // Blended: white tint over black, the tile at (4, 4).
    let mut q = queue(64.0, 64.0, Color::BLACK);
    q.translucent = Pass {
        scissor: q.scissor.clone(),
        commands: vec![sprite(
            Rect::new(4.0, 4.0, w, h),
            g.tile,
            Color::WHITE,
            None,
        )],
    };
    let px = draw_one(
        &mut d,
        64,
        64,
        &q,
        "a_glyph_sprite_renders_its_coverage_tinted",
    );
    let mut brightest = 0u8;
    for y in 4..(4 + h as u32) {
        for x in 4..(4 + w as u32) {
            brightest = brightest.max(pixel(&px, 64, x, y)[0]);
        }
    }
    assert!(
        brightest > 200,
        "some pixel of the glyph is near white: {brightest}"
    );
    assert_eq!(
        pixel(&px, 64, 60, 60),
        rgb(0.0, 0.0, 0.0),
        "outside the tile stays black"
    );
    assert_eq!(
        pixel(&px, 64, 4, 4),
        rgb(0.0, 0.0, 0.0),
        "the tile's corner is empty"
    );

    // Opaque: black tint composited onto white.
    let mut q = queue(64.0, 64.0, Color::WHITE);
    q.opaque = Pass {
        scissor: q.scissor.clone(),
        commands: vec![sprite(
            Rect::new(4.0, 4.0, w, h),
            g.tile,
            Color::BLACK,
            Some(Color::WHITE),
        )],
    };
    let px = draw_one(
        &mut d,
        64,
        64,
        &q,
        "a_glyph_sprite_renders_its_coverage_tinted.opaque",
    );
    let mut darkest = 255u8;
    for y in 4..(4 + h as u32) {
        for x in 4..(4 + w as u32) {
            darkest = darkest.min(pixel(&px, 64, x, y)[0]);
        }
    }
    assert!(
        darkest < 60,
        "some pixel of the glyph is near black: {darkest}"
    );
    assert_eq!(pixel(&px, 64, 60, 60), rgb(1.0, 1.0, 1.0));
}

#[test]
fn an_image_renders_at_its_size_and_at_half_size_through_its_mips() {
    let Some((_g, mut d)) = device() else { return };
    let mut atlas = Atlas::new();
    let id = atlas.insert(Class::Image, &halves()).unwrap();
    let tile = atlas.sprite(id).tile;
    d.upload(&atlas);

    let mut q = queue(48.0, 48.0, Color::BLACK);
    q.translucent = Pass {
        scissor: q.scissor.clone(),
        commands: vec![
            image(Rect::new(0.0, 0.0, 32.0, 32.0), tile, false, 1.0),
            image(Rect::new(32.0, 0.0, 16.0, 16.0), tile, false, 1.0),
            image(Rect::new(0.0, 32.0, 16.0, 16.0), tile, true, 1.0),
            image(Rect::new(16.0, 32.0, 16.0, 16.0), tile, false, 0.5),
        ],
    };
    let px = draw_one(
        &mut d,
        48,
        48,
        &q,
        "an_image_renders_at_its_size_and_at_half_size_through_its_mips",
    );
    assert!(
        near(pixel(&px, 48, 4, 16), rgb(1.0, 0.0, 0.0), 2),
        "full size, left half red"
    );
    assert!(
        near(pixel(&px, 48, 27, 16), rgb(0.0, 1.0, 0.0), 2),
        "full size, right half green"
    );
    assert!(
        near(pixel(&px, 48, 34, 8), rgb(1.0, 0.0, 0.0), 8),
        "half size, left half red"
    );
    assert!(
        near(pixel(&px, 48, 45, 8), rgb(0.0, 1.0, 0.0), 8),
        "half size, right half green"
    );
    let g = pixel(&px, 48, 2, 40);
    assert!(g[0] == g[1] && g[1] == g[2], "grayscale is grey: {g:?}");
    assert!(g[0] > 30 && g[0] < 90, "luma of pure red: {g:?}");
    assert!(
        near(pixel(&px, 48, 18, 40), rgb(0.5, 0.0, 0.0), 3),
        "half opacity over black"
    );
}

#[test]
fn dirty_cells_go_up_as_row_runs() {
    let Some((_g, mut d)) = device() else { return };
    let mut atlas = Atlas::new();
    let strip = Bitmap {
        width: 200,
        height: 8,
        format: Format::Rgba8,
        pixels: vec![200; 200 * 8 * 4],
    };
    atlas.insert(Class::Image, &strip).unwrap();
    d.upload(&atlas);
    // A new page: every cell dirty, 16 rows, one run a row, five levels.
    assert_eq!(d.upload_calls(), 16 * 5);
    // A second strip lands beside the first on the same shelf: one run of
    // cells in row 0, five levels.
    atlas.insert(Class::Image, &strip).unwrap();
    d.upload(&atlas);
    assert_eq!(d.upload_calls(), 16 * 5 + 5);
    // Nothing dirty: nothing uploaded.
    d.upload(&atlas);
    assert_eq!(d.upload_calls(), 16 * 5 + 5);
}

#[test]
fn a_page_past_the_budget_panics_with_the_class_named() {
    let Some((_g, _d)) = device() else { return };
    drop(_d);
    let mut d = Device::try_open(Budget {
        mono_pages: 1,
        color_pages: 1,
    })
    .unwrap();
    let mut atlas = Atlas::new();
    let big = Bitmap {
        width: 600,
        height: 600,
        format: Format::Rgba8,
        pixels: vec![10; 600 * 600 * 4],
    };
    atlas.insert(Class::Image, &big).unwrap();
    atlas.insert(Class::Image, &big).unwrap();
    assert!(
        atlas.pages().count() >= 2,
        "two 600 px images need two pages"
    );
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.upload(&atlas)));
    let msg = r.unwrap_err();
    let msg = msg
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| msg.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    assert!(msg.contains("color atlas budget of 1"), "{msg}");
    assert!(msg.contains("glyph,"), "the page counts by class: {msg}");
}

#[test]
fn a_page_id_is_looked_up_even_when_pages_of_both_kinds_interleave() {
    let Some((_g, mut d)) = device() else { return };
    let mut atlas = Atlas::new();
    let inter = atlas.add_font(INTER).unwrap();
    atlas.warm(inter, 14, 'a'..='c'); // mono page, id 0
    let img = atlas.insert(Class::Image, &halves()).unwrap(); // color page, id 1
    d.upload(&atlas);
    let tile = atlas.sprite(img).tile;
    assert_eq!(tile.atlas, AtlasId(1));
    let mut q = queue(32.0, 32.0, Color::BLACK);
    q.translucent = Pass {
        scissor: q.scissor.clone(),
        commands: vec![image(Rect::new(0.0, 0.0, 32.0, 32.0), tile, false, 1.0)],
    };
    let px = draw_one(
        &mut d,
        32,
        32,
        &q,
        "a_page_id_is_looked_up_even_when_pages_of_both_kinds_interleave",
    );
    assert!(near(pixel(&px, 32, 4, 16), rgb(1.0, 0.0, 0.0), 2));
    assert!(near(pixel(&px, 32, 27, 16), rgb(0.0, 1.0, 0.0), 2));
}

#[test]
fn a_transparent_edge_has_no_dark_fringe() {
    let Some((_g, mut d)) = device() else { return };
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for _y in 0..32 {
        for x in 0..32 {
            let p = if x < 16 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 0]
            };
            pixels.extend_from_slice(&p);
        }
    }
    let bitmap = Bitmap {
        width: 32,
        height: 32,
        format: Format::Rgba8,
        pixels,
    };
    let mut atlas = Atlas::new();
    let id = atlas.insert(Class::Image, &bitmap).unwrap();
    let tile = atlas.sprite(id).tile;
    d.upload(&atlas);

    let mut q = queue(48.0, 48.0, Color::TRANSPARENT);
    q.translucent = Pass {
        scissor: q.scissor.clone(),
        commands: vec![image(Rect::new(0.0, 0.0, 48.0, 48.0), tile, false, 1.0)],
    };
    let px = draw_one(&mut d, 48, 48, &q, "a_transparent_edge_has_no_dark_fringe");
    // The target is XRGB8888, so alpha reads back as 255 and the pixel is
    // the sprite composited over the black clear: red == coverage, which the
    // premultiplied bilinear ramp makes linear between the two texel centres
    // (device x 22.75 and 24.25 at 1.5x). Straight filtering gave coverage squared.
    let mut seen = 0;
    for x in 22..26u32 {
        let u = (x as f32 + 0.5) / 1.5;
        let cov = (16.5 - u).clamp(0.0, 1.0);
        let want = (cov * 255.0).round() as i32;
        let p = pixel(&px, 48, x, 24);
        if want > 8 && want < 247 {
            seen += 1;
        }
        assert!(
            (p[0] as i32 - want).abs() <= 6 && p[1] == 0 && p[2] == 0,
            "x {x}: red {} should be the coverage {want}: {p:?}",
            p[0]
        );
    }
    assert!(seen >= 2, "the ramp had fewer than two partial pixels");
}

const SQUARE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="black"/></svg>"#;

#[test]
fn many_rescales_never_exhaust_the_table() {
    let Some((_g, mut d)) = device() else { return };
    let mut atlas = Atlas::new();
    let inter = atlas.add_font(INTER).unwrap();
    let (font, id) = atlas.lookup(&[inter], 'a').unwrap();
    atlas.glyph(font, id, 14);
    atlas.insert_svg(SQUARE, 16).unwrap();
    d.upload(&atlas);
    for _ in 0..100 {
        atlas.set_scale(1.0);
        atlas.set_scale(2.0);
        d.upload(&atlas);
    }
}

#[test]
#[should_panic(expected = "(dmabuf import) is not implemented")]
fn an_external_hits_todo() {
    let Some((_g, mut d)) = device() else {
        // No device: satisfy should_panic.
        panic!("(dmabuf import) is not implemented (skipped)");
    };
    let mut atlas = Atlas::new();
    let fd = Arc::new(OwnedFd::from(std::fs::File::open("/dev/null").unwrap()));
    atlas.external(External {
        size: Size::new(16.0, 16.0),
        fourcc: 0x3231564e,
        modifier: 0,
        planes: vec![Plane {
            fd,
            offset: 0,
            stride: 16,
        }],
        generation: 0,
    });
    d.upload(&atlas);
}

#[test]
fn an_icon_sprite_draws_through_its_mips() {
    let Some((_g, mut d)) = device() else { return };
    let mut atlas = Atlas::new();
    let id = atlas.insert_svg(SQUARE, 32).unwrap();
    let tile = atlas.sprite(id).tile;
    d.upload(&atlas);
    let red = Color::rgb(1.0, 0.0, 0.0);
    let mut q = queue(48.0, 32.0, Color::BLACK);
    q.translucent = Pass {
        scissor: q.scissor.clone(),
        commands: vec![
            sprite(Rect::new(0.0, 0.0, 32.0, 32.0), tile, red, None),
            sprite(Rect::new(32.0, 0.0, 16.0, 16.0), tile, red, None),
        ],
    };
    let px = draw_one(&mut d, 48, 32, &q, "an_icon_sprite_draws_through_its_mips");
    assert!(
        near(pixel(&px, 48, 16, 16), rgb(1.0, 0.0, 0.0), 3),
        "full size"
    );
    assert!(
        near(pixel(&px, 48, 40, 8), rgb(1.0, 0.0, 0.0), 3),
        "half size"
    );
}

#[test]
fn at_scale_two_a_glyph_lands_one_to_one() {
    let Some((_g, mut d)) = device() else { return };
    let mut atlas = Atlas::new();
    let inter = atlas.add_font(INTER).unwrap();
    atlas.set_scale(2.0);
    let (font, id) = atlas.lookup(&[inter], 'H').unwrap();
    let g = atlas.glyph(font, id, 14);
    d.upload(&atlas);
    // As render would: the logical rect scaled by 2.
    let (w, h) = ((g.size.width * 2.0).round(), (g.size.height * 2.0).round());
    assert_eq!((w as u32, h as u32), (g.physical_width, g.physical_height));
    let (w, h) = (w as u32, h as u32);
    let mut q = queue(32.0, 32.0, Color::BLACK);
    q.scale = 2.0;
    q.translucent = Pass {
        scissor: q.scissor.clone(),
        commands: vec![sprite(
            Rect::new(4.0, 4.0, w as f32, h as f32),
            g.tile,
            Color::WHITE,
            None,
        )],
    };
    let px = draw_one(&mut d, 32, 32, &q, "at_scale_two_a_glyph_lands_one_to_one");
    let page = atlas.page(g.tile.atlas).unwrap();
    let (bx, by) = (g.tile.bounds.origin.x as u32, g.tile.bounds.origin.y as u32);
    let mut ink = 0;
    for j in 0..h {
        for i in 0..w {
            let cov = page.level(0)[((by + j) * atlas::PAGE + bx + i) as usize];
            ink += (cov > 128) as u32;
            let p = pixel(&px, 32, 4 + i, 4 + j);
            assert!(
                near(p, [cov, cov, cov, 255], 2),
                "({i}, {j}): drawn {p:?} vs coverage {cov}"
            );
        }
    }
    assert!(ink > 10, "the H has ink");
}

/// A 600 by 600 opaque image of one colour: too big to share a page.
fn solid(r: u8, g: u8) -> Bitmap {
    Bitmap {
        width: 600,
        height: 600,
        format: Format::Rgba8,
        pixels: [r, g, 0, 255].repeat(600 * 600),
    }
}

#[test]
fn a_reused_id_that_changes_class_draws_from_the_right_array() {
    let Some((_g, mut d)) = device() else { return };
    let mut atlas = Atlas::new();
    let inter = atlas.add_font(INTER).unwrap();
    let (font, id) = atlas.lookup(&[inter], 'H').unwrap();
    atlas.glyph(font, id, 14); // id 0: mono layer 0
    let red = atlas.insert(Class::Image, &solid(255, 0)).unwrap(); // id 1: color layer 0
    let green = atlas.insert(Class::Image, &solid(0, 255)).unwrap(); // id 2: color layer 1
    d.upload(&atlas);
    // A rebuild packs sprites before glyphs: red takes id 0 (color layer 0),
    // green id 1 (color layer 1), the glyph id 2 (mono). Ids 0 and 1 were
    // mono and color layer 0 before, so stale entries put green on layer 0.
    atlas.set_scale(2.0);
    let g = atlas.glyph(font, id, 14);
    let (rt, gt) = (atlas.sprite(red).tile, atlas.sprite(green).tile);
    assert_eq!(
        (rt.atlas, gt.atlas, g.tile.atlas),
        (AtlasId(0), AtlasId(1), AtlasId(2))
    );
    d.upload(&atlas);

    let (w, h) = (g.physical_width as f32, g.physical_height as f32);
    let mut q = queue(64.0, 32.0, Color::BLACK);
    q.translucent = Pass {
        scissor: q.scissor.clone(),
        commands: vec![
            image(Rect::new(0.0, 0.0, 16.0, 16.0), rt, false, 1.0),
            image(Rect::new(16.0, 0.0, 16.0, 16.0), gt, false, 1.0),
            sprite(Rect::new(40.0, 4.0, w, h), g.tile, Color::WHITE, None),
        ],
    };
    let px = draw_one(
        &mut d,
        64,
        32,
        &q,
        "a_reused_id_that_changes_class_draws_from_the_right_array",
    );
    assert!(
        near(pixel(&px, 64, 8, 8), rgb(1.0, 0.0, 0.0), 2),
        "red image"
    );
    assert!(
        near(pixel(&px, 64, 24, 8), rgb(0.0, 1.0, 0.0), 2),
        "green image"
    );
    let mut brightest = 0u8;
    for y in 4..4 + h as u32 {
        for x in 40..40 + w as u32 {
            brightest = brightest.max(pixel(&px, 64, x, y)[0]);
        }
    }
    assert!(brightest > 200, "the glyph has ink: {brightest}");
}
