//! Layout end to end: the module installed on an `App`, roots marked by
//! hand, styles written, ticks taken, boxes read.

use std::cell::RefCell;

use app::prelude::*;
use geometry::{Insets, Rect, Size};
use layout::prelude::*;

// ── fixtures ────────────────────────────────────────────────────────────

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

// Systems cannot capture, so the tests log through thread-locals. Each
// test runs on its own thread, so logs never mix.
thread_local! {
    /// Every `LayoutDone` seen, in order: its `roots`.
    static DONE: RefCell<Vec<Vec<NodeId>>> = const { RefCell::new(Vec::new()) };
    /// Every `Emitted<OnChanged<ComputedLayout>>` seen: its targets.
    static MOVED: RefCell<Vec<Vec<NodeId>>> = const { RefCell::new(Vec::new()) };
    /// Every `Emitted<OnChanged<LayoutStyle>>` seen: its targets.
    static RESTYLED: RefCell<Vec<Vec<NodeId>>> = const { RefCell::new(Vec::new()) };
}

fn log_done(_: &mut App, d: &LayoutDone) {
    DONE.with(|l| l.borrow_mut().push(d.roots.clone()));
}
fn log_moved(_: &mut App, e: &Emitted<OnChanged<ComputedLayout>>) {
    MOVED.with(|l| l.borrow_mut().push(e.targets.to_vec()));
}
fn log_restyled(_: &mut App, e: &Emitted<OnChanged<LayoutStyle>>) {
    RESTYLED.with(|l| l.borrow_mut().push(e.targets.to_vec()));
}
fn take_done() -> Vec<Vec<NodeId>> {
    DONE.with(|l| std::mem::take(&mut *l.borrow_mut()))
}
fn take_moved() -> Vec<Vec<NodeId>> {
    MOVED.with(|l| std::mem::take(&mut *l.borrow_mut()))
}
fn take_restyled() -> Vec<Vec<NodeId>> {
    RESTYLED.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// An app with the module and the three logging systems.
fn app() -> App {
    let mut app = App::new();
    app.add_module(LayoutModule)
        .system(log_done)
        .system(log_moved)
        .system(log_restyled);
    app
}

/// A root of fixed size, a flex row with children aligned to the start so
/// a leaf keeps the height it asked for.
fn root_style(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::default()
        .size(px(width), px(height))
        .align_items(Align::Start)
}

/// Spawn a marked root under the app root.
fn root(app: &mut App, width: f32, height: f32) -> NodeId {
    app.spawn_with(
        app.root(),
        Leaf,
        (LayoutRoot(true), root_style(width, height)),
    )
    .id()
}

fn child(app: &mut App, parent: NodeId, style: LayoutStyle) -> NodeId {
    app.spawn_with(parent, Leaf, (style,)).id()
}

fn rect(app: &App, id: NodeId) -> Rect {
    app.component::<ComputedLayout>(id).unwrap().rect
}

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::default().size(px(w), px(h))
}

fn sorted(mut v: Vec<NodeId>) -> Vec<NodeId> {
    v.sort_by_key(|id| id.slot());
    v
}

// ── the module and the drain ────────────────────────────────────────────

#[test]
fn a_tick_with_nothing_to_do_signals_an_empty_layout_done() {
    let mut app = app();
    app.tick();
    app.tick();
    assert_eq!(take_done(), vec![Vec::<NodeId>::new(), Vec::new()]);
    assert!(take_moved().is_empty());
}

#[test]
fn on_changed_layout_style_never_fires_while_the_module_is_installed() {
    let mut app = app();
    let leaf = app.spawn(app.root(), Leaf).id();
    app.component_mut::<LayoutStyle>(leaf).unwrap().width = px(10.0);
    app.tick();
    assert!(
        take_restyled().is_empty(),
        "layout took the record before the drain"
    );
}

