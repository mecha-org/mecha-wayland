//! The hit test: every node under a window whose `ComputedLayout` contains a
//! position, in dispatch order.

use app::{App, NodeId};
use geometry::{Point, Rect};
use layout::{ComputedLayout, Viewport};

use crate::contacts::HitSet;

/// Every node under `window` (`window` included) whose `ComputedLayout.rect`
/// contains `position`. A preorder walk collects the matches ancestor
/// first, then the list is reversed: deepest/frontmost first, `window`
/// last. That lines up with `render`'s rule that a later preorder index
/// paints nearer, without this crate ever reading `render`'s `Scene`.
///
/// The walk carries, per node, the shift and clip its ancestors'
/// `Viewport`s impose: the node's rect is moved by `-shift` before the
/// test, and a point outside `clip` matches nothing below the clipping
/// node. A `Viewport` does not move or clip its own node.
///
/// A node whose `ComputedLayout` was never written by `LayoutModule` is
/// `Rect::default()`, which is zero-sized and so never matches; no
/// separate filter is needed for it.
pub(crate) fn hit_test(app: &App, window: NodeId, position: Point) -> HitSet {
    let mut matches = HitSet::new();
    let mut stack = vec![(window, Point::default(), None::<Rect>)];
    while let Some((id, shift, clip)) = stack.pop() {
        let layout = app
            .component::<ComputedLayout>(id)
            .copied()
            .unwrap_or_default();
        let rect = Rect {
            origin: layout.rect.origin - shift,
            size: layout.rect.size,
        };
        if rect.contains(position) && clip.is_none_or(|c| c.contains(position)) {
            matches.push(id);
        }
        let (shift, clip) = match app.component::<Viewport>(id) {
            Some(v) => {
                let clip = if v.clip {
                    let padding_box = rect.inset(layout.border);
                    Some(clip.map_or(padding_box, |c| intersect(c, padding_box)))
                } else {
                    clip
                };
                (shift + v.scroll, clip)
            }
            None => (shift, clip),
        };
        if let Some(children) = app.tree().children(id) {
            stack.extend(children.iter().rev().map(|&c| (c, shift, clip)));
        }
    }
    matches.reverse();
    matches
}

/// The overlap of two rects; zero-sized (so it contains nothing) when
/// they do not meet.
fn intersect(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x().max(b.x()), a.y().max(b.y()));
    let (r, bt) = (a.right().min(b.right()), a.bottom().min(b.bottom()));
    Rect::new(x, y, (r - x).max(0.0), (bt - y).max(0.0))
}

#[cfg(test)]
mod tests {
    use app::prelude::*;
    use geometry::Insets;
    use layout::prelude::*;
    use window::prelude::*;

    use super::*;

    struct Leaf;
    impl Build for Leaf {
        type Widget = Leaf;
    }
    impl Widget for Leaf {
        type Builder = Leaf;
        fn build(b: Leaf, _: Handle<Self>, _: &mut Spawner<'_, Self>) -> Self {
            b
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_module(LayoutModule).add_module(WindowModule);
        app
    }

    /// An absolutely positioned box at `(x, y)`, `w` by `h`, relative to
    /// its containing block.
    fn at(x: f32, y: f32, w: f32, h: f32) -> LayoutStyle {
        LayoutStyle::default()
            .absolute()
            .inset(Insets::new(px(y), auto(), auto(), px(x)))
            .size(px(w), px(h))
    }

    /// A 200x200 window; a 120x120 card absolutely positioned at (20, 20);
    /// a 40x40 button, the card's one flow child, so it sits at the
    /// card's content-box origin — window-relative rect (20, 20, 40, 40).
    fn scene() -> (App, NodeId, NodeId, NodeId) {
        let mut app = app();
        let win = app.spawn(
            app.root(),
            window().layout(LayoutStyle::default().size(px(200.0), px(200.0))),
        );
        let card = app.spawn_with(win, Leaf, (at(20.0, 20.0, 120.0, 120.0),));
        let button = app.spawn_with(
            card,
            Leaf,
            (LayoutStyle::default().size(px(40.0), px(40.0)),),
        );
        app.tick();
        (app, win.id(), card.id(), button.id())
    }

    #[test]
    fn matches_every_nesting_level_deepest_first_window_last() {
        let (app, win, card, button) = scene();
        assert_eq!(
            hit_test(&app, win, Point::new(30.0, 30.0)).into_vec(),
            vec![button, card, win]
        );
    }

