mod common;

use atlas::{AtlasId, AtlasTile};
use common::{device, near, pixel, png, rgb};
use geometry::{Color, Corners, Insets, Rect, Size};
use gles::XRGB8888;
use render::{Command, Pass, Queue};

fn queue(w: f32, h: f32, clear: Color, scissor: Vec<Rect>) -> Queue {
    Queue {
        size: Size::new(w, h),
        scale: 1.0,
        clear,
        depth: 2.0,
        scissor,
        opaque: Pass::default(),
        translucent: Pass::default(),
    }
}

#[test]
fn a_linear_target_has_one_plane_and_the_xrgb_fourcc() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(64, 48, &[]);
    assert_eq!((t.width(), t.height()), (64, 48));
    assert_eq!(t.fourcc(), XRGB8888);
    assert_eq!(t.modifier(), 0, "an empty list is linear");
    let planes: Vec<_> = t.planes().collect();
    assert_eq!(planes.len(), 1);
    assert!(planes[0].stride >= 64 * 4);
    assert_eq!(planes[0].offset, 0);
    d.destroy(t);
}

#[test]
fn a_target_made_with_the_linear_modifier_reports_it() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(16, 16, &[0]);
    assert_eq!(t.modifier(), 0, "DRM_FORMAT_MOD_LINEAR");
    d.destroy(t);
}

#[test]
fn a_clear_covers_the_whole_scissor() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(64, 48, &[]);
    let red = Color::rgb(1.0, 0.0, 0.0);
    d.draw(
        &t,
        &queue(64.0, 48.0, red, vec![Rect::new(0.0, 0.0, 64.0, 48.0)]),
    );
    let px = d.read(&t);
    png("a_clear_covers_the_whole_scissor", 64, 48, &px);
    assert_eq!(px.len(), 64 * 48 * 4);
    for y in 0..48 {
        for x in 0..64 {
            assert_eq!(pixel(&px, 64, x, y), rgb(1.0, 0.0, 0.0), "at {x},{y}");
        }
    }
    d.destroy(t);
}

#[test]
fn a_clear_under_two_rects_leaves_the_rest_untouched() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(64, 48, &[]);
    let red = Color::rgb(1.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);
    d.draw(
        &t,
        &queue(64.0, 48.0, red, vec![Rect::new(0.0, 0.0, 64.0, 48.0)]),
    );
    d.draw(
        &t,
        &queue(
            64.0,
            48.0,
            green,
            vec![
                Rect::new(0.0, 0.0, 16.0, 16.0),
                Rect::new(40.0, 30.0, 24.0, 18.0),
            ],
        ),
    );
    let px = d.read(&t);
    png(
        "a_clear_under_two_rects_leaves_the_rest_untouched",
        64,
        48,
        &px,
    );
    assert_eq!(pixel(&px, 64, 0, 0), rgb(0.0, 1.0, 0.0));
    assert_eq!(pixel(&px, 64, 15, 15), rgb(0.0, 1.0, 0.0));
    assert_eq!(pixel(&px, 64, 16, 16), rgb(1.0, 0.0, 0.0));
    assert_eq!(pixel(&px, 64, 40, 30), rgb(0.0, 1.0, 0.0));
    assert_eq!(pixel(&px, 64, 63, 47), rgb(0.0, 1.0, 0.0));
    assert_eq!(pixel(&px, 64, 39, 29), rgb(1.0, 0.0, 0.0));
    assert!(near(pixel(&px, 64, 32, 24), rgb(1.0, 0.0, 0.0), 0));
    d.destroy(t);
}

#[test]
fn an_empty_scissor_draws_nothing() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(8, 8, &[]);
    d.draw(
        &t,
        &queue(
            8.0,
            8.0,
            Color::rgb(0.0, 0.0, 1.0),
            vec![Rect::new(0.0, 0.0, 8.0, 8.0)],
        ),
    );
    d.draw(&t, &queue(8.0, 8.0, Color::rgb(1.0, 1.0, 1.0), vec![]));
    let px = d.read(&t);
    png("an_empty_scissor_draws_nothing", 8, 8, &px);
    assert_eq!(pixel(&px, 8, 4, 4), rgb(0.0, 0.0, 1.0));
    d.destroy(t);
}