#[test]
fn measure_helpers() {
    let c = Constraints {
        known_width: Some(200.0),
        known_height: None,
        available_width: Available::Definite(200.0),
        available_height: Available::MaxContent,
    };
    assert!(!Measure::none().is_set());
    assert_eq!(Measure::none().measure(c), Size::ZERO);
    assert_eq!(Measure::default().measure(c), Size::ZERO);
    let f = Measure::fixed(Size::new(30.0, 20.0));
    assert!(f.is_set());
    assert_eq!(f.measure(c), Size::new(30.0, 20.0));
    let w = Measure::with(|c| Size::new(c.known_width.unwrap_or(0.0), 5.0));
    assert_eq!(w.measure(c), Size::new(200.0, 5.0));
    assert_eq!(format!("{:?}", Measure::none()), "Measure(unset)");
    assert_eq!(format!("{w:?}"), "Measure(set)");
}

#[test]
fn computed_layout_content_is_inside_padding_and_border() {
    let l = ComputedLayout {
        rect: Rect::new(10.0, 20.0, 100.0, 50.0),
        padding: Insets::new(1.0, 2.0, 3.0, 4.0),
        border: Insets::all(1.0),
    };
    assert_eq!(l.content(), Rect::new(15.0, 22.0, 92.0, 44.0));
    let tiny = ComputedLayout {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        padding: Insets::all(8.0),
        border: Insets::all(0.0),
    };
    assert_eq!(tiny.content(), Rect::new(8.0, 8.0, 0.0, 0.0));
    assert!(!LayoutDone { roots: vec![] }.recomputed());
    assert!(
        LayoutDone {
            roots: vec![tiny_id()]
        }
        .recomputed()
    );
}

/// Any id will do for the signal's `recomputed`; the app root is always live.
fn tiny_id() -> NodeId {
    App::new().root()
}

// ── dirtiness ───────────────────────────────────────────────────────────

#[test]
fn a_style_write_under_a_root_dirties_that_root_once() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r, fixed(50.0, 40.0));
    let b = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    assert_eq!(take_done(), vec![vec![r]], "the spawn dirtied the root");

    app.component_mut::<LayoutStyle>(a).unwrap().width = px(80.0);
    app.component_mut::<LayoutStyle>(b).unwrap().width = px(80.0);
    app.tick();
    assert_eq!(take_done(), vec![vec![r]], "two writes, one root, once");

    app.tick();
    assert_eq!(take_done(), vec![vec![]], "nothing pending");
}

#[test]
fn a_measure_write_dirties_the_root() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r, LayoutStyle::default());
    app.tick();
    take_done();
    *app.component_mut::<Measure>(a).unwrap() = Measure::fixed(Size::new(30.0, 20.0));
    app.tick();
    assert_eq!(take_done(), vec![vec![r]]);
}

#[test]
fn a_write_outside_every_root_dirties_nothing() {
    let mut app = app();
    let stray = app.spawn(app.root(), Leaf).id();
    app.tick();
    take_done();
    app.component_mut::<LayoutStyle>(stray).unwrap().width = px(10.0);
    app.tick();
    assert_eq!(take_done(), vec![vec![]]);
}

#[test]
fn two_roots_are_dirtied_independently() {
    let mut app = app();
    let r1 = root(&mut app, 300.0, 100.0);
    let r2 = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r1, fixed(50.0, 40.0));
    let _b = child(&mut app, r2, fixed(50.0, 40.0));
    app.tick();
    assert_eq!(
        take_done(),
        vec![vec![r1, r2]],
        "both marked, in first-write order"
    );

    app.component_mut::<LayoutStyle>(a).unwrap().width = px(80.0);
    app.tick();
    assert_eq!(take_done(), vec![vec![r1]]);
}

#[test]
fn spawn_and_remove_under_a_root_dirty_it() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    take_done();

    let b = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    assert_eq!(take_done(), vec![vec![r]], "spawn");

    assert!(app.remove(a));
    app.tick();
    assert_eq!(take_done(), vec![vec![r]], "remove");

    assert!(app.remove(b));
    app.tick();
    assert_eq!(take_done(), vec![vec![r]], "remove the last child");
}

