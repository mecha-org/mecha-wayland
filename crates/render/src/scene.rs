//! A window's scene: the full sorted command lists of the last walk, what
//! that walk drew per node, the damage of the last few frames, the stack
//! the walk descends with, and the reusable queue a backend reads. Every
//! one of those is a buffer kept between frames, so a steady frame
//! allocates nothing. Nothing here reads a column; the walk fills it and
//! `queue` filters it.

use std::collections::VecDeque;

use app::NodeId;
use geometry::{Color, Rect, Size};

use crate::{Command, Pass, Queue, rect, walk::Visit};

/// Past this many rects a damage list collapses to its bounding rect.
const MAX_RECTS: usize = 16;

#[derive(Debug)]
pub(crate) struct Scene {
    size: Size,
    scale: f32,
    clear: Color,
    depth: f32,
    /// Every opaque command of the last walk, z descending after `finish`.
    pub(crate) opaque: Vec<Command>,
    /// Every translucent command of the last walk, z ascending after `finish`.
    pub(crate) translucent: Vec<Command>,
    /// What last frame drew: each node with the bounds of its commands.
    pub(crate) drawn: Vec<(NodeId, Rect)>,
    /// What this frame is drawing; swapped into `drawn` by `finish`.
    pub(crate) drawing: Vec<(NodeId, Rect)>,
    /// This frame's damage as the walk collects it, device pixels, not
    /// yet clamped.
    pub(crate) damage: Vec<Rect>,
    /// The walk's preorder stack, kept so a frame does not allocate one:
    /// each node still to visit with what its ancestors hand it.
    pub(crate) stack: Vec<Visit>,
    /// The last `buffers` frames' damage, newest first, each clamped and
    /// collapsed.
    history: VecDeque<Vec<Rect>>,
    buffers: usize,
    /// No frame has been walked yet.
    fresh: bool,
    queue: Queue,
}

impl Scene {
    pub(crate) fn new(buffers: usize) -> Self {
        Self {
            size: Size::ZERO,
            scale: 1.0,
            clear: Color::BLACK,
            depth: 0.0,
            opaque: Vec::new(),
            translucent: Vec::new(),
            drawn: Vec::new(),
            drawing: Vec::new(),
            damage: Vec::new(),
            stack: Vec::new(),
            history: VecDeque::with_capacity(buffers),
            buffers,
            fresh: true,
            queue: Queue::default(),
        }
    }

    /// The window in device pixels, at the origin.
    pub(crate) fn window_rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.size.width, self.size.height)
    }

    /// Start a frame: store the window's facts and empty this frame's
    /// lists. Returns whether the whole window is damaged: the first
    /// frame, or a size or scale that differs from last frame's.
    pub(crate) fn begin(&mut self, size: Size, scale: f32, clear: Color) -> bool {
        let full = self.fresh || self.size != size || self.scale != scale;
        self.fresh = false;
        self.size = size;
        self.scale = scale;
        self.clear = clear;
        self.opaque.clear();
        self.translucent.clear();
        self.drawing.clear();
        self.damage.clear();
        self.stack.clear();
        full
    }

    /// End a frame after the walk: sort the passes, take the drawing list
    /// over as what was drawn, and file this frame's damage, the whole
    /// window if `full`. `visited` is how many nodes the walk took a z
    /// for.
    pub(crate) fn finish(&mut self, full: bool, visited: u32) {
        self.depth = 2.0 * visited as f32;
        self.opaque.sort_by(|a, b| b.z.total_cmp(&a.z));
        self.translucent.sort_by(|a, b| a.z.total_cmp(&b.z));
        std::mem::swap(&mut self.drawn, &mut self.drawing);

        // The oldest list, if one falls off, becomes the next frame's
        // collector, so the ring allocates only while it grows.
        let mut damage = std::mem::take(&mut self.damage);
        let window = self.window_rect();
        if full {
            damage.clear();
            if !window.is_empty() {
                damage.push(window);
            }
        } else {
            clamp(&mut damage, window);
            collapse(&mut damage);
        }
        self.history.push_front(damage);
        while self.history.len() > self.buffers {
            let mut spare = self.history.pop_back().expect("longer than buffers");
            spare.clear();
            self.damage = spare;
        }
    }

    /// The commands for a buffer of `age`: the scissor is the union of the
    /// newest `age` frames' damage, or the window when `age` is 0 or past
    /// the frames held; each pass gets the commands that touch it.
    ///
    /// The union is one pass, newest frame first and push order within a
    /// frame: a rect is dropped when a rect already kept contains it; a
    /// kept rect is not removed when a later, larger one arrives. So a
    /// widget repainting every frame is cleared and drawn once, not once
    /// per age.
    pub(crate) fn queue(&mut self, age: usize) -> &Queue {
        let window = self.window_rect();
        let q = &mut self.queue;
        q.size = self.size;
        q.scale = self.scale;
        q.clear = self.clear;
        q.depth = self.depth;
        q.scissor.clear();
        if age >= 1 && age <= self.history.len() {
            for list in self.history.iter().take(age) {
                for r in list.iter().copied() {
                    if !q.scissor.iter().any(|kept| rect::contains(*kept, r)) {
                        q.scissor.push(r);
                    }
                }
            }
            collapse(&mut q.scissor);
        } else if !window.is_empty() {
            q.scissor.push(window);
        }
        fill(&mut q.opaque, &self.opaque, &q.scissor);
        fill(&mut q.translucent, &self.translucent, &q.scissor);
        q
    }
}