#[test]
fn a_fractional_scissor_is_rounded_outward() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(16, 16, &[]);
    d.draw(
        &t,
        &queue(
            16.0,
            16.0,
            Color::BLACK,
            vec![Rect::new(0.0, 0.0, 16.0, 16.0)],
        ),
    );
    d.draw(
        &t,
        &queue(
            16.0,
            16.0,
            Color::WHITE,
            vec![Rect::new(4.5, 4.5, 2.0, 2.0)],
        ),
    );
    let px = d.read(&t);
    png("a_fractional_scissor_is_rounded_outward", 16, 16, &px);
    assert_eq!(pixel(&px, 16, 4, 4), rgb(1.0, 1.0, 1.0), "floor of 4.5");
    assert_eq!(
        pixel(&px, 16, 6, 6),
        rgb(1.0, 1.0, 1.0),
        "ceil of 6.5 covers pixel 6"
    );
    assert_eq!(pixel(&px, 16, 7, 7), rgb(0.0, 0.0, 0.0));
    assert_eq!(pixel(&px, 16, 3, 3), rgb(0.0, 0.0, 0.0));
    d.destroy(t);
}

const NO_TILE: AtlasTile = AtlasTile {
    atlas: AtlasId(0),
    bounds: Rect::ZERO,
};

/// A quad command. `background: Some(c)` puts it in the opaque list
/// composited onto `c`; `None` blends it.
fn quad(rect: Rect, z: f32, color: Color, background: Option<Color>) -> Command {
    Command {
        rect,
        z,
        color,
        border_color: color,
        background: background.unwrap_or(Color::TRANSPARENT),
        radii: Corners::all(0.0),
        border: Insets::all(0.0),
        tile: NO_TILE,
        flags: Command::pack(Command::QUAD, background.is_some(), false, 1.0),
        clip: rect,
    }
}

fn whole(w: f32, h: f32) -> Vec<Rect> {
    vec![Rect::new(0.0, 0.0, w, h)]
}

fn opaque(mut q: Queue, cmds: Vec<Command>) -> Queue {
    q.opaque = Pass {
        scissor: q.scissor.clone(),
        commands: cmds,
    };
    q
}

fn translucent(mut q: Queue, cmds: Vec<Command>) -> Queue {
    q.translucent = Pass {
        scissor: q.scissor.clone(),
        commands: cmds,
    };
    q
}

/// The shader's rounded-box distance, on the CPU, for a reference.
fn sd_box(px: f32, py: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = px.abs() - hx + r;
    let qy = py.abs() - hy + r;
    let (mx, my) = (qx.max(0.0), qy.max(0.0));
    (mx * mx + my * my).sqrt() + qx.max(qy).min(0.0) - r
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[test]
fn an_opaque_fill_is_exact_inside_and_absent_outside() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(32, 32, &[]);
    let q = queue(32.0, 32.0, Color::BLACK, whole(32.0, 32.0));
    let fill = Color::rgb(0.2, 0.6, 0.9);
    let q = opaque(
        q,
        vec![quad(
            Rect::new(4.0, 6.0, 10.0, 8.0),
            0.0,
            fill,
            Some(Color::BLACK),
        )],
    );
    d.draw(&t, &q);
    let px = d.read(&t);
    png(
        "an_opaque_fill_is_exact_inside_and_absent_outside",
        32,
        32,
        &px,
    );
    for y in 0..32 {
        for x in 0..32 {
            let inside = (4..14).contains(&x) && (6..14).contains(&y);
            let want = if inside {
                rgb(0.2, 0.6, 0.9)
            } else {
                rgb(0.0, 0.0, 0.0)
            };
            assert!(
                near(pixel(&px, 32, x, y), want, 1),
                "at {x},{y}: {:?}",
                pixel(&px, 32, x, y)
            );
        }
    }
    d.destroy(t);
}