    #[test]
    fn a_point_inside_the_card_but_outside_the_button_skips_the_button() {
        let (app, win, card, _) = scene();
        assert_eq!(
            hit_test(&app, win, Point::new(100.0, 100.0)).into_vec(),
            vec![card, win]
        );
    }

    #[test]
    fn a_point_outside_the_card_matches_only_the_window() {
        let (app, win, ..) = scene();
        assert_eq!(
            hit_test(&app, win, Point::new(150.0, 150.0)).into_vec(),
            vec![win]
        );
    }

    #[test]
    fn the_half_open_rule_holds_at_a_nested_edge() {
        let (app, win, card, button) = scene();
        // the button's rect is (20, 20, 40, 40); its right/bottom edge is 60.
        assert_eq!(
            hit_test(&app, win, Point::new(59.9, 59.9)).into_vec(),
            vec![button, card, win]
        );
        assert_eq!(
            hit_test(&app, win, Point::new(60.0, 59.9)).into_vec(),
            vec![card, win],
            "the right edge is excluded"
        );
        assert_eq!(
            hit_test(&app, win, Point::new(20.0, 20.0)).into_vec(),
            vec![button, card, win],
            "the top-left corner is included"
        );
    }

    #[test]
    fn a_point_outside_the_window_matches_nothing() {
        let (app, win, ..) = scene();
        assert!(hit_test(&app, win, Point::new(250.0, 250.0)).is_empty());
    }

    fn scroll(app: &mut App, id: NodeId, clip: bool, x: f32, y: f32) {
        *app.component_mut::<Viewport>(id).unwrap() = Viewport {
            clip,
            scroll: Point::new(x, y),
        };
    }

    #[test]
    fn scroll_shifts_descendants_not_the_node() {
        let (mut app, win, card, button) = scene();
        scroll(&mut app, card, false, 0.0, 30.0);
        // the button now sits at (20, -10, 40, 40).
        assert_eq!(
            hit_test(&app, win, Point::new(30.0, 30.0)).into_vec(),
            vec![card, win],
            "its old place misses"
        );
        assert_eq!(
            hit_test(&app, win, Point::new(30.0, 0.0)).into_vec(),
            vec![button, win],
            "30 px higher hits it, even outside the unclipped card"
        );
        assert_eq!(
            hit_test(&app, win, Point::new(100.0, 100.0)).into_vec(),
            vec![card, win],
            "the card is still hit at its own rect"
        );
    }

    #[test]
    fn clip_hides_descendants_outside_the_padding_box() {
        let (mut app, win, card, button) = scene();
        // push the button to (20, 140): below the card's box (20..140).
        scroll(&mut app, card, true, 0.0, -120.0);
        assert_eq!(
            hit_test(&app, win, Point::new(30.0, 150.0)).into_vec(),
            vec![win],
            "outside the clip, the button is not hit"
        );
        scroll(&mut app, card, true, 0.0, -50.0);
        // the button at (20, 70, 40, 40) is inside the clip.
        assert_eq!(
            hit_test(&app, win, Point::new(30.0, 80.0)).into_vec(),
            vec![button, card, win]
        );
    }

    #[test]
    fn nested_shifts_add_and_clips_intersect() {
        let (mut app, win, card, button) = scene();
        let deep = app
            .spawn_with(
                button,
                Leaf,
                (LayoutStyle::default().size(px(10.0), px(10.0)),),
            )
            .id();
        app.tick();
        // deep's layout rect is (20, 20, 10, 10).
        scroll(&mut app, card, false, 0.0, 10.0);
        scroll(&mut app, button, true, 0.0, 5.0);
        // the button is at (20, 10, 40, 40); deep at (20, 5, 10, 10),
        // shifts 10 + 5 added, clipped to the button's box (y from 10).
        assert_eq!(
            hit_test(&app, win, Point::new(25.0, 12.0)).into_vec(),
            vec![deep, button, win]
        );
        assert_eq!(
            hit_test(&app, win, Point::new(25.0, 7.0)).into_vec(),
            vec![win],
            "deep is shifted there but outside the button's clip"
        );
        // an outer clip on the card (box 20..140) intersects the button's.
        scroll(&mut app, card, true, 0.0, 10.0);
        assert_eq!(
            hit_test(&app, win, Point::new(25.0, 12.0)).into_vec(),
            vec![win],
            "inside the button's clip, outside the card's"
        );
    }
}
