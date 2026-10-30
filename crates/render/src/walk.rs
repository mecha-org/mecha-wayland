//! The walk: one window's subtree in preorder, `Layout` and `AnimatedPaint` read
//! into commands in device pixels, the solid, the scroll and the clip
//! above each node carried down, and the damage of dirty nodes collected.
//! Nothing here is a system; `on_frame` calls [`walk`] once per `Frame`.

use animation::{AnimatedPaint, Layout};
use app::{Comps, CompsMut, NodeId, Tree};
use geometry::{Color, Corners, Insets, Point, Rect};
use layout::Viewport;
use paint::{MonochromeSprite, Paint, PolychromeSprite, Quad};

use crate::{
    Command, Drawn, NO_TILE, rect,
    scene::{Mover, Scene, Scrolled},
};

/// A colour and the device rect known to be exactly that colour behind a
/// node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Solid {
    pub(crate) color: Color,
    pub(crate) rect: Rect,
}

/// A node still to visit: what its ancestors hand it. `shift` is the
/// sum of their `Viewport` scrolls, logical pixels, taken off the node's
/// layout; `clip` is the device rect its pixels are kept to, the window
/// when no ancestor clips.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Visit {
    pub(crate) id: NodeId,
    pub(crate) solid: Option<Solid>,
    pub(crate) shift: Point,
    pub(crate) clip: Rect,
    /// Inside the viewport that scrolled this walk: how far its scroll
    /// moved since the last walk, logical pixels.
    pub(crate) moved: Option<Point>,
}