#[test]
fn removing_a_whole_root_dirties_nothing_and_does_not_panic() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let _a = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    take_done();
    assert!(app.remove(r));
    app.tick();
    assert_eq!(
        take_done(),
        vec![vec![]],
        "the root is gone, nothing to recompute"
    );
}

#[test]
fn marking_a_node_as_a_root_makes_it_its_own_dirty_root() {
    let mut app = app();
    let later = app
        .spawn_with(app.root(), Leaf, (root_style(200.0, 100.0),))
        .id();
    let _inner = child(&mut app, later, fixed(50.0, 40.0));
    app.tick();
    assert_eq!(take_done(), vec![vec![]], "not a root yet");
    app.component_mut::<LayoutRoot>(later).unwrap().0 = true;
    app.tick();
    assert_eq!(take_done(), vec![vec![later]]);
}

// ── boxes ───────────────────────────────────────────────────────────────

#[test]
fn a_row_of_fixed_children_lands_side_by_side() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r, fixed(50.0, 40.0));
    let b = child(&mut app, r, fixed(70.0, 40.0));
    app.tick();
    assert_eq!(rect(&app, r), Rect::new(0.0, 0.0, 300.0, 100.0));
    assert_eq!(rect(&app, a), Rect::new(0.0, 0.0, 50.0, 40.0));
    assert_eq!(rect(&app, b), Rect::new(50.0, 0.0, 70.0, 40.0));
}

#[test]
fn a_column_stacks_children() {
    let mut app = app();
    let r = app
        .spawn_with(
            app.root(),
            Leaf,
            (LayoutRoot(true), root_style(300.0, 100.0).column()),
        )
        .id();
    let a = child(&mut app, r, fixed(50.0, 40.0));
    let b = child(&mut app, r, fixed(70.0, 30.0));
    app.tick();
    assert_eq!(rect(&app, a), Rect::new(0.0, 0.0, 50.0, 40.0));
    assert_eq!(rect(&app, b), Rect::new(0.0, 40.0, 70.0, 30.0));
}

#[test]
fn percent_and_fill_resolve_against_the_parent() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let half = child(
        &mut app,
        r,
        LayoutStyle::default().size(percent(50.0), percent(50.0)),
    );
    let r2 = root(&mut app, 200.0, 80.0);
    let all = child(&mut app, r2, LayoutStyle::default().fill());
    app.tick();
    assert_eq!(rect(&app, half), Rect::new(0.0, 0.0, 150.0, 50.0));
    assert_eq!(rect(&app, all), Rect::new(0.0, 0.0, 200.0, 80.0));
}

#[test]
fn padding_and_border_inset_the_content_and_offset_the_child() {
    let mut app = app();
    let style = root_style(300.0, 100.0)
        .padding_all(px(10.0))
        .border(Insets::all(px(2.0)));
    let r = app
        .spawn_with(app.root(), Leaf, (LayoutRoot(true), style))
        .id();
    let inner = child(&mut app, r, LayoutStyle::default().fill());
    app.tick();
    let root_layout = *app.component::<ComputedLayout>(r).unwrap();
    assert_eq!(root_layout.padding, Insets::all(10.0));
    assert_eq!(root_layout.border, Insets::all(2.0));
    assert_eq!(root_layout.content(), Rect::new(12.0, 12.0, 276.0, 76.0));
    assert_eq!(rect(&app, inner), Rect::new(12.0, 12.0, 276.0, 76.0));
}

#[test]
fn nested_boxes_are_in_the_roots_coordinates() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let outer = child(&mut app, r, fixed(200.0, 80.0).padding_all(px(5.0)));
    let inner = child(&mut app, outer, fixed(20.0, 10.0));
    let sibling = child(&mut app, r, fixed(30.0, 10.0));
    app.tick();
    assert_eq!(rect(&app, outer), Rect::new(0.0, 0.0, 200.0, 80.0));
    assert_eq!(rect(&app, inner), Rect::new(5.0, 5.0, 20.0, 10.0));
    assert_eq!(rect(&app, sibling), Rect::new(200.0, 0.0, 30.0, 10.0));
}

