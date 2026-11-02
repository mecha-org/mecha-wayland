//! A `Scrollable` end to end: contacts in, `Viewport.scroll` out, frames
//! driven by a stand-in WSI on a clock the test advances.

use std::time::{Duration, Instant};

use animation::AnimationModule;
use app::prelude::*;
use geometry::Point;
use interactivity::prelude::*;
use layout::prelude::*;
use paint::prelude::*;
use render::prelude::*;
use scroll::ScrollClock;
use scroll::prelude::*;
use widgets::prelude::*;
use window::prelude::*;

/// How far the clock moves per answered frame.
const FRAME: Duration = Duration::from_millis(16);

/// Every `FrameRequested` not yet answered.
#[derive(Default)]
struct Requests(Vec<NodeId>);
impl Resource for Requests {}

/// The nodes each event reached, in order.
#[derive(Default)]
struct Log {
    presses: Vec<NodeId>,
    clicks: Vec<NodeId>,
    cancels: Vec<NodeId>,
}
impl Resource for Log {}

/// The WSI stand-in's ear: it answers each request with a `Frame` on the
/// next [`Fixture::frame`], as a compositor's callback comes later.
fn log_requested(app: &mut App, r: &FrameRequested) {
    app.resource_mut::<Requests>().0.push(r.0);
}
fn log_press(app: &mut App, e: &Emitted<Press>) {
    app.resource_mut::<Log>().presses.extend(e.targets.iter());
}
fn log_clicked(app: &mut App, e: &Emitted<Clicked>) {
    app.resource_mut::<Log>().clicks.extend(e.targets.iter());
}
fn log_cancelled(app: &mut App, e: &Emitted<PressCancelled>) {
    app.resource_mut::<Log>().cancels.extend(e.targets.iter());
}

struct Fixture {
    app: App,
    win: NodeId,
    list: NodeId,
    rows: Vec<NodeId>,
}

/// A 200×400 window filled by a `Scrollable` of `n` 50 px buttons.
fn fixture(n: usize, axis: Axis) -> Fixture {
    let mut app = App::new();
    app.add_module(LayoutModule)
        .add_module(PaintModule)
        .add_module(WindowModule)
        .add_module(InteractivityModule)
        .add_module(AnimationModule)
        .add_module(ScrollModule)
        .add_module(RenderModule::default())
        .init_resource::<Requests>()
        .init_resource::<Log>()
        .system(log_requested)
        .system(log_press)
        .system(log_clicked)
        .system(log_cancelled);
    app.resource_mut::<ScrollClock>().now = Some(Instant::now());
    let win = app.spawn(
        app.root(),
        window().layout(LayoutStyle::default().column().size(px(200.0), px(400.0))),
    );
    let style = match axis {
        Axis::Vertical => LayoutStyle::default().block().fill(),
        Axis::Horizontal => LayoutStyle::default().row().fill(),
    };
    let list = app.spawn(win, scrollable().axis(axis).style(style));
    let rows = (0..n)
        .map(|_| {
            let row = match axis {
                Axis::Vertical => button().height(px(50.0)),
                Axis::Horizontal => button().style(
                    LayoutStyle::default()
                        .size(px(50.0), percent(100.0))
                        .shrink(0.0),
                ),
            };
            app.spawn(list, row).id()
        })
        .collect();
    app.tick();
    let mut f = Fixture {
        app,
        win: win.id(),
        list: list.id(),
        rows,
    };
    while f.frame() {}
    f.app.resource_mut::<Log>().presses.clear();
    f
}

impl Fixture {
    fn input(&mut self, finger: u32, phase: ContactPhase, x: f32, y: f32, time: u32) {
        self.app.signal(ContactInput {
            window: self.win,
            contact: ContactId::Touch(finger),
            phase,
            position: Point::new(x, y),
            time,
        });
        self.app.flush();
    }
    /// A finger lands: its position, then the press.
    fn down(&mut self, finger: u32, x: f32, y: f32, time: u32) {
        self.input(finger, ContactPhase::Moved, x, y, time);
        self.input(finger, ContactPhase::Pressed, x, y, time);
    }
    fn to(&mut self, finger: u32, x: f32, y: f32, time: u32) {
        self.input(finger, ContactPhase::Moved, x, y, time);
    }
    fn up(&mut self, finger: u32, x: f32, y: f32, time: u32) {
        self.input(finger, ContactPhase::Released, x, y, time);
    }

    fn scroll(&self) -> Point {
        self.app.component::<Viewport>(self.list).unwrap().scroll
    }
    fn log(&self) -> &Log {
        self.app.resource::<Log>()
    }

    /// Answer every request so far, one frame later on the clock. `false`
    /// if nothing had asked.
    fn frame(&mut self) -> bool {
        let requests = std::mem::take(&mut self.app.resource_mut::<Requests>().0);
        if requests.is_empty() {
            return false;
        }
        let mut clock = self.app.resource_mut::<ScrollClock>();
        clock.now = clock.now.map(|now| now + FRAME);
        drop(clock);
        for w in requests {
            self.app.signal(Frame(w));
        }
        self.app.flush();
        true
    }