/// Logical to device: both edges scaled and rounded, so rects that touch
/// in logical pixels touch in device pixels.
pub(crate) fn scale_rect(r: Rect, s: f32) -> Rect {
    let x0 = (r.x() * s).round();
    let y0 = (r.y() * s).round();
    let x1 = (r.right() * s).round();
    let y1 = (r.bottom() * s).round();
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// The rect inside a quad's edges: inset on every side by the larger of
/// the greatest radius and the greatest border width. Device values in,
/// device rect out.
pub(crate) fn interior(r: Rect, radii: Corners<f32>, border: Insets<f32>) -> Rect {
    let radius = radii
        .top_left
        .max(radii.top_right)
        .max(radii.bottom_right)
        .max(radii.bottom_left);
    let width = border
        .top
        .max(border.right)
        .max(border.bottom)
        .max(border.left);
    r.inset(Insets::all(radius.max(width).max(0.0)))
}

/// A border a renderer would draw: some width, and a colour with alpha.
fn border_visible(q: &Quad) -> bool {
    let b = q.border;
    (b.top > 0.0 || b.right > 0.0 || b.bottom > 0.0 || b.left > 0.0)
        && !q.border_color.is_transparent()
}

/// The solid a quad's children receive. `inside` says the quad lies
/// within `solid`'s rect; `interior` is the quad's interior in device
/// pixels. The flag `is_opaque` plays no part: it picks the pass, and the
/// pixels are that colour either way.
pub(crate) fn hand_down(
    q: &Quad,
    solid: Option<Solid>,
    inside: bool,
    interior: Rect,
) -> Option<Solid> {
    // A fill that leaves no pixel is asked first: composited onto a solid
    // it reads as opaque, and answering that way would narrow the solid
    // to this quad's interior for no reason.
    if q.color.is_transparent() {
        return if border_visible(q) {
            solid.map(|s| Solid {
                color: s.color,
                rect: rect::intersection(s.rect, interior),
            })
        } else {
            solid
        };
    }
    let effective = match (inside, solid) {
        (true, Some(s)) => q.color.over(s.color),
        _ => q.color,
    };
    if effective.a >= 1.0 {
        Some(Solid {
            color: effective,
            rect: interior,
        })
    } else {
        None
    }
}

/// Preorder over `window`'s subtree. Every visited node takes a z; a
/// visible paint in a non-empty box emits its commands into `scene`,
/// each at its layout less the shift above it and carrying the clip
/// above it, unless the command lies wholly outside that clip; a dirty
/// node, or a clean one whose bounds or layout changed since last frame,
/// adds its old and its new bounds to `scene.damage`, one rect when they
/// coincide; every node that drew is recorded in `scene.drawing`. Bounds
/// are cut to the clip. A `Viewport` adds its scroll to the shift its
/// descendants get and, with `clip`, cuts their clip to its padding box.
/// Returns how many nodes were visited.
pub(crate) fn walk(
    tree: Tree<'_>,
    layouts: &Comps<'_, Layout>,
    paints: &Comps<'_, AnimatedPaint>,
    viewports: &Comps<'_, Viewport>,
    drawn: &mut CompsMut<'_, Drawn>,
    window: NodeId,
    scale: f32,
    clear: Color,
    scene: &mut Scene,
) -> u32 {
    let root_solid = (clear.a >= 1.0).then(|| Solid {
        color: clear,
        rect: scene.window_rect(),
    });
    // The scene lends its stack, so a steady frame allocates nothing; the
    // loop body needs the rest of the scene mutably, hence the take.
    let mut stack = std::mem::take(&mut scene.stack);
    stack.clear();
    stack.push(Visit {
        id: window,
        solid: root_solid,
        shift: Point::default(),
        clip: scene.window_rect(),
        moved: None,
    });
    let mut visited = 0u32;
    while let Some(Visit {
        id,
        solid,
        shift,
        clip,
        moved,
    }) = stack.pop()
    {
        let z = 2.0 * visited as f32;
        visited += 1;
        let (Some(layout), Some(animated)) = (layouts.get(id), paints.get(id)) else {
            continue;
        };
        let paint = &animated.0;
        // Where the node is displayed: its layout less the scroll above it.
        let layout = Layout {
            rect: Rect {
                origin: layout.rect.origin - shift,
                size: layout.rect.size,
            },
            ..*layout
        };
        let r = scale_rect(layout.rect, scale);
        let mut bounds: Option<Rect> = None;
        let mut child_solid = solid;
        if !r.is_empty() && !paint.is_invisible() {
            match paint {
                Paint::None => {}
                Paint::Quad(q) => {
                    child_solid = emit_quad(q, r, z, solid, clip, scale, scene);
                    bounds = Some(r);
                }
                Paint::Monochrome(run) => {
                    bounds = emit_run(run, layout.content(), z, solid, clip, scale, scene);
                }
                Paint::Polychrome(p) => {
                    let content = scale_rect(layout.content(), scale);
                    if !content.is_empty() {
                        emit_image(p, content, z, solid, clip, scale, scene);
                        bounds = Some(content);
                        child_solid = None;
                    }
                }
            }
        }
        let bounds = bounds
            .map(|b| rect::intersection(b, clip))
            .filter(|b| !b.is_empty());
        // A node that drew nothing keeps no layout: its moves cannot
        // damage, so a hidden row scrolling past writes no record.
        let kept = bounds.map(|_| layout);
        if let Some(mut d) = drawn.get_mut(id) {
            // Layout may be written at frame time, after the last drain,
            // and affect pixels even when the emitted bounds stay equal.
            // The shift is in it, so a scroll that keeps the clipped
            // bounds equal still damages them.
            if d.dirty || d.rect != bounds || d.layout != kept {
                // Only a clean node carried by the one scroll keeps the
                // frame a pure scroll.
                let carried = moved.is_some_and(|m| carried_by(d.layout, kept, m));
                if d.dirty || !carried {
                    scene.pure = false;
                }
                if let Some(old) = d.rect {
                    scene.damage.push(old);
                }
                // A node that was marked but did not move damages one rect,
                // not the same one twice.
                if let Some(new) = bounds
                    && d.rect != Some(new)
                {
                    scene.damage.push(new);
                }
            }
            d.set_if_neq(Drawn {
                rect: bounds,
                layout: kept,
                dirty: false,
            });
        }
        if let Some(b) = bounds {
            scene.drawing.push((id, b));
            if moved.is_none() {
                scene.outside.push((id, b));
            }
        }
        let (shift, clip, moved) = match viewports.get(id) {
            Some(v) => {
                let clip = if v.clip {
                    let padding_box = scale_rect(layout.rect.inset(layout.border), scale);
                    rect::intersection(clip, padding_box)
                } else {
                    clip
                };
                // Whole device pixels, so every edge moves by the same
                // amount and a row keeps its height while it scrolls.
                let device = Point::new((v.scroll.x * scale).round(), (v.scroll.y * scale).round());
                let snapped = Point::new(device.x / scale, device.y / scale);
                let moved = scrolled(scene, id, v.clip, device, clip, child_solid, moved, scale);
                (shift + snapped, clip, moved)
            }
            None => (shift, clip, moved),
        };
        if let Some(children) = tree.children(id) {
            for &child in children.iter().rev() {
                stack.push(Visit {
                    id: child,
                    solid: child_solid,
                    shift,
                    clip,
                    moved,
                });
            }
        }
    }
    scene.stack = stack;
    visited
}

/// Note a viewport's device `scroll` and visible `area` for the next walk
/// and judge its move since the last one. Returns what its descendants
/// carry: `moved` from above, or this viewport's move in logical pixels
/// if it is the one that scrolled. A move fails the pure scroll when the
/// viewport does not clip, sits inside another that moved, follows
/// another mover, changed its area, or has anything but one colour behind
/// its area.
#[allow(clippy::too_many_arguments)]
fn scrolled(
    scene: &mut Scene,
    id: NodeId,
    clips: bool,
    scroll: Point,
    area: Rect,
    behind: Option<Solid>,
    moved: Option<Point>,
    scale: f32,
) -> Option<Point> {
    scene.viewing.push(Scrolled { id, scroll, area });
    let Some(last) = scene.viewed.iter().find(|s| s.id == id).copied() else {
        return moved;
    };
    if last.scroll == scroll {
        return moved;
    }
    let flat = behind.is_some_and(|s| rect::contains(s.rect, area));
    if !clips || moved.is_some() || scene.mover.is_some() || last.area != area || !flat {
        scene.pure = false;
    }
    let by = scroll - last.scroll;
    scene.mover = Some(Mover { id, area, by });
    Some(Point::new(by.x / scale, by.y / scale))
}

/// A node's displayed layout went from `old` to `new` only by the scroll
/// moving `by` logical pixels: same box, origin up by `by`. A node coming
/// into or going out of sight is carried too.
fn carried_by(old: Option<Layout>, new: Option<Layout>, by: Point) -> bool {
    let (Some(old), Some(new)) = (old, new) else {
        return true;
    };
    let gap = old.rect.origin - new.rect.origin - by;
    gap.x.abs() < 1e-3
        && gap.y.abs() < 1e-3
        && old.rect.size == new.rect.size
        && old.padding == new.padding
        && old.border == new.border
}

/// The quad's three routes, and what it hands down, cut to the clip.
fn emit_quad(
    q: &Quad,
    r: Rect,
    z: f32,
    solid: Option<Solid>,
    clip: Rect,
    scale: f32,
    scene: &mut Scene,
) -> Option<Solid> {
    // Judged on the part the clip keeps, so a row sliding under a
    // viewport's edge takes the same route all the way.
    let kept = rect::intersection(r, clip);
    let inside = solid.is_some_and(|s| rect::contains(s.rect, kept));
    let radii = q.radii.map(|v| v * scale);
    let border = q.border.map(|v| v * scale);
    let visible = border_visible(q);
    let edge_free = q.radii.is_zero() && !visible;
    // The interior keeps the unfiltered widths: an invisible border still
    // says how far in the fill is flat.
    let inner = interior(r, radii, border);
    // `border_color == color` and no width is how a command says it has no
    // border, so a shader compositing the ring paints the fill there.
    let base = Command {
        rect: r,
        z,
        color: q.color,
        border_color: if visible { q.border_color } else { q.color },
        background: Color::TRANSPARENT,
        radii,
        border: if visible { border } else { Insets::all(0.0) },
        tile: NO_TILE,
        flags: Command::pack(Command::QUAD, false, false, 1.0),
        clip,
    };
    let opaque = Command::pack(Command::QUAD, true, false, 1.0);
    if q.is_opaque && inside {
        // The shader composites the whole quad, edges and border included,
        // onto the solid.
        let s = solid.expect("inside implies a solid");
        push(
            &mut scene.opaque,
            Command {
                background: s.color,
                flags: opaque,
                ..base
            },
        );
        // Its flat middle as a plain fill one step nearer: the depth test
        // keeps the box's shader to the rim.
        if !edge_free && q.color.a >= 1.0 && !inner.is_empty() {
            let flat = q.color.over(s.color);
            push(
                &mut scene.opaque,
                Command {
                    rect: inner,
                    z: z + 1.0,
                    color: flat,
                    border_color: flat,
                    background: flat,
                    radii: Corners::all(0.0),
                    border: Insets::all(0.0),
                    flags: opaque,
                    ..base
                },
            );
        }
    } else if q.is_opaque && edge_free && q.color.a >= 1.0 {
        // A plain fill.
        push(
            &mut scene.opaque,
            Command {
                background: q.color,
                flags: opaque,
                ..base
            },
        );
    } else {
        push(&mut scene.translucent, base);
        // The interior split: an opaque fill with edges and nothing known
        // behind it writes depth for its flat middle.
        if q.is_opaque && q.color.a >= 1.0 && !inner.is_empty() {
            push(
                &mut scene.opaque,
                Command {
                    rect: inner,
                    z: z + 1.0,
                    border_color: q.color,
                    background: q.color,
                    radii: Corners::all(0.0),
                    border: Insets::all(0.0),
                    flags: opaque,
                    ..base
                },
            );
        }
    }
    // Nothing of the quad shows outside the clip, so nothing is known
    // there.
    hand_down(q, solid, inside, inner).map(|s| Solid {
        color: s.color,
        rect: rect::intersection(s.rect, clip),
    })
}

/// Into `list`, unless the command has no pixel inside its clip.
fn push(list: &mut Vec<Command>, c: Command) {
    if rect::intersects(c.rect, c.clip) {
        list.push(c);
    }
}

/// One command per visible sprite, from the content box's corner. Returns
/// the union of their rects.
fn emit_run(
    run: &[MonochromeSprite],
    content: Rect,
    z: f32,
    solid: Option<Solid>,
    clip: Rect,
    scale: f32,
    scene: &mut Scene,
) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for s in run {
        if s.is_invisible() {
            continue;
        }
        let logical = Rect::new(
            content.x() + s.offset.x,
            content.y() + s.offset.y,
            s.size.width,
            s.size.height,
        );
        let sr = scale_rect(logical, scale);
        if sr.is_empty() {
            continue;
        }
        let over = if s.is_opaque {
            solid.filter(|so| rect::contains(so.rect, rect::intersection(sr, clip)))
        } else {
            None
        };
        let cmd = Command {
            rect: sr,
            z,
            color: s.color,
            border_color: s.color,
            background: over.map_or(Color::TRANSPARENT, |o| o.color),
            radii: Corners::all(0.0),
            border: Insets::all(0.0),
            tile: s.tile,
            flags: Command::pack(Command::SPRITE, over.is_some(), false, 1.0),
            clip,
        };
        if over.is_some() {
            push(&mut scene.opaque, cmd);
        } else {
            push(&mut scene.translucent, cmd);
        }
        bounds = Some(bounds.map_or(sr, |b| rect::union(b, sr)));
    }
    bounds
}