#[test]
fn flex_grow_splits_the_free_space() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let a = child(
        &mut app,
        r,
        LayoutStyle::default().height(px(40.0)).grow(1.0),
    );
    let b = child(
        &mut app,
        r,
        LayoutStyle::default().height(px(40.0)).grow(2.0),
    );
    app.tick();
    assert_eq!(rect(&app, a), Rect::new(0.0, 0.0, 100.0, 40.0));
    assert_eq!(rect(&app, b), Rect::new(100.0, 0.0, 200.0, 40.0));
}

#[test]
fn justify_places_children_as_css_would() {
    let mut app = app();
    let centered = app
        .spawn_with(
            app.root(),
            Leaf,
            (
                LayoutRoot(true),
                root_style(300.0, 100.0).justify(Justify::Center),
            ),
        )
        .id();
    let c1 = child(&mut app, centered, fixed(50.0, 40.0));
    let c2 = child(&mut app, centered, fixed(50.0, 40.0));
    let between = app
        .spawn_with(
            app.root(),
            Leaf,
            (
                LayoutRoot(true),
                root_style(300.0, 100.0).justify(Justify::SpaceBetween),
            ),
        )
        .id();
    let b1 = child(&mut app, between, fixed(50.0, 40.0));
    let b2 = child(&mut app, between, fixed(50.0, 40.0));
    app.tick();
    assert_eq!(rect(&app, c1).x(), 100.0);
    assert_eq!(rect(&app, c2).x(), 150.0);
    assert_eq!(rect(&app, b1).x(), 0.0);
    assert_eq!(rect(&app, b2).x(), 250.0);
}

#[test]
fn a_fixed_measure_sizes_a_leaf() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let leaf = app
        .spawn_with(r, Leaf, (Measure::fixed(Size::new(30.0, 20.0)),))
        .id();
    app.tick();
    assert_eq!(rect(&app, leaf), Rect::new(0.0, 0.0, 30.0, 20.0));
}

#[test]
fn a_measure_closure_sees_the_offered_width() {
    let mut app = app();
    let r = app
        .spawn_with(
            app.root(),
            Leaf,
            (
                LayoutRoot(true),
                LayoutStyle::default().size(px(200.0), px(100.0)).column(),
            ),
        )
        .id();
    // Stretched across a 200 wide column; answers "at width w I am 1000/w tall".
    let measure = Measure::with(|c| {
        let w = match (c.known_width, c.available_width) {
            (Some(w), _) => w,
            (None, Available::Definite(w)) => w,
            _ => 100.0,
        };
        Size::new(w, 1000.0 / w)
    });
    let text = app.spawn_with(r, Leaf, (measure,)).id();
    app.tick();
    assert_eq!(rect(&app, text), Rect::new(0.0, 0.0, 200.0, 5.0));
}

#[test]
fn a_write_between_ticks_moves_the_box_next_tick_and_then_rests() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r, fixed(50.0, 40.0));
    let b = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    take_done();
    take_moved();

    app.component_mut::<LayoutStyle>(a).unwrap().width = px(80.0);
    app.tick();
    assert_eq!(rect(&app, a), Rect::new(0.0, 0.0, 80.0, 40.0));
    assert_eq!(rect(&app, b), Rect::new(80.0, 0.0, 50.0, 40.0));
    assert_eq!(take_done(), vec![vec![r]]);
    let moved = take_moved();
    assert_eq!(moved.len(), 1);
    assert_eq!(
        sorted(moved[0].clone()),
        sorted(vec![a, b]),
        "the root did not move"
    );

    app.tick();
    assert_eq!(take_done(), vec![vec![]]);
    assert!(take_moved().is_empty());
}

#[test]
fn on_changed_computed_layout_names_only_the_boxes_that_changed() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r, fixed(50.0, 40.0));
    let b = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    take_moved();
    app.component_mut::<LayoutStyle>(b).unwrap().height = px(60.0);
    app.tick();
    let moved = take_moved();
    assert_eq!(moved, vec![vec![b]], "a and the root are where they were");
    let _ = a;
}