#[test]
fn rounded_corners_match_the_cpu_reference() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(40, 40, &[]);
    let q = queue(40.0, 40.0, Color::BLACK, whole(40.0, 40.0));
    let mut c = quad(
        Rect::new(4.0, 4.0, 32.0, 24.0),
        0.0,
        Color::WHITE,
        Some(Color::BLACK),
    );
    c.radii = Corners::all(8.0);
    let q = opaque(q, vec![c]);
    d.draw(&t, &q);
    let px = d.read(&t);
    png("rounded_corners_match_the_cpu_reference", 40, 40, &px);
    // The corner pixel of the rect is empty, the centre is full.
    assert!(near(pixel(&px, 40, 4, 4), rgb(0.0, 0.0, 0.0), 2));
    assert_eq!(pixel(&px, 40, 20, 16), rgb(1.0, 1.0, 1.0));
    // Along the diagonal of the top-left corner, coverage follows the SDF.
    for i in 0..8u32 {
        let (x, y) = (4 + i, 4 + i);
        let (lx, ly) = (x as f32 + 0.5 - 20.0, y as f32 + 0.5 - 16.0);
        let dist = sd_box(lx, ly, 16.0, 12.0, 8.0);
        let cov = 1.0 - smoothstep(-0.5, 0.5, dist);
        let want = (cov * 255.0).round() as u8;
        let got = pixel(&px, 40, x, y)[0];
        assert!(
            got.abs_diff(want) <= 2,
            "at {x},{y}: got {got}, want {want}"
        );
    }
    d.destroy(t);
}

#[test]
fn clip_crops_a_quad() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(40, 40, &[]);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let mut c = quad(Rect::new(0.0, 0.0, 40.0, 40.0), 0.0, Color::WHITE, None);
    c.radii = Corners::all(8.0);
    let q = queue(40.0, 40.0, blue, whole(40.0, 40.0));
    d.draw(&t, &translucent(q.clone(), vec![c]));
    let whole_quad = d.read(&t);
    let clipped = Command {
        clip: Rect::new(0.0, 0.0, 20.0, 40.0),
        ..c
    };
    d.draw(&t, &translucent(q, vec![clipped]));
    let px = d.read(&t);
    png("clip_crops_a_quad", 40, 40, &px);
    assert_eq!(pixel(&px, 40, 10, 20), rgb(1.0, 1.0, 1.0), "the quad");
    for y in 0..40 {
        for x in 0..40 {
            let got = pixel(&px, 40, x, y);
            if x >= 20 {
                assert_eq!(got, rgb(0.0, 0.0, 1.0), "cut away at {x},{y}");
            } else {
                // The kept half, its rounded corners included, is the
                // unclipped draw's.
                let want = pixel(&whole_quad, 40, x, y);
                assert!(near(got, want, 1), "at {x},{y}: {got:?}, want {want:?}");
            }
        }
    }
    assert_eq!(
        pixel(&px, 40, 0, 0),
        rgb(0.0, 0.0, 1.0),
        "the top-left corner is still rounded"
    );
    d.destroy(t);
}

#[test]
fn borders_with_unequal_insets_sit_where_each_side_says() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(40, 40, &[]);
    let q = queue(40.0, 40.0, Color::BLACK, whole(40.0, 40.0));
    let mut c = quad(
        Rect::new(4.0, 4.0, 32.0, 32.0),
        0.0,
        Color::rgb(0.0, 0.0, 1.0),
        Some(Color::BLACK),
    );
    c.border_color = Color::rgb(1.0, 0.0, 0.0);
    c.border = Insets::new(2.0, 6.0, 4.0, 1.0); // top, right, bottom, left
    let q = opaque(q, vec![c]);
    d.draw(&t, &q);
    let px = d.read(&t);
    png(
        "borders_with_unequal_insets_sit_where_each_side_says",
        40,
        40,
        &px,
    );
    let red = rgb(1.0, 0.0, 0.0);
    let blue = rgb(0.0, 0.0, 1.0);
    assert!(near(pixel(&px, 40, 20, 4), red, 1), "top edge");
    assert!(near(pixel(&px, 40, 20, 5), red, 1), "top, 2 px deep");
    assert!(near(pixel(&px, 40, 20, 6), blue, 1), "below the top border");
    assert!(near(pixel(&px, 40, 35, 20), red, 1), "right edge");
    assert!(near(pixel(&px, 40, 30, 20), red, 1), "right, 6 px deep");
    assert!(
        near(pixel(&px, 40, 29, 20), blue, 1),
        "inside the right border"
    );
    assert!(near(pixel(&px, 40, 20, 35), red, 1), "bottom edge");
    assert!(near(pixel(&px, 40, 20, 32), red, 1), "bottom, 4 px deep");
    assert!(
        near(pixel(&px, 40, 20, 31), blue, 1),
        "above the bottom border"
    );
    assert!(near(pixel(&px, 40, 4, 20), red, 1), "left edge, 1 px");
    assert!(
        near(pixel(&px, 40, 5, 20), blue, 1),
        "inside the left border"
    );
    assert!(near(pixel(&px, 40, 20, 20), blue, 1), "centre");
    d.destroy(t);
}