    /// Frames until none is asked for; the offset after each.
    fn settle(&mut self) -> Vec<f32> {
        let mut ys = Vec::new();
        for _ in 0..2000 {
            if !self.frame() {
                return ys;
            }
            ys.push(self.scroll().y);
        }
        panic!("never settled");
    }

    /// A 625 px/s upward flick from y 300 to 250, released moving.
    fn fling(&mut self) {
        self.down(1, 100.0, 300.0, 0);
        for k in 1..=10u32 {
            self.to(1, 100.0, 300.0 - 5.0 * k as f32, 8 * k);
        }
        self.up(1, 100.0, 250.0, 80);
    }
}

#[test]
fn drag_follows_the_finger() {
    let mut f = fixture(20, Axis::Vertical);
    f.down(1, 100.0, 300.0, 0);
    f.to(1, 100.0, 289.0, 8);
    assert_eq!(f.scroll(), Point::ZERO, "crossing the slop moves nothing");
    f.to(1, 100.0, 249.0, 16);
    assert_eq!(f.scroll(), Point::new(0.0, 40.0));
}

#[test]
fn tap_on_a_row_still_clicks() {
    let mut f = fixture(20, Axis::Vertical);
    f.down(1, 100.0, 310.0, 0);
    f.to(1, 100.0, 315.0, 8);
    f.up(1, 100.0, 315.0, 16);
    assert!(f.log().clicks.contains(&f.rows[6]));
    assert_eq!(f.scroll(), Point::ZERO);
}

#[test]
fn swipe_on_a_row_does_not_click() {
    let mut f = fixture(20, Axis::Vertical);
    f.down(1, 100.0, 310.0, 0);
    f.to(1, 100.0, 290.0, 8);
    f.to(1, 100.0, 250.0, 16);
    f.up(1, 100.0, 250.0, 200);
    assert!(f.log().cancels.contains(&f.rows[6]));
    assert!(!f.log().clicks.contains(&f.rows[6]));
}

#[test]
fn fling_glides_then_stops_requesting_frames() {
    let mut f = fixture(20, Axis::Vertical);
    f.fling();
    let released = f.scroll().y;
    let ys = f.settle();
    assert!(ys.len() > 10, "{} frames", ys.len());
    assert!(ys[5] > ys[0] && ys[0] >= released, "{ys:?}");
    assert!(ys.windows(2).all(|p| p[1] >= p[0]), "{ys:?}");
    let rest = *ys.last().unwrap();
    assert!(rest > released + 100.0 && rest < 600.0, "{rest}");
    assert_eq!(
        f.app.component::<GrabPresses>(f.list),
        Some(&GrabPresses(false))
    );

    // A full cycle after settling: the frame comes and asks for nothing.
    f.app.signal(Frame(f.win));
    f.app.flush();
    assert!(f.app.resource::<Requests>().0.is_empty());
    assert_eq!(f.scroll().y, rest);
}

#[test]
fn pull_past_top_springs_back_to_zero() {
    let mut f = fixture(20, Axis::Vertical);
    f.down(1, 100.0, 100.0, 0);
    f.to(1, 100.0, 111.0, 8);
    f.to(1, 100.0, 211.0, 16);
    let y = f.scroll().y;
    assert!(y > -100.0 && y < 0.0, "{y}");
    f.up(1, 100.0, 211.0, 200);
    assert!(!f.settle().is_empty());
    assert_eq!(f.scroll().y, 0.0);
}

#[test]
fn touch_on_a_gliding_list_stops_it_and_taps_nothing() {
    let mut f = fixture(20, Axis::Vertical);
    f.fling();
    for _ in 0..3 {
        assert!(f.frame());
    }
    let y = f.scroll().y;
    f.app.resource_mut::<Log>().presses.clear();
    f.down(2, 100.0, 310.0, 200);
    assert_eq!(f.log().presses, vec![f.list], "only the list got the press");
    for _ in 0..5 {
        f.frame();
        assert_eq!(f.scroll().y, y);
    }
    f.up(2, 100.0, 310.0, 400);
    assert!(f.rows.iter().all(|r| !f.log().clicks.contains(r)));
    assert_eq!(f.scroll().y, y);
}

#[test]
fn second_finger_does_not_tap_while_dragging() {
    let mut f = fixture(20, Axis::Vertical);
    f.down(1, 100.0, 300.0, 0);
    f.to(1, 100.0, 280.0, 8);
    let y = f.scroll().y;
    f.down(2, 100.0, 110.0, 10);
    f.to(2, 100.0, 60.0, 12);
    f.up(2, 100.0, 60.0, 14);
    assert!(!f.log().presses.contains(&f.rows[2]));
    assert!(!f.log().clicks.contains(&f.rows[2]));
    assert_eq!(f.scroll().y, y, "the second finger does not drive");
    f.to(1, 100.0, 270.0, 16);
    assert_eq!(f.scroll().y, y + 10.0, "the first still does");
}