thread_local! {
    static WIDEN: std::cell::Cell<Option<NodeId>> = const { std::cell::Cell::new(None) };
}

fn widen_on_tick(app: &mut App, _: &Tick) {
    if let Some(id) = WIDEN.with(|w| w.take()) {
        app.component_mut::<LayoutStyle>(id).unwrap().width = px(80.0);
    }
}

#[test]
fn a_style_written_by_a_tick_system_is_laid_out_in_the_same_tick() {
    let mut app = app();
    app.system(widen_on_tick);
    let r = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    assert_eq!(rect(&app, a).width(), 50.0);
    WIDEN.with(|w| w.set(Some(a)));
    app.tick();
    assert_eq!(
        rect(&app, a).width(),
        80.0,
        "written during Tick, laid out at PostTick"
    );
}

#[test]
fn spawn_and_remove_move_the_siblings() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    let b = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    assert_eq!(rect(&app, b).x(), 50.0);
    assert!(app.remove(a));
    app.tick();
    assert_eq!(rect(&app, b).x(), 0.0);
}

#[test]
fn two_roots_do_not_disturb_each_other() {
    let mut app = app();
    let r1 = root(&mut app, 300.0, 100.0);
    let r2 = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r1, fixed(50.0, 40.0));
    let b = child(&mut app, r2, fixed(50.0, 40.0));
    app.tick();
    take_done();
    assert_eq!(
        rect(&app, r2),
        Rect::new(0.0, 0.0, 300.0, 100.0),
        "each root at its own origin"
    );
    app.component_mut::<LayoutStyle>(a).unwrap().width = px(80.0);
    app.tick();
    assert_eq!(take_done(), vec![vec![r1]]);
    assert_eq!(rect(&app, b), Rect::new(0.0, 0.0, 50.0, 40.0));
}

#[test]
fn a_node_outside_every_root_keeps_a_zero_computed_layout() {
    let mut app = app();
    let stray = app.spawn_with(app.root(), Leaf, (fixed(50.0, 40.0),)).id();
    app.tick();
    assert_eq!(rect(&app, stray), Rect::ZERO);
    app.component_mut::<LayoutStyle>(stray).unwrap().width = px(10.0);
    app.tick();
    assert_eq!(rect(&app, stray), Rect::ZERO);
    assert_eq!(rect(&app, app.root()), Rect::ZERO);
}

#[test]
fn hidden_takes_no_space_and_gets_a_zero_size_box() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r, fixed(50.0, 40.0));
    let h = child(&mut app, r, fixed(50.0, 40.0).hidden());
    let under_h = child(&mut app, h, fixed(10.0, 10.0));
    let b = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    assert_eq!(rect(&app, a), Rect::new(0.0, 0.0, 50.0, 40.0));
    assert_eq!(rect(&app, h), Rect::ZERO);
    assert_eq!(rect(&app, under_h), Rect::ZERO);
    assert_eq!(rect(&app, b), Rect::new(50.0, 0.0, 50.0, 40.0));
}

#[test]
fn marking_a_root_later_lays_its_subtree_out() {
    let mut app = app();
    let later = app
        .spawn_with(app.root(), Leaf, (root_style(200.0, 100.0),))
        .id();
    let inner = child(&mut app, later, fixed(50.0, 40.0));
    app.tick();
    assert_eq!(rect(&app, inner), Rect::ZERO);
    app.component_mut::<LayoutRoot>(later).unwrap().0 = true;
    app.tick();
    assert_eq!(rect(&app, later), Rect::new(0.0, 0.0, 200.0, 100.0));
    assert_eq!(rect(&app, inner), Rect::new(0.0, 0.0, 50.0, 40.0));
}