#[test]
fn a_translucent_quad_blends_over_the_clear() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(16, 16, &[]);
    let q = queue(16.0, 16.0, Color::rgb(0.0, 0.0, 1.0), whole(16.0, 16.0));
    let q = translucent(
        q,
        vec![quad(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            0.0,
            Color::rgba(1.0, 0.0, 0.0, 0.5),
            None,
        )],
    );
    d.draw(&t, &q);
    let px = d.read(&t);
    png("a_translucent_quad_blends_over_the_clear", 16, 16, &px);
    assert!(
        near(pixel(&px, 16, 8, 8), rgb(0.5, 0.0, 0.5), 2),
        "{:?}",
        pixel(&px, 16, 8, 8)
    );
    d.destroy(t);
}

#[test]
fn opaque_compositing_equals_blending() {
    let Some((_g, mut d)) = device() else { return };
    let clear = Color::rgb(0.1, 0.3, 0.7);
    let mut c = quad(
        Rect::new(3.0, 3.0, 26.0, 26.0),
        0.0,
        Color::rgba(0.9, 0.5, 0.2, 0.6),
        None,
    );
    c.radii = Corners::all(6.0);
    c.border = Insets::all(3.0);
    c.border_color = Color::rgba(0.2, 0.9, 0.4, 0.8);

    let a = d.target(32, 32, &[]);
    let mut o = c;
    o.background = clear;
    o.flags = Command::pack(Command::QUAD, true, false, 1.0);
    d.draw(
        &a,
        &opaque(queue(32.0, 32.0, clear, whole(32.0, 32.0)), vec![o]),
    );
    let composited = d.read(&a);

    let b = d.target(32, 32, &[]);
    d.draw(
        &b,
        &translucent(queue(32.0, 32.0, clear, whole(32.0, 32.0)), vec![c]),
    );
    let blended = d.read(&b);

    png(
        "opaque_compositing_equals_blending.opaque",
        32,
        32,
        &composited,
    );
    png(
        "opaque_compositing_equals_blending.translucent",
        32,
        32,
        &blended,
    );
    for y in 0..32 {
        for x in 0..32 {
            let (p, q) = (pixel(&composited, 32, x, y), pixel(&blended, 32, x, y));
            assert!(near(p, q, 2), "at {x},{y}: opaque {p:?}, blended {q:?}");
        }
    }
    d.destroy(a);
    d.destroy(b);
}

#[test]
fn a_nearer_opaque_quad_hides_a_farther_one_listed_after_it() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(32, 32, &[]);
    let q = queue(32.0, 32.0, Color::BLACK, whole(32.0, 32.0));
    let mut q = queue_with_depth(q, 4.0);
    q.opaque = Pass {
        scissor: whole(32.0, 32.0),
        commands: vec![
            quad(
                Rect::new(8.0, 8.0, 16.0, 16.0),
                2.0,
                Color::rgb(1.0, 0.0, 0.0),
                Some(Color::BLACK),
            ),
            quad(
                Rect::new(0.0, 0.0, 32.0, 32.0),
                0.0,
                Color::rgb(0.0, 1.0, 0.0),
                Some(Color::BLACK),
            ),
        ],
    };
    d.draw(&t, &q);
    let px = d.read(&t);
    png(
        "a_nearer_opaque_quad_hides_a_farther_one_listed_after_it",
        32,
        32,
        &px,
    );
    assert_eq!(
        pixel(&px, 32, 16, 16),
        rgb(1.0, 0.0, 0.0),
        "the nearer red stays"
    );
    assert_eq!(
        pixel(&px, 32, 2, 2),
        rgb(0.0, 1.0, 0.0),
        "green where red is not"
    );
    d.destroy(t);
}