/// The commands whose part inside their clip touches any scissor rect,
/// in order, and the rects one of them touched. `scissor` holds at most `MAX_RECTS` entries after
/// `collapse`, so a word of bits tracks which were touched.
fn fill(pass: &mut Pass, commands: &[Command], scissor: &[Rect]) {
    debug_assert!(scissor.len() <= MAX_RECTS);
    pass.commands.clear();
    pass.scissor.clear();
    let mut touched: u32 = 0;
    for c in commands {
        let shown = rect::intersection(c.rect, c.clip);
        let mut hit = false;
        for (i, s) in scissor.iter().enumerate() {
            if rect::intersects(shown, *s) {
                touched |= 1 << i;
                hit = true;
            }
        }
        if hit {
            pass.commands.push(*c);
        }
    }
    for (i, s) in scissor.iter().enumerate() {
        if touched & (1 << i) != 0 {
            pass.scissor.push(*s);
        }
    }
}

/// Cut every rect to the window and drop what falls outside.
fn clamp(rects: &mut Vec<Rect>, window: Rect) {
    rects.retain_mut(|r| {
        *r = rect::intersection(*r, window);
        !r.is_empty()
    });
}

/// Past `MAX_RECTS`, one bounding rect.
fn collapse(rects: &mut Vec<Rect>) {
    if rects.len() > MAX_RECTS {
        let all = rects.iter().copied().fold(Rect::ZERO, rect::union);
        rects.clear();
        rects.push(all);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NO_TILE;
    use app::App;
    use geometry::{Corners, Insets};

    fn cmd(rect: Rect, z: f32) -> Command {
        Command {
            rect,
            z,
            color: Color::WHITE,
            border_color: Color::WHITE,
            background: Color::TRANSPARENT,
            radii: Corners::all(0.0),
            border: Insets::all(0.0),
            tile: NO_TILE,
            flags: 0,
            clip: WINDOW,
        }
    }

    const WINDOW: Rect = Rect::new(0.0, 0.0, 100.0, 50.0);

    fn scene() -> Scene {
        let mut s = Scene::new(2);
        assert!(
            s.begin(WINDOW.size, 1.0, Color::BLACK),
            "the first frame is full"
        );
        s.finish(true, 1);
        s
    }

    #[test]
    fn the_first_frame_and_a_new_size_or_scale_damage_the_window() {
        let mut s = scene();
        assert_eq!(s.queue(1).scissor, vec![WINDOW]);
        assert!(!s.begin(WINDOW.size, 1.0, Color::BLACK), "same again");
        assert!(
            s.begin(Size::new(120.0, 50.0), 1.0, Color::BLACK),
            "a new size"
        );
        assert!(
            s.begin(Size::new(120.0, 50.0), 2.0, Color::BLACK),
            "a new scale"
        );
        s.finish(true, 1);
        assert_eq!(s.queue(1).scissor, vec![Rect::new(0.0, 0.0, 120.0, 50.0)]);
        assert_eq!(s.queue(1).depth, 2.0);
    }

    #[test]
    fn ages_union_the_history_and_anything_else_is_the_window() {
        let mut s = scene();
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 20.0, 10.0, 10.0);
        s.begin(WINDOW.size, 1.0, Color::BLACK);
        s.damage.push(a);
        s.finish(false, 1);
        assert_eq!(s.queue(1).scissor, vec![a]);
        assert_eq!(
            s.queue(2).scissor,
            vec![a, WINDOW],
            "the first frame is still held"
        );
        s.begin(WINDOW.size, 1.0, Color::BLACK);
        s.damage.push(b);
        s.finish(false, 1);
        assert_eq!(s.queue(1).scissor, vec![b]);
        assert_eq!(s.queue(2).scissor, vec![b, a], "newest first");
        assert_eq!(s.queue(3).scissor, vec![WINDOW], "past the two frames held");
        assert_eq!(s.queue(0).scissor, vec![WINDOW], "an unknown buffer");
    }

    #[test]
    fn the_union_drops_a_rect_another_contains() {
        let a = Rect::new(0.0, 0.0, 20.0, 20.0);
        let inner = Rect::new(5.0, 5.0, 5.0, 5.0);
        let mut s = Scene::new(3);
        assert!(s.begin(WINDOW.size, 1.0, Color::BLACK), "the first frame");
        s.finish(true, 1);
        for d in [a, a, inner] {
            s.begin(WINDOW.size, 1.0, Color::BLACK);
            s.damage.push(d);
            s.finish(false, 1);
        }
        assert_eq!(
            s.queue(3).scissor,
            vec![inner, a],
            "newest first; the second a is the first one again"
        );
        assert_eq!(
            s.queue(2).scissor,
            vec![inner, a],
            "a is kept though it arrived after the rect it contains"
        );

        // One frame's own list is deduplicated the same way.
        s.begin(WINDOW.size, 1.0, Color::BLACK);
        s.damage.push(a);
        s.damage.push(inner);
        s.finish(false, 1);
        assert_eq!(s.queue(1).scissor, vec![a], "inner is inside a");
    }

    #[test]
    fn damage_is_clamped_to_the_window_and_collapses_past_sixteen() {
        let mut s = scene();
        s.begin(WINDOW.size, 1.0, Color::BLACK);
        s.damage.push(Rect::new(90.0, 40.0, 20.0, 20.0));
        s.damage.push(Rect::new(200.0, 200.0, 5.0, 5.0));
        s.finish(false, 1);
        assert_eq!(s.queue(1).scissor, vec![Rect::new(90.0, 40.0, 10.0, 10.0)]);

        s.begin(WINDOW.size, 1.0, Color::BLACK);
        for i in 0..17 {
            s.damage.push(Rect::new(i as f32, 0.0, 1.0, 1.0));
        }
        s.finish(false, 1);
        assert_eq!(s.queue(1).scissor, vec![Rect::new(0.0, 0.0, 17.0, 1.0)]);
    }

    #[test]
    fn a_pass_holds_the_commands_that_touch_the_scissor_and_the_rects_they_touch() {
        let mut s = scene();
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(50.0, 20.0, 10.0, 10.0);
        s.begin(WINDOW.size, 1.0, Color::BLACK);
        s.opaque.push(cmd(Rect::new(5.0, 5.0, 10.0, 10.0), 2.0));
        s.opaque.push(cmd(Rect::new(30.0, 30.0, 5.0, 5.0), 4.0));
        s.translucent.push(cmd(Rect::new(0.0, 40.0, 5.0, 5.0), 6.0));
        s.damage.push(a);
        s.damage.push(b);
        s.finish(false, 4);
        let q = s.queue(1);
        assert_eq!(q.scissor, vec![a, b]);
        assert_eq!(q.opaque.scissor, vec![a], "only the rect a command touched");
        assert_eq!(q.opaque.commands.len(), 1);
        assert_eq!(q.opaque.commands[0].rect, Rect::new(5.0, 5.0, 10.0, 10.0));
        assert!(
            q.translucent.scissor.is_empty(),
            "nothing translucent under the damage"
        );
        assert!(q.translucent.commands.is_empty());
    }

    #[test]
    fn finish_sorts_opaque_front_to_back_and_translucent_back_to_front() {
        let mut s = scene();
        s.begin(WINDOW.size, 1.0, Color::BLACK);
        s.opaque.push(cmd(WINDOW, 2.0));
        s.opaque.push(cmd(WINDOW, 6.0));
        s.opaque.push(cmd(WINDOW, 4.0));
        s.translucent.push(cmd(WINDOW, 6.0));
        s.translucent.push(cmd(WINDOW, 2.0));
        s.finish(true, 4);
        let q = s.queue(1);
        let zs = |v: &[Command]| v.iter().map(|c| c.z).collect::<Vec<_>>();
        assert_eq!(zs(&q.opaque.commands), vec![6.0, 4.0, 2.0]);
        assert_eq!(zs(&q.translucent.commands), vec![2.0, 6.0]);
        assert_eq!(q.depth, 8.0);
    }

    #[test]
    fn finish_takes_the_drawing_list_over() {
        let app = App::new();
        let id = app.root();
        let mut s = scene();
        s.begin(WINDOW.size, 1.0, Color::BLACK);
        s.drawing.push((id, WINDOW));
        s.finish(false, 1);
        assert_eq!(s.drawn, vec![(id, WINDOW)]);
        s.begin(WINDOW.size, 1.0, Color::BLACK);
        assert!(s.drawing.is_empty(), "begin empties this frame's list");
        assert_eq!(s.drawn.len(), 1, "and keeps last frame's");
    }
}