#[test]
fn absolute_children_are_placed_by_inset_outside_the_flow() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let a = child(&mut app, r, fixed(50.0, 40.0));
    let abs = child(
        &mut app,
        r,
        fixed(30.0, 30.0)
            .absolute()
            .inset(Insets::new(px(10.0), auto(), auto(), px(20.0))),
    );
    let b = child(&mut app, r, fixed(50.0, 40.0));
    app.tick();
    assert_eq!(rect(&app, abs), Rect::new(20.0, 10.0, 30.0, 30.0));
    assert_eq!(rect(&app, a).x(), 0.0);
    assert_eq!(rect(&app, b).x(), 50.0, "the absolute child took no space");
}

#[test]
fn block_layout_stacks_and_stretches_width() {
    let mut app = app();
    let r = app
        .spawn_with(
            app.root(),
            Leaf,
            (LayoutRoot(true), root_style(300.0, 100.0).block()),
        )
        .id();
    let a = child(&mut app, r, LayoutStyle::default().height(px(20.0)));
    let b = child(&mut app, r, LayoutStyle::default().height(px(30.0)));
    app.tick();
    assert_eq!(rect(&app, a), Rect::new(0.0, 0.0, 300.0, 20.0));
    assert_eq!(rect(&app, b), Rect::new(0.0, 20.0, 300.0, 30.0));
}

/// Three thirds of 100 do not land on whole pixels, so the rounding walk
/// has to work from the unrounded cumulative offset: every child must
/// start where its previous sibling ended, at every level.
#[test]
fn nested_thirds_tile_their_parent_exactly() {
    let mut app = app();
    let r = app
        .spawn_with(
            app.root(),
            Leaf,
            (
                LayoutRoot(true),
                LayoutStyle::default().size(px(100.0), px(30.0)),
            ),
        )
        .id();
    let mut rows = Vec::new();
    for _ in 0..3 {
        let row = child(&mut app, r, LayoutStyle::default().grow(1.0));
        let leaves: Vec<NodeId> = (0..3)
            .map(|_| child(&mut app, row, LayoutStyle::default().grow(1.0)))
            .collect();
        rows.push((row, leaves));
    }
    app.tick();

    let check = |parent: NodeId, children: &[NodeId]| {
        let p = rect(&app, parent);
        let first = rect(&app, children[0]);
        assert_eq!(first.x(), p.x(), "the first child starts at the parent");
        for pair in children.windows(2) {
            let left = rect(&app, pair[0]);
            let right = rect(&app, pair[1]);
            assert_eq!(
                right.x(),
                left.right(),
                "a child starts where its sibling ended"
            );
        }
        let last = rect(&app, children[children.len() - 1]);
        assert_eq!(last.right(), p.right(), "the last child ends at the parent");
        for &c in children {
            assert!(rect(&app, c).width() > 0.0, "no child collapsed to zero");
        }
    };

    let row_ids: Vec<NodeId> = rows.iter().map(|(row, _)| *row).collect();
    check(r, &row_ids);
    for (row, leaves) in &rows {
        check(*row, leaves);
    }
}

/// Wrapping puts the overflowing child on a second line, and
/// `align_content` decides where the lines sit on the cross axis.
#[test]
fn wrap_starts_a_second_line_where_align_content_puts_it() {
    let mut app = app();
    let packed = app
        .spawn_with(
            app.root(),
            Leaf,
            (LayoutRoot(true), root_style(100.0, 100.0).wrap()),
        )
        .id();
    let p1 = child(&mut app, packed, fixed(60.0, 20.0));
    let p2 = child(&mut app, packed, fixed(60.0, 20.0));

    let spread = app
        .spawn_with(
            app.root(),
            Leaf,
            (
                LayoutRoot(true),
                root_style(100.0, 100.0)
                    .wrap()
                    .align_content(Justify::SpaceBetween),
            ),
        )
        .id();
    let s1 = child(&mut app, spread, fixed(60.0, 20.0));
    let s2 = child(&mut app, spread, fixed(60.0, 20.0));
    app.tick();

    // Two lines of one child each: 60 + 60 does not fit in 100.
    assert_eq!(rect(&app, p1), Rect::new(0.0, 0.0, 60.0, 20.0));
    assert_eq!(
        rect(&app, p2),
        Rect::new(0.0, 20.0, 60.0, 20.0),
        "lines packed at the start, the second under the first"
    );
    assert_eq!(rect(&app, s1), Rect::new(0.0, 0.0, 60.0, 20.0));
    assert_eq!(
        rect(&app, s2),
        Rect::new(0.0, 80.0, 60.0, 20.0),
        "space-between pushes the last line to the bottom"
    );
}