#[test]
fn a_translucent_quad_behind_an_opaque_one_is_hidden_by_depth() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(32, 32, &[]);
    let mut q = queue_with_depth(queue(32.0, 32.0, Color::BLACK, whole(32.0, 32.0)), 4.0);
    q.opaque = Pass {
        scissor: whole(32.0, 32.0),
        commands: vec![quad(
            Rect::new(8.0, 8.0, 16.0, 16.0),
            2.0,
            Color::WHITE,
            Some(Color::BLACK),
        )],
    };
    q.translucent = Pass {
        scissor: whole(32.0, 32.0),
        commands: vec![quad(
            Rect::new(0.0, 0.0, 32.0, 32.0),
            0.0,
            Color::rgba(1.0, 0.0, 0.0, 0.5),
            None,
        )],
    };
    d.draw(&t, &q);
    let px = d.read(&t);
    png(
        "a_translucent_quad_behind_an_opaque_one_is_hidden_by_depth",
        32,
        32,
        &px,
    );
    assert_eq!(
        pixel(&px, 32, 16, 16),
        rgb(1.0, 1.0, 1.0),
        "white untouched"
    );
    assert!(
        near(pixel(&px, 32, 2, 2), rgb(0.5, 0.0, 0.0), 2),
        "red blended over black elsewhere"
    );
    d.destroy(t);
}

#[test]
fn a_pass_is_drawn_only_inside_its_scissor() {
    let Some((_g, mut d)) = device() else { return };
    let t = d.target(32, 32, &[]);
    let mut q = queue(32.0, 32.0, Color::BLACK, whole(32.0, 32.0));
    q.opaque = Pass {
        scissor: vec![Rect::new(0.0, 0.0, 16.0, 32.0)],
        commands: vec![quad(
            Rect::new(0.0, 0.0, 32.0, 32.0),
            0.0,
            Color::WHITE,
            Some(Color::BLACK),
        )],
    };
    d.draw(&t, &q);
    let px = d.read(&t);
    png("a_pass_is_drawn_only_inside_its_scissor", 32, 32, &px);
    assert_eq!(pixel(&px, 32, 8, 16), rgb(1.0, 1.0, 1.0));
    assert_eq!(pixel(&px, 32, 24, 16), rgb(0.0, 0.0, 0.0));
    d.destroy(t);
}

fn queue_with_depth(mut q: Queue, depth: f32) -> Queue {
    q.depth = depth;
    q
}

#[test]
fn a_bogus_modifier_falls_back_to_a_linear_buffer() {
    let Some((_g, mut d)) = device() else { return };
    // No vendor owns 0x00ff_ffff_ffff_fffe, so GBM refuses the list and
    // `target` falls back to `RENDERING | LINEAR`, which every driver can
    // render into and every compositor can import.
    let t = d.target(16, 16, &[0x00ff_ffff_ffff_fffe]);
    assert_eq!(t.modifier(), 0, "DRM_FORMAT_MOD_LINEAR");
    assert_eq!(t.planes().count(), 1);
    d.destroy(t);
}

#[test]
fn the_buffers_first_row_is_the_drawings_top_row() {
    let Some((_g, mut d)) = device() else { return };
    let (w, h) = (16u32, 8u32);
    // A linear target, so the CPU map below reads the pixels as they lie.
    let t = d.target(w, h, &[]);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let q = queue(w as f32, h as f32, blue, whole(w as f32, h as f32));
    let q = opaque(
        q,
        vec![quad(
            Rect::new(0.0, 0.0, w as f32, 1.0),
            0.0,
            Color::WHITE,
            Some(blue),
        )],
    );
    d.draw(&t, &q);
    let px = d.read(&t);
    png("the_buffers_first_row_is_the_drawings_top_row", w, h, &px);
    for x in 0..w {
        assert_eq!(pixel(&px, w, x, 0), rgb(1.0, 1.0, 1.0), "read row 0 at {x}");
    }
    for x in 0..w {
        assert_eq!(
            pixel(&px, w, x, h - 1),
            rgb(0.0, 0.0, 1.0),
            "read row {} at {x}",
            h - 1
        );
    }
    // `read` shares its y with the shader and the scissor, so the two
    // assertions above hold under a flip in all three. This one does not:
    // it reads the dmabuf's memory, whose first row is the row the
    // compositor puts at the top of the window. XRGB8888 is
    // little-endian, so the bytes are B, G, R, X.
    let row = d.first_row(&t);
    assert_eq!(row.len(), w as usize * 4);
    for x in 0..w as usize {
        assert_eq!(
            &row[x * 4..x * 4 + 3],
            &[255, 255, 255],
            "buffer row 0 at {x}"
        );
    }
    d.destroy(t);
}