/// One command over the content box. A tile composited onto a known
/// colour is opaque whatever its alpha, radii included.
fn emit_image(
    p: &PolychromeSprite,
    content: Rect,
    z: f32,
    solid: Option<Solid>,
    clip: Rect,
    scale: f32,
    scene: &mut Scene,
) {
    let over = if p.is_opaque {
        solid.filter(|so| rect::contains(so.rect, rect::intersection(content, clip)))
    } else {
        None
    };
    let cmd = Command {
        rect: content,
        z,
        color: Color::WHITE,
        border_color: Color::WHITE,
        background: over.map_or(Color::TRANSPARENT, |o| o.color),
        radii: p.radii.map(|v| v * scale),
        border: Insets::all(0.0),
        tile: p.tile,
        flags: Command::pack(Command::IMAGE, over.is_some(), p.grayscale, p.opacity),
        clip,
    };
    if over.is_some() {
        push(&mut scene.opaque, cmd);
    } else {
        push(&mut scene.translucent, cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);

    #[test]
    fn scaling_rounds_both_edges_so_neighbours_keep_touching() {
        assert_eq!(
            scale_rect(Rect::new(0.0, 0.0, 3.0, 3.0), 1.5),
            Rect::new(0.0, 0.0, 5.0, 5.0)
        );
        assert_eq!(
            scale_rect(Rect::new(3.0, 0.0, 3.0, 3.0), 1.5),
            Rect::new(5.0, 0.0, 4.0, 5.0)
        );
        assert_eq!(
            scale_rect(Rect::new(1.0, 2.0, 3.0, 4.0), 2.0),
            Rect::new(2.0, 4.0, 6.0, 8.0)
        );
        assert_eq!(
            scale_rect(Rect::new(1.0, 2.0, 3.0, 4.0), 1.0),
            Rect::new(1.0, 2.0, 3.0, 4.0)
        );
    }

    #[test]
    fn the_interior_is_inset_by_the_larger_of_radius_and_border() {
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert_eq!(
            interior(r, Corners::all(6.0), Insets::new(8.0, 0.0, 0.0, 0.0)),
            Rect::new(8.0, 8.0, 84.0, 34.0)
        );
        assert_eq!(
            interior(r, Corners::new(0.0, 6.0, 0.0, 0.0), Insets::all(2.0)),
            Rect::new(6.0, 6.0, 88.0, 38.0)
        );
        assert_eq!(interior(r, Corners::all(0.0), Insets::all(0.0)), r);
    }

    #[test]
    fn what_a_quad_hands_down() {
        let inner = Rect::new(12.0, 12.0, 46.0, 16.0);
        let black = Some(Solid {
            color: Color::BLACK,
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
        });

        // An opaque fill: the solid is that colour over the interior.
        assert_eq!(
            hand_down(&Quad::new(RED), None, false, inner),
            Some(Solid {
                color: RED,
                rect: inner
            })
        );
        // A translucent fill inside a solid composites to an opaque one.
        let half = Color::rgba(1.0, 0.0, 0.0, 0.5);
        assert_eq!(
            hand_down(&Quad::new(half), black, true, inner),
            Some(Solid {
                color: half.over(Color::BLACK),
                rect: inner
            })
        );
        // A translucent fill with nothing known behind it: nothing known.
        assert_eq!(hand_down(&Quad::new(half), None, false, inner), None);
        assert_eq!(hand_down(&Quad::new(half), black, false, inner), None);
        // A transparent fill with a border narrows the solid to the interior.
        let ring = Quad::new(Color::TRANSPARENT).border(2.0, GREEN);
        assert_eq!(
            hand_down(&ring, black, true, inner),
            Some(Solid {
                color: Color::BLACK,
                rect: inner
            })
        );
        assert_eq!(hand_down(&ring, None, false, inner), None);
        // A transparent fill with no border passes the solid through.
        assert_eq!(hand_down(&Quad::default(), black, true, inner), black);
    }
}