/// `column_gap` is the gap between columns, so it is the one a row sees;
/// `row_gap` is the gap between rows, the one a column sees.
#[test]
fn gap_lands_on_the_axis_the_container_runs_along() {
    let mut app = app();
    let row = app
        .spawn_with(
            app.root(),
            Leaf,
            (
                LayoutRoot(true),
                root_style(300.0, 100.0).column_gap(px(10.0)),
            ),
        )
        .id();
    let r1 = child(&mut app, row, fixed(20.0, 20.0));
    let r2 = child(&mut app, row, fixed(20.0, 20.0));

    let row_cross = app
        .spawn_with(
            app.root(),
            Leaf,
            (LayoutRoot(true), root_style(300.0, 100.0).row_gap(px(10.0))),
        )
        .id();
    let x1 = child(&mut app, row_cross, fixed(20.0, 20.0));
    let x2 = child(&mut app, row_cross, fixed(20.0, 20.0));

    let column = app
        .spawn_with(
            app.root(),
            Leaf,
            (
                LayoutRoot(true),
                root_style(300.0, 100.0).column().row_gap(px(10.0)),
            ),
        )
        .id();
    let c1 = child(&mut app, column, fixed(20.0, 20.0));
    let c2 = child(&mut app, column, fixed(20.0, 20.0));
    app.tick();

    assert_eq!(rect(&app, r1), Rect::new(0.0, 0.0, 20.0, 20.0));
    assert_eq!(
        rect(&app, r2),
        Rect::new(30.0, 0.0, 20.0, 20.0),
        "a row is spaced by the column gap"
    );
    assert_eq!(rect(&app, x1).x(), 0.0);
    assert_eq!(
        rect(&app, x2).x(),
        20.0,
        "the row gap does not space a single row's children"
    );
    assert_eq!(rect(&app, c1), Rect::new(0.0, 0.0, 20.0, 20.0));
    assert_eq!(
        rect(&app, c2),
        Rect::new(0.0, 30.0, 20.0, 20.0),
        "a column is spaced by the row gap"
    );
}

/// The four clamps, each on its own axis.
#[test]
fn min_and_max_sizes_clamp_each_axis() {
    let mut app = app();
    let r = root(&mut app, 300.0, 100.0);
    let wide = child(&mut app, r, fixed(10.0, 20.0).min_width(px(30.0)));
    let narrow = child(&mut app, r, fixed(80.0, 20.0).max_width(px(40.0)));
    let tall = child(&mut app, r, fixed(20.0, 5.0).min_height(px(25.0)));
    let short = child(&mut app, r, fixed(20.0, 90.0).max_height(px(30.0)));
    app.tick();

    assert_eq!(rect(&app, wide).width(), 30.0, "min_width raised it");
    assert_eq!(rect(&app, narrow).width(), 40.0, "max_width capped it");
    assert_eq!(rect(&app, tall).height(), 25.0, "min_height raised it");
    assert_eq!(rect(&app, short).height(), 30.0, "max_height capped it");
    // The heights are untouched by the width clamps, and vice versa.
    assert_eq!(rect(&app, wide).height(), 20.0);
    assert_eq!(rect(&app, narrow).height(), 20.0);
    assert_eq!(rect(&app, tall).width(), 20.0);
    assert_eq!(rect(&app, short).width(), 20.0);
    // Side by side in the row, at the clamped widths.
    assert_eq!(rect(&app, wide).x(), 0.0);
    assert_eq!(rect(&app, narrow).x(), 30.0);
    assert_eq!(rect(&app, tall).x(), 70.0);
    assert_eq!(rect(&app, short).x(), 90.0);
}