#[test]
fn short_content_claims_but_does_not_move() {
    let mut f = fixture(3, Axis::Vertical);
    f.down(1, 100.0, 100.0, 0);
    f.to(1, 100.0, 75.0, 8);
    f.to(1, 100.0, 50.0, 16);
    assert_eq!(f.scroll(), Point::ZERO);
    f.up(1, 100.0, 50.0, 24);
    f.settle();
    assert_eq!(f.scroll(), Point::ZERO);
    assert!(f.log().cancels.contains(&f.rows[2]));
    assert!(!f.log().clicks.contains(&f.rows[2]));
}

#[test]
fn idle_offset_clamps_when_content_shrinks() {
    let mut f = fixture(20, Axis::Vertical);
    f.down(1, 100.0, 390.0, 0);
    f.to(1, 100.0, 379.0, 8);
    f.to(1, 100.0, -221.0, 16);
    f.up(1, 100.0, -221.0, 500);
    assert_eq!(f.scroll().y, 600.0, "at the end, at rest");
    for &row in &f.rows[10..] {
        f.app.remove(row);
    }
    f.app.tick();
    assert_eq!(f.scroll().y, 100.0);
}

#[test]
fn shrinking_mid_glide_settles_in_the_new_range() {
    let mut f = fixture(20, Axis::Vertical);
    f.fling();
    assert!(f.frame());
    for &row in &f.rows[10..] {
        f.app.remove(row);
    }
    f.app.tick();
    f.settle();
    assert_eq!(f.scroll().y, 100.0);
}

#[test]
fn cancel_mid_drag_springs_back() {
    let mut f = fixture(20, Axis::Vertical);
    f.down(1, 100.0, 100.0, 0);
    f.to(1, 100.0, 111.0, 8);
    f.to(1, 100.0, 211.0, 16);
    assert!(f.scroll().y < 0.0);
    f.input(1, ContactPhase::Cancelled, 100.0, 211.0, 24);
    assert!(!f.settle().is_empty());
    assert_eq!(f.scroll().y, 0.0);
}

#[test]
fn horizontal_axis_scrolls_x() {
    let mut f = fixture(20, Axis::Horizontal);
    f.down(1, 100.0, 300.0, 0);
    f.to(1, 100.0, 250.0, 8);
    f.to(1, 100.0, 200.0, 16);
    f.up(1, 100.0, 200.0, 200);
    assert_eq!(f.scroll(), Point::ZERO, "a vertical swipe does nothing");
    f.down(2, 150.0, 200.0, 1000);
    f.to(2, 139.0, 200.0, 1008);
    f.to(2, 99.0, 200.0, 1016);
    assert_eq!(f.scroll(), Point::new(40.0, 0.0));
}

#[test]
fn removed_while_gliding_is_skipped() {
    let mut f = fixture(20, Axis::Vertical);
    f.fling();
    assert!(f.frame());
    assert!(f.frame());
    assert!(f.app.remove(f.list));
    for _ in 0..5 {
        f.frame();
        f.app.signal(Frame(f.win));
        f.app.flush();
    }
    f.app.tick();
}

#[test]
fn resting_finger_does_not_let_a_second_finger_swipe_click() {
    let mut f = fixture(20, Axis::Vertical);
    f.down(1, 100.0, 110.0, 0);
    f.down(2, 100.0, 310.0, 8);
    f.to(2, 100.0, 290.0, 16);
    f.to(2, 100.0, 250.0, 24);
    assert_eq!(
        f.scroll(),
        Point::new(0.0, 40.0),
        "the second finger drives"
    );
    f.up(2, 100.0, 250.0, 200);
    f.up(1, 100.0, 110.0, 210);
    assert!(f.log().cancels.contains(&f.rows[6]));
    assert!(
        f.log().cancels.contains(&f.rows[2]),
        "the resting finger too"
    );
    assert!(f.rows.iter().all(|r| !f.log().clicks.contains(r)));
}

#[test]
fn a_moved_row_does_not_click_on_a_second_fingers_release() {
    let mut f = fixture(20, Axis::Vertical);
    f.down(1, 100.0, 300.0, 0);
    f.down(2, 100.0, 110.0, 4);
    f.to(1, 100.0, 289.0, 8);
    f.to(1, 100.0, 189.0, 16);
    assert_eq!(f.scroll(), Point::new(0.0, 100.0));
    f.up(2, 100.0, 110.0, 20);
    assert!(f.log().cancels.contains(&f.rows[2]));
    assert!(!f.log().clicks.contains(&f.rows[2]));
    f.to(1, 100.0, 179.0, 24);
    assert_eq!(f.scroll(), Point::new(0.0, 110.0), "the first still drives");
}
