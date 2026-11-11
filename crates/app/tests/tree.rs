use app::prelude::*;

// ── fixtures ────────────────────────────────────────────────────────────

/// A leaf holding a string.
struct Label(String);
struct LabelBuilder(&'static str);
impl Build for LabelBuilder {
    type Widget = Label;
}
impl Widget for Label {
    type Builder = LabelBuilder;
    fn build(b: LabelBuilder, _me: Handle<Self>, _s: &mut Spawner<'_, Self>) -> Self {
        Label(b.0.to_string())
    }
}

/// Builds two `Label`s under itself, so one spawn makes three nodes.
struct Pair {
    left: Handle<Label>,
    right: Handle<Label>,
}
struct PairBuilder;
impl Build for PairBuilder {
    type Widget = Pair;
}
impl Widget for Pair {
    type Builder = PairBuilder;
    fn build(_b: PairBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let left = s.spawn(me, LabelBuilder("left"));
        let right = s.spawn(me, LabelBuilder("right"));
        Pair { left, right }
    }
}

/// Builds a chain within its own build: a `list` child, two labels under
/// it, and a further child under the second of those labels — spawning
/// under handles ("me", then "list", then "b") returned earlier in the
/// same build.
struct Chain;
struct ChainBuilder;
impl Build for ChainBuilder {
    type Widget = Chain;
}
impl Widget for Chain {
    type Builder = ChainBuilder;
    fn build(_b: ChainBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let list = s.spawn(me, LabelBuilder("list"));
        s.spawn(list, LabelBuilder("item-a"));
        let b = s.spawn(list, LabelBuilder("item-b"));
        s.spawn(b, LabelBuilder("grandchild"));
        Chain
    }
}

/// Never spawned by any test.
struct Ghost;
impl Build for Ghost {
    type Widget = Ghost;
}
impl Widget for Ghost {
    type Builder = Ghost;
    fn build(b: Ghost, _me: Handle<Self>, _s: &mut Spawner<'_, Self>) -> Self {
        b
    }
}

/// Records whether the spawner's `me` is the handle `build` was given.
struct Echo(bool);
struct EchoBuilder;
impl Build for EchoBuilder {
    type Widget = Echo;
}
impl Widget for Echo {
    type Builder = EchoBuilder;
    fn build(_b: EchoBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        Echo(s.me() == me)
    }
}

// ── root ────────────────────────────────────────────────────────────────

#[test]
fn new_app_has_a_live_root_that_is_its_own_parent() {
    let app = App::new();
    let root = app.root();
    assert!(app.is_live(root));
    assert_eq!(app.parent(root), Some(root));
    assert_eq!(app.children(root), Some(&[][..]));
    assert_eq!(App::default().root(), root);
}

// ── spawn ───────────────────────────────────────────────────────────────

#[test]
fn spawn_appends_children_in_call_order() {
    let mut app = App::new();
    let root = app.root();
    let a = app.spawn(root, LabelBuilder("a"));
    let b = app.spawn(root, LabelBuilder("b"));
    let c = app.spawn(a, LabelBuilder("c"));
    assert_ne!(a.id(), b.id());
    assert_eq!(app.children(root), Some(&[a.id(), b.id()][..]));
    assert_eq!(app.children(a), Some(&[c.id()][..]));
    assert_eq!(app.children(c), Some(&[][..]));
    assert_eq!(app.parent(a), Some(root));
    assert_eq!(app.parent(c), Some(a.id()));
    assert!(app.is_live(a) && app.is_live(b) && app.is_live(c));
}

#[test]
fn nested_build_attaches_grandchildren_under_the_right_parent() {
    let mut app = App::new();
    let pair = app.spawn(app.root(), PairBuilder);
    assert_eq!(app.children(app.root()), Some(&[pair.id()][..]));
    let kids = app.children(pair).unwrap().to_vec();
    assert_eq!(kids.len(), 2);
    assert_eq!(app.parent(kids[0]), Some(pair.id()));
    assert_eq!(app.parent(kids[1]), Some(pair.id()));
}

#[test]
fn a_build_can_spawn_under_a_handle_it_created_earlier_in_the_same_build() {
    let mut app = App::new();
    let chain = app.spawn(app.root(), ChainBuilder);
    let list = app.children(chain).unwrap().to_vec();
    assert_eq!(list.len(), 1, "one child spawned under `me`");
    let list = list[0];
    assert_eq!(app.parent(list), Some(chain.id()));

    let list_kids = app.children(list).unwrap().to_vec();
    assert_eq!(list_kids.len(), 2, "two labels spawned under `list`");
    let b = list_kids[1];
    assert_eq!(app.parent(b), Some(list));

    let b_kids = app.children(b).unwrap().to_vec();
    assert_eq!(b_kids.len(), 1, "one child spawned under `b`");
    assert_eq!(app.parent(b_kids[0]), Some(b));
}

#[test]
fn every_spawn_gets_a_distinct_id() {
    let mut app = App::new();
    let root = app.root();
    let mut seen = std::collections::HashSet::new();
    seen.insert(root);
    for _ in 0..10 {
        assert!(seen.insert(app.spawn(root, LabelBuilder("n")).id()));
    }
    assert_eq!(app.children(root).unwrap().len(), 10);
}

// ── widgets ─────────────────────────────────────────────────────────────

#[test]
fn widget_lookup_by_handle_and_by_id() {
    let mut app = App::new();
    let a = app.spawn(app.root(), LabelBuilder("a"));
    assert_eq!(app.widget::<Label>(a).map(|l| l.0.as_str()), Some("a"));
    app.widget_mut::<Label>(a).unwrap().0.push('!');
    assert_eq!(
        app.widget::<Label>(a.id()).map(|l| l.0.as_str()),
        Some("a!")
    );
}

#[test]
fn widget_of_another_type_is_none() {
    let mut app = App::new();
    let a = app.spawn(app.root(), LabelBuilder("a"));
    assert!(app.widget::<Pair>(a).is_none());
    assert!(app.widget_mut::<Pair>(a).is_none());
    assert!(app.widget::<Ghost>(a).is_none(), "type with no column");
}

#[test]
fn a_build_can_keep_handles_to_its_children() {
    let mut app = App::new();
    let pair = app.spawn(app.root(), PairBuilder);
    let (left, right) = {
        let p = app.widget::<Pair>(pair).unwrap();
        (p.left, p.right)
    };
    assert_eq!(app.widget::<Label>(left).unwrap().0, "left");
    assert_eq!(app.widget::<Label>(right).unwrap().0, "right");
    assert_eq!(app.children(pair), Some(&[left.id(), right.id()][..]));
}

#[test]
fn widgets_by_type_lists_every_live_one_in_spawn_order() {
    let mut app = App::new();
    let root = app.root();
    let a = app.spawn(root, LabelBuilder("a"));
    let _pair = app.spawn(root, PairBuilder);
    let b = app.spawn(root, LabelBuilder("b"));

    let labels: Vec<(NodeId, String)> = app
        .widgets::<Label>()
        .map(|(id, l)| (id, l.0.clone()))
        .collect();
    let names: Vec<&str> = labels.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(
        names,
        ["a", "left", "right", "b"],
        "fresh slots come in spawn order"
    );
    assert_eq!(labels[0].0, a.id());
    assert_eq!(labels[3].0, b.id());

    for (_, l) in app.widgets::<Label>() {
        l.0.make_ascii_uppercase();
    }
    assert_eq!(app.widget::<Label>(a).unwrap().0, "A");
    assert_eq!(app.widgets::<Pair>().count(), 1);
    assert_eq!(app.widgets::<Ghost>().count(), 0, "never spawned");
}

#[test]
fn a_slot_reused_by_another_widget_type_does_not_resurrect_the_old_widget() {
    let mut app = App::new();
    let root = app.root();
    let label = app.spawn(root, LabelBuilder("gone"));
    assert!(app.remove(label));

    // One freed slot in the FIFO, so this spawn — of a different widget
    // type — reuses it (see the unit test confirming FIFO reuse in
    // `slots.rs`).
    let _pair = app.spawn(root, PairBuilder);

    assert!(!app.is_live(label));
    assert!(app.widget::<Label>(label).is_none());
    assert!(
        app.widgets::<Label>().all(|(id, _)| id != label.id()),
        "the reused slot must not be listed as a live Label"
    );
}

// ── remove ──────────────────────────────────────────────────────────────

#[test]
fn remove_frees_the_whole_subtree() {
    let mut app = App::new();
    let root = app.root();
    let before = app.spawn(root, LabelBuilder("before"));
    let pair = app.spawn(root, PairBuilder);
    let after = app.spawn(root, LabelBuilder("after"));
    let kids = app.children(pair).unwrap().to_vec();

    assert!(app.remove(pair));

    assert!(!app.is_live(pair));
    assert!(app.widget::<Pair>(pair).is_none());
    assert_eq!(app.children(pair), None);
    for kid in &kids {
        assert!(!app.is_live(*kid));
        assert!(app.widget::<Label>(*kid).is_none());
        assert_eq!(app.parent(*kid), None);
    }
    assert_eq!(app.children(root), Some(&[before.id(), after.id()][..]));
    assert_eq!(app.widgets::<Label>().count(), 2);
    assert_eq!(app.widgets::<Pair>().count(), 0);
    assert!(!app.remove(pair), "second removal is a no-op");
    assert!(app.is_live(before) && app.is_live(after));
}

#[test]
fn a_removed_id_stays_stale_after_its_slot_is_reused() {
    let mut app = App::new();
    let root = app.root();
    let old = app.spawn(root, LabelBuilder("old"));
    assert!(app.remove(old));
    // One freed slot in the FIFO, so this spawn reuses it.
    let new = app.spawn(root, LabelBuilder("new"));
    assert_ne!(new.id(), old.id());
    assert!(!app.is_live(old));
    assert!(app.widget::<Label>(old).is_none());
    assert_eq!(app.widget::<Label>(new).unwrap().0, "new");
    assert_eq!(app.children(root), Some(&[new.id()][..]));
    assert_eq!(app.widgets::<Label>().count(), 1);
}

#[test]
fn removing_a_leaf_keeps_its_siblings() {
    let mut app = App::new();
    let root = app.root();
    let a = app.spawn(root, LabelBuilder("a"));
    let b = app.spawn(root, LabelBuilder("b"));
    let c = app.spawn(root, LabelBuilder("c"));
    assert!(app.remove(b));
    assert_eq!(app.children(root), Some(&[a.id(), c.id()][..]));
    assert!(app.widget::<Label>(a).is_some());
    assert!(app.widget::<Label>(c).is_some());
}

#[test]
fn removing_a_subtree_stales_every_descendant_three_levels_deep() {
    let mut app = App::new();
    let root = app.root();
    let top = app.spawn(root, LabelBuilder("top"));
    let mid = app.spawn(top, LabelBuilder("mid"));
    let leaf = app.spawn(mid, LabelBuilder("leaf"));
    let grandleaf = app.spawn(leaf, LabelBuilder("grandleaf"));

    assert!(app.remove(top));

    for id in [top.id(), mid.id(), leaf.id(), grandleaf.id()] {
        assert!(!app.is_live(id), "every id in the removed subtree is stale");
        assert!(app.widget::<Label>(id).is_none());
    }
    assert_eq!(
        app.children(root),
        Some(&[][..]),
        "top is gone from root's children"
    );
}

#[test]
fn move_to_moves_the_same_subtree_and_signals_once() {
    #[derive(Default)]
    struct Moves(Vec<Moved>);
    impl Resource for Moves {}

    let mut app = App::new();
    app.init_resource::<Moves>()
        .system(|app: &mut App, m: &Moved| app.resource_mut::<Moves>().0.push(*m));
    let root = app.root();
    let from = app.spawn(root, LabelBuilder("from"));
    let to = app.spawn(root, LabelBuilder("to"));
    let kept = app.spawn(to, LabelBuilder("kept"));
    let pair = app.spawn(from, PairBuilder);
    let left = app.widget::<Pair>(pair).unwrap().left;

    app.move_to(pair, to);
    app.flush();

    assert_eq!(app.children(from), Some(&[][..]));
    assert_eq!(app.children(to), Some(&[kept.id(), pair.id()][..]), "last");
    assert_eq!(app.parent(pair), Some(to.id()));
    assert_eq!(app.parent(left), Some(pair.id()), "the subtree came along");
    assert_eq!(
        app.widget::<Label>(left).unwrap().0,
        "left",
        "the same node"
    );
    let moved = Moved {
        id: pair.id(),
        from: from.id(),
        to: to.id(),
    };
    assert_eq!(app.resource::<Moves>().0, [moved]);

    app.move_to(pair, to);
    app.flush();
    assert_eq!(
        app.resource::<Moves>().0.len(),
        1,
        "to its own parent: nothing"
    );
}

#[test]
#[should_panic(expected = "under itself")]
fn moving_under_its_own_subtree_panics() {
    let mut app = App::new();
    let root = app.root();
    let top = app.spawn(root, LabelBuilder("top"));
    let leaf = app.spawn(top, LabelBuilder("leaf"));
    app.move_to(top, leaf);
}

#[test]
#[should_panic(expected = "root")]
fn removing_the_root_panics() {
    let mut app = App::new();
    app.remove(app.root());
}

#[test]
#[should_panic(expected = "stale parent")]
fn spawn_under_a_removed_parent_panics() {
    let mut app = App::new();
    let gone = app.spawn(app.root(), LabelBuilder("gone"));
    app.remove(gone);
    app.spawn(gone, LabelBuilder("orphan"));
}

// ── traversal ───────────────────────────────────────────────────────────

#[test]
fn ancestors_walk_up_to_and_including_the_root() {
    let mut app = App::new();
    let root = app.root();
    let a = app.spawn(root, LabelBuilder("a"));
    let b = app.spawn(a, LabelBuilder("b"));
    let c = app.spawn(b, LabelBuilder("c"));
    assert_eq!(
        app.ancestors(c).collect::<Vec<_>>(),
        vec![b.id(), a.id(), root]
    );
    assert_eq!(app.ancestors(a).collect::<Vec<_>>(), vec![root]);
    assert_eq!(app.ancestors(root).count(), 0);
    app.remove(c);
    assert_eq!(app.ancestors(c).count(), 0, "stale id has no ancestors");
}

#[test]
fn descendants_are_pre_order_excluding_self() {
    // root -> a -> (b -> d, c)
    let mut app = App::new();
    let root = app.root();
    let a = app.spawn(root, LabelBuilder("a"));
    let b = app.spawn(a, LabelBuilder("b"));
    let c = app.spawn(a, LabelBuilder("c"));
    let d = app.spawn(b, LabelBuilder("d"));
    assert_eq!(
        app.descendants(root).collect::<Vec<_>>(),
        vec![a.id(), b.id(), d.id(), c.id()]
    );
    assert_eq!(app.descendants(b).collect::<Vec<_>>(), vec![d.id()]);
    assert_eq!(app.descendants(d).count(), 0);
}

#[test]
fn descendants_of_a_removed_subtree_are_gone() {
    let mut app = App::new();
    let root = app.root();
    let pair = app.spawn(root, PairBuilder);
    let tail = app.spawn(root, LabelBuilder("tail"));
    assert_eq!(app.descendants(root).count(), 4);
    app.remove(pair);
    assert_eq!(app.descendants(root).collect::<Vec<_>>(), vec![tail.id()]);
    assert_eq!(
        app.descendants(pair).count(),
        0,
        "stale id has no descendants"
    );
}

#[test]
fn tree_view_reads_match_the_app_reads_and_outlive_the_call() {
    let mut app = App::new();
    let pair = app.spawn(app.root(), PairBuilder);
    let (left, right) = {
        let p = app.widget::<Pair>(pair).unwrap();
        (p.left, p.right)
    };

    let tree = app.tree();
    assert_eq!(tree.root(), app.root());
    assert!(tree.is_live(left));
    assert_eq!(tree.parent(left), Some(pair.id()));
    assert_eq!(tree.children(pair), Some(&[left.id(), right.id()][..]));
    assert_eq!(
        tree.ancestors(left).collect::<Vec<_>>(),
        vec![pair.id(), app.root()]
    );
    assert_eq!(
        tree.descendants(app.root()).collect::<Vec<_>>(),
        vec![pair.id(), left.id(), right.id()]
    );

    // The view is `Copy`, and a slice it hands out lives as long as the
    // view's borrow, not the method call.
    let children = tree.children(pair).unwrap();
    let copy = tree;
    assert_eq!(copy.children(pair), Some(children));
}

#[test]
fn spawner_me_is_the_handle_build_receives() {
    let mut app = App::new();
    let echo = app.spawn(app.root(), EchoBuilder);
    assert!(app.widget::<Echo>(echo).unwrap().0);
}

// ── slot ────────────────────────────────────────────────────────────────

struct SlotLeaf;
impl Build for SlotLeaf {
    type Widget = SlotLeaf;
}
impl Widget for SlotLeaf {
    type Builder = SlotLeaf;
    fn build(b: SlotLeaf, _: Handle<Self>, _: &mut Spawner<'_, Self>) -> Self {
        b
    }
}

#[test]
fn slot_is_the_arena_index_and_is_reused_after_remove() {
    let mut app = App::new();
    assert_eq!(app.root().slot(), 0);
    let a = app.spawn(app.root(), SlotLeaf);
    let b = app.spawn(app.root(), SlotLeaf);
    assert_ne!(a.id().slot(), b.id().slot());
    assert!(app.remove(a));
    let c = app.spawn(app.root(), SlotLeaf);
    assert_eq!(c.id().slot(), a.id().slot(), "the freed slot is reused");
    assert_ne!(c.id(), a.id(), "but the id is new");
}