/// Every side of `padding` and `border` is its own value, and the content
/// box is inset by the sum of the two on each side.
#[test]
fn asymmetric_padding_and_border_inset_each_side_on_its_own() {
    let mut app = app();
    let style = root_style(200.0, 100.0)
        .padding(Insets::new(px(4.0), px(8.0), px(12.0), px(16.0)))
        .border(Insets::new(px(1.0), px(2.0), px(3.0), px(4.0)));
    let r = app
        .spawn_with(app.root(), Leaf, (LayoutRoot(true), style))
        .id();
    let inner = child(&mut app, r, LayoutStyle::default().fill());
    app.tick();

    let l = *app.component::<ComputedLayout>(r).unwrap();
    assert_eq!(l.padding, Insets::new(4.0, 8.0, 12.0, 16.0));
    assert_eq!(l.border, Insets::new(1.0, 2.0, 3.0, 4.0));
    // left 4 + 16, top 1 + 4, and 200 - 20 - 10 by 100 - 5 - 15.
    let content = Rect::new(20.0, 5.0, 170.0, 80.0);
    assert_eq!(l.content(), content);
    assert_eq!(
        rect(&app, inner),
        content,
        "the child fills the content box"
    );
}

// ── StyleContext through a real dispatch ────────────────────────────────

struct Poke;
impl Event for Poke {}

struct Editable;
struct EditableBuilder(Box<dyn FnMut(&mut Context<'_, Editable>) -> bool>);
impl Build for EditableBuilder {
    type Widget = Editable;
}
impl Widget for Editable {
    type Builder = EditableBuilder;
    fn build(b: EditableBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let mut action = b.0;
        s.on::<Poke>(me, move |ctx, _| {
            let _ = action(ctx);
        });
        Editable
    }
}

fn editable(
    app: &mut App,
    parent: NodeId,
    style: LayoutStyle,
    action: impl FnMut(&mut Context<'_, Editable>) -> bool + 'static,
) -> NodeId {
    app.spawn_with(parent, EditableBuilder(Box::new(action)), (style,))
        .id()
}

fn poke(app: &mut App, id: NodeId) {
    app.emit(Poke, id);
    app.flush();
}

#[test]
fn set_width_moves_the_box_once_and_a_repeat_call_writes_nothing() {
    let mut app = app();
    let root_id = root(&mut app, 100.0, 100.0);
    let leaf = editable(&mut app, root_id, LayoutStyle::default(), |ctx| {
        ctx.set_width(px(30.0))
    });

    app.tick();
    take_moved();
    take_restyled();

    poke(&mut app, leaf);
    app.tick();
    assert_eq!(rect(&app, leaf).width(), 30.0);
    assert_eq!(take_moved(), vec![vec![leaf]], "the box moved");
    assert!(
        take_restyled().is_empty(),
        "OnChanged<LayoutStyle> never fires while LayoutModule is installed"
    );

    poke(&mut app, leaf);
    app.tick();
    assert!(
        take_moved().is_empty(),
        "the same width again writes nothing, so nothing to lay out"
    );
}

#[test]
fn viewport_does_not_change_layout() {
    let mut app = app();
    let r = root(&mut app, 100.0, 100.0);
    let a = child(&mut app, r, fixed(40.0, 40.0));
    let b = child(&mut app, a, fixed(10.0, 10.0));
    app.tick();
    let before: Vec<_> = [r, a, b]
        .iter()
        .map(|&id| *app.component::<ComputedLayout>(id).unwrap())
        .collect();
    take_done();
    take_moved();
    *app.component_mut::<Viewport>(a).unwrap() = Viewport {
        clip: true,
        scroll: geometry::Point::new(3.0, 7.0),
    };
    app.tick();
    assert_eq!(
        take_done(),
        vec![Vec::<NodeId>::new()],
        "no root recomputed"
    );
    assert!(take_moved().is_empty());
    let after: Vec<_> = [r, a, b]
        .iter()
        .map(|&id| *app.component::<ComputedLayout>(id).unwrap())
        .collect();
    assert_eq!(before, after);
}
