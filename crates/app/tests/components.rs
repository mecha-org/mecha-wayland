use app::prelude::*;

// ── fixtures ────────────────────────────────────────────────────────────

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct Pos {
    x: i32,
    y: i32,
}
impl Component for Pos {}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct Size {
    w: u32,
    h: u32,
}
impl Component for Size {}

/// Never registered by any test.
#[derive(Default, Debug, PartialEq)]
struct Never;
impl Component for Never {}

/// Four more types, so a six-column query has six distinct columns.
#[derive(Default, Debug, PartialEq)]
struct Tag1(u8);
impl Component for Tag1 {}
#[derive(Default, Debug, PartialEq)]
struct Tag2(u8);
impl Component for Tag2 {}
#[derive(Default, Debug, PartialEq)]
struct Tag3(u8);
impl Component for Tag3 {}
#[derive(Default, Debug, PartialEq)]
struct Tag4(u8);
impl Component for Tag4 {}

/// A widget with nothing in it.
struct Leaf;
impl Build for Leaf {
    type Widget = Leaf;
}
impl Widget for Leaf {
    type Builder = Leaf;
    fn build(b: Leaf, _me: Handle<Self>, _s: &mut Spawner<'_, Self>) -> Self {
        b
    }
}

/// Spawns two `Leaf` children, so one spawn makes three nodes.
struct Branch;
impl Build for Branch {
    type Widget = Branch;
}
impl Widget for Branch {
    type Builder = Branch;
    fn build(b: Branch, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        s.spawn(me, Leaf);
        s.spawn(me, Leaf);
        b
    }
}

/// Writes its own `Pos` during build and spawns a child with an initial
/// `Size`.
struct Placed {
    child: Handle<Leaf>,
}
struct PlacedBuilder {
    x: i32,
}
impl Build for PlacedBuilder {
    type Widget = Placed;
}
impl Widget for Placed {
    type Builder = PlacedBuilder;
    fn build(b: PlacedBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        s.component_mut::<Pos>(me).unwrap().x = b.x;
        assert_eq!(s.component::<Pos>(me).map(|p| p.x), Some(b.x));
        let child = s.spawn_with(me, Leaf, (Size { w: 3, h: 4 },));
        Placed { child }
    }
}

/// Records what its own `Pos` was when its build ran.
struct Seen {
    pos_at_build: Option<Pos>,
}
struct SeenBuilder;
impl Build for SeenBuilder {
    type Widget = Seen;
}
impl Widget for Seen {
    type Builder = SeenBuilder;
    fn build(_b: SeenBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        Seen {
            pos_at_build: s.component::<Pos>(me).copied(),
        }
    }
}

/// An app with `Pos` and `Size` registered.
fn app() -> App {
    let mut app = App::new();
    app.register_component::<Pos>();
    app.register_component::<Size>();
    app
}

// ── registration and reads ──────────────────────────────────────────────

#[test]
fn register_backfills_the_root_and_live_nodes_with_default() {
    let mut app = App::new();
    let before = app.spawn(app.root(), Leaf);
    app.register_component::<Pos>();
    assert_eq!(app.component::<Pos>(app.root()), Some(&Pos::default()));
    assert_eq!(app.component::<Pos>(before), Some(&Pos::default()));
    let after = app.spawn(app.root(), Leaf);
    assert_eq!(
        app.component::<Pos>(after),
        Some(&Pos::default()),
        "spawned after registration"
    );
}

#[test]
#[should_panic(expected = "already registered")]
fn registering_a_component_twice_panics() {
    let mut app = app();
    app.register_component::<Pos>();
}

#[test]
#[should_panic(expected = "not registered")]
fn reading_an_unregistered_component_panics() {
    let app = app();
    let _ = app.component::<Never>(app.root());
}

#[test]
fn component_of_a_stale_id_is_none() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    assert!(app.component::<Pos>(a).is_some());
    app.remove(a);
    assert_eq!(app.component::<Pos>(a), None);
}

// ── writes and change tracking ──────────────────────────────────────────

#[test]
fn writes_through_component_mut_are_visible_and_flagged_once() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    {
        let mut pos = app.component_mut::<Pos>(a).unwrap();
        assert_eq!(pos.id(), a.id());
        pos.x = 1;
        pos.y = 2;
    }
    assert_eq!(app.component::<Pos>(a), Some(&Pos { x: 1, y: 2 }));
    // A second guard to the same node before a drain adds no second entry.
    app.component_mut::<Pos>(a).unwrap().x = 3;
    assert_eq!(app.take_changed::<Pos>().collect::<Vec<_>>(), vec![a.id()]);
    assert_eq!(app.take_changed::<Pos>().count(), 0, "drained");
}

#[test]
fn deref_does_not_flag() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let guard = app.component_mut::<Pos>(a).unwrap();
    let _read = guard.x;
    drop(guard);
    assert_eq!(app.take_changed::<Pos>().count(), 0);
}

#[test]
fn take_changed_is_in_first_write_order() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let b = app.spawn(app.root(), Leaf);
    let c = app.spawn(app.root(), Leaf);
    app.component_mut::<Pos>(c).unwrap().x = 1;
    app.component_mut::<Pos>(a).unwrap().x = 1;
    app.component_mut::<Pos>(c).unwrap().x = 2;
    let _untouched = b;
    assert_eq!(
        app.take_changed::<Pos>().collect::<Vec<_>>(),
        vec![c.id(), a.id()]
    );
}

#[test]
fn a_node_removed_before_the_drain_is_skipped() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let b = app.spawn(app.root(), Leaf);
    app.component_mut::<Pos>(a).unwrap().x = 1;
    app.component_mut::<Pos>(b).unwrap().x = 1;
    app.remove(a);
    assert_eq!(app.take_changed::<Pos>().collect::<Vec<_>>(), vec![b.id()]);
}

#[test]
fn a_reused_slot_drains_only_the_new_id_once() {
    let mut app = app();
    let old = app.spawn(app.root(), Leaf);
    app.component_mut::<Pos>(old).unwrap().x = 1;
    app.remove(old);
    // The only free slot is `old`'s, so `new` reuses it.
    let new = app.spawn(app.root(), Leaf);
    assert_eq!(
        app.component::<Pos>(new),
        Some(&Pos::default()),
        "a reused slot starts at default"
    );
    app.component_mut::<Pos>(new).unwrap().x = 2;
    assert_eq!(
        app.take_changed::<Pos>().collect::<Vec<_>>(),
        vec![new.id()]
    );
}

#[test]
fn dropping_the_drain_early_still_clears() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let b = app.spawn(app.root(), Leaf);
    app.component_mut::<Pos>(a).unwrap().x = 1;
    app.component_mut::<Pos>(b).unwrap().x = 1;
    {
        let mut drain = app.take_changed::<Pos>();
        assert_eq!(drain.next(), Some(a.id()));
        // dropped with `b` still pending
    }
    assert_eq!(app.take_changed::<Pos>().count(), 0, "nothing left over");
    // `b`'s bit is clear again, so a new write is recorded.
    app.component_mut::<Pos>(b).unwrap().x = 5;
    assert_eq!(app.take_changed::<Pos>().collect::<Vec<_>>(), vec![b.id()]);
}

#[test]
fn set_if_neq_skips_equal_values() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    assert!(
        !app.component_mut::<Pos>(a)
            .unwrap()
            .set_if_neq(Pos::default())
    );
    assert_eq!(app.take_changed::<Pos>().count(), 0, "equal: not flagged");
    assert!(
        app.component_mut::<Pos>(a)
            .unwrap()
            .set_if_neq(Pos { x: 1, y: 0 })
    );
    assert_eq!(app.component::<Pos>(a), Some(&Pos { x: 1, y: 0 }));
    assert_eq!(app.take_changed::<Pos>().collect::<Vec<_>>(), vec![a.id()]);
}

#[test]
fn remove_resets_the_whole_subtree_to_default() {
    let mut app = app();
    let branch = app.spawn(app.root(), Branch);
    let kids = app.children(branch).unwrap().to_vec();
    app.component_mut::<Pos>(branch).unwrap().x = 1;
    app.component_mut::<Pos>(kids[0]).unwrap().x = 2;
    app.component_mut::<Pos>(kids[1]).unwrap().x = 3;
    app.remove(branch);
    // Three slots are free; three spawns reuse them all.
    let reused = [
        app.spawn(app.root(), Leaf),
        app.spawn(app.root(), Leaf),
        app.spawn(app.root(), Leaf),
    ];
    for id in reused {
        assert_eq!(app.component::<Pos>(id), Some(&Pos::default()));
    }
    assert_eq!(
        app.take_changed::<Pos>().count(),
        0,
        "stale ids are skipped"
    );
}

#[test]
fn component_mut_of_a_stale_id_is_none() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    app.remove(a);
    assert!(app.component_mut::<Pos>(a).is_none());
}

#[test]
#[should_panic(expected = "not registered")]
fn draining_an_unregistered_type_panics() {
    let mut app = app();
    let _ = app.take_changed::<Never>().count();
}

#[test]
#[should_panic(expected = "not registered")]
fn component_mut_of_an_unregistered_type_panics() {
    let mut app = app();
    let _ = app.component_mut::<Never>(app.root());
}

// ── views and queries ───────────────────────────────────────────────────

#[test]
fn a_shared_view_reads_and_indexes() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let gone = app.spawn(app.root(), Leaf);
    app.remove(gone);
    app.component_mut::<Pos>(a).unwrap().x = 1;

    let pos = app.query::<&Pos>();
    assert_eq!(pos.get(a), Some(&Pos { x: 1, y: 0 }));
    assert_eq!(pos[a].x, 1);
    assert_eq!(pos.get(gone), None);
}

#[test]
#[should_panic(expected = "stale id")]
fn indexing_a_stale_id_panics() {
    let mut app = app();
    let gone = app.spawn(app.root(), Leaf);
    app.remove(gone);
    let pos = app.query::<&Pos>();
    let _ = pos[gone];
}

#[test]
fn iteration_visits_live_nodes_in_slot_order_and_skips_dead_ones() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let b = app.spawn(app.root(), Leaf);
    let c = app.spawn(app.root(), Leaf);
    app.remove(b);
    let ids: Vec<NodeId> = app.query::<&Pos>().iter().map(|(id, _)| id).collect();
    assert_eq!(ids, vec![app.root(), a.id(), c.id()]);
    let ids: Vec<NodeId> = app
        .query::<&mut Pos>()
        .iter_mut()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ids, vec![app.root(), a.id(), c.id()]);
}

#[test]
fn iter_mut_guards_coexist_and_flag_only_what_was_written_in_write_order() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let _b = app.spawn(app.root(), Leaf);
    let c = app.spawn(app.root(), Leaf);
    {
        let mut pos = app.query::<&mut Pos>();
        // Slot order: root, a, b, c. Hold every guard at once.
        let mut guards: Vec<_> = pos.iter_mut().map(|(_, guard)| guard).collect();
        guards[3].x = 1; // c
        guards[1].x = 2; // a
        let _read = guards[2].x; // b: a read does not flag
    }
    assert_eq!(app.component::<Pos>(a), Some(&Pos { x: 2, y: 0 }));
    assert_eq!(
        app.take_changed::<Pos>().collect::<Vec<_>>(),
        vec![c.id(), a.id()]
    );
}

#[test]
fn get_mut_on_a_mutable_view_flags() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let gone = app.spawn(app.root(), Leaf);
    app.remove(gone);
    {
        let mut pos = app.query::<&mut Pos>();
        pos.get_mut(a).unwrap().y = 7;
        assert_eq!(pos.get(a), Some(&Pos { x: 0, y: 7 }));
        assert_eq!(pos[a].y, 7);
        assert!(pos.get_mut(gone).is_none(), "stale id");
    }
    assert_eq!(app.take_changed::<Pos>().collect::<Vec<_>>(), vec![a.id()]);
}

#[test]
fn a_shared_and_a_mutable_column_are_usable_together() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let b = app.spawn(app.root(), Leaf);
    app.component_mut::<Pos>(a).unwrap().x = 10;
    app.component_mut::<Pos>(b).unwrap().x = 20;
    let _ = app.take_changed::<Pos>().count();
    {
        let (pos, mut size) = app.query::<(&Pos, &mut Size)>();
        for (id, mut s) in size.iter_mut() {
            s.w = pos[id].x as u32 + 1;
        }
    }
    assert_eq!(app.component::<Size>(a), Some(&Size { w: 11, h: 0 }));
    assert_eq!(app.component::<Size>(b), Some(&Size { w: 21, h: 0 }));
    assert_eq!(
        app.take_changed::<Size>().collect::<Vec<_>>(),
        vec![app.root(), a.id(), b.id()]
    );
    assert_eq!(app.take_changed::<Pos>().count(), 0, "reads do not flag");
}

#[test]
fn the_same_column_twice_shared_is_fine() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let (first, second) = app.query::<(&Pos, &Pos)>();
    assert_eq!(first.get(a), second.get(a));
}

#[test]
#[should_panic(expected = "same place twice")]
fn the_same_column_shared_and_mutable_panics() {
    let mut app = app();
    let _ = app.query::<(&mut Pos, &Pos)>();
}

#[test]
#[should_panic(expected = "same place twice")]
fn the_same_column_twice_mutably_panics() {
    let mut app = app();
    let _ = app.query::<(&Size, &mut Pos, &mut Pos)>();
}

#[test]
fn a_six_column_query_resolves_in_any_order() {
    let mut app = app();
    app.register_component::<Tag1>();
    app.register_component::<Tag2>();
    app.register_component::<Tag3>();
    app.register_component::<Tag4>();
    let a = app.spawn(app.root(), Leaf);
    {
        // Deliberately not in registration order.
        let (t4, mut size, t1, mut t3, pos, mut t2) =
            app.query::<(&Tag4, &mut Size, &Tag1, &mut Tag3, &Pos, &mut Tag2)>();
        size.get_mut(a).unwrap().h = 1;
        t3.get_mut(a).unwrap().0 = 3;
        t2.get_mut(a).unwrap().0 = 2;
        assert_eq!(t4[a], Tag4(0));
        assert_eq!(t1[a], Tag1(0));
        assert_eq!(pos[a], Pos::default());
    }
    assert_eq!(app.component::<Size>(a), Some(&Size { w: 0, h: 1 }));
    assert_eq!(app.component::<Tag3>(a), Some(&Tag3(3)));
    assert_eq!(app.component::<Tag2>(a), Some(&Tag2(2)));
    assert_eq!(app.take_changed::<Tag2>().collect::<Vec<_>>(), vec![a.id()]);
}

#[test]
#[should_panic(expected = "not registered")]
fn querying_an_unregistered_type_panics() {
    let mut app = app();
    let _ = app.query::<(&Pos, &mut Never)>();
}

#[test]
fn a_query_with_skipped_columns_fetches_the_right_ones() {
    let mut app = app();
    app.register_component::<Tag1>();
    app.register_component::<Tag2>();
    app.register_component::<Tag3>();
    app.register_component::<Tag4>();
    let a = app.spawn(app.root(), Leaf);
    {
        // Columns 0, 3, 5 of a six-column registry: two gaps skipped.
        let (pos, mut t2, t4) = app.query::<(&Pos, &mut Tag2, &Tag4)>();
        t2.get_mut(a).unwrap().0 = 2;
        assert_eq!(pos[a], Pos::default());
        assert_eq!(t4[a], Tag4(0));
    }
    assert_eq!(app.component::<Tag2>(a), Some(&Tag2(2)));
    assert_eq!(app.take_changed::<Tag2>().collect::<Vec<_>>(), vec![a.id()]);
}

#[test]
fn iter_on_a_mutable_view_reads_without_flagging() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let b = app.spawn(app.root(), Leaf);
    let pos = app.query::<&mut Pos>();
    let ids: Vec<NodeId> = pos.iter().map(|(id, _)| id).collect();
    assert_eq!(ids, vec![app.root(), a.id(), b.id()]);
    assert_eq!(app.take_changed::<Pos>().count(), 0);
}

// ── split, bundles, and builders ────────────────────────────────────────

#[test]
fn split_walks_the_tree_while_writing_a_column() {
    let mut app = app();
    let branch = app.spawn(app.root(), Branch);
    let kids = app.children(branch).unwrap().to_vec();
    {
        let (tree, mut data) = app.split();
        let mut size = data.query::<&mut Size>();
        for id in tree.descendants(tree.root()) {
            let depth = tree.ancestors(id).count() as u32;
            size.get_mut(id).unwrap().h = depth;
        }
    }
    assert_eq!(app.component::<Size>(branch).map(|s| s.h), Some(1));
    assert_eq!(app.component::<Size>(kids[0]).map(|s| s.h), Some(2));
    assert_eq!(app.component::<Size>(kids[1]).map(|s| s.h), Some(2));
    assert_eq!(
        app.take_changed::<Size>().collect::<Vec<_>>(),
        vec![branch.id(), kids[0], kids[1]],
        "pre-order is the write order"
    );
}

#[test]
fn split_gives_single_node_access_beside_the_tree() {
    let mut app = app();
    let a = app.spawn(app.root(), Leaf);
    let (tree, mut data) = app.split();
    let parent = tree.parent(a).unwrap();
    data.component_mut::<Pos>(a).unwrap().x = 1;
    assert_eq!(data.component::<Pos>(a), Some(&Pos { x: 1, y: 0 }));
    assert_eq!(data.component::<Pos>(parent), Some(&Pos::default()));
    drop(data);
    assert_eq!(app.take_changed::<Pos>().collect::<Vec<_>>(), vec![a.id()]);
}

#[test]
fn spawn_with_values_are_in_place_before_the_build_runs() {
    let mut app = app();
    let seen = app.spawn_with(
        app.root(),
        SeenBuilder,
        (Pos { x: 5, y: 6 }, Size { w: 7, h: 8 }),
    );
    assert_eq!(
        app.widget::<Seen>(seen).unwrap().pos_at_build,
        Some(Pos { x: 5, y: 6 }),
        "the build saw its initial value"
    );
    assert_eq!(app.component::<Pos>(seen), Some(&Pos { x: 5, y: 6 }));
    assert_eq!(app.component::<Size>(seen), Some(&Size { w: 7, h: 8 }));
    assert_eq!(
        app.take_changed::<Pos>().collect::<Vec<_>>(),
        vec![seen.id()]
    );
    assert_eq!(
        app.take_changed::<Size>().collect::<Vec<_>>(),
        vec![seen.id()]
    );
}

#[test]
fn spawn_with_an_empty_bundle_is_spawn() {
    let mut app = app();
    let a = app.spawn_with(app.root(), Leaf, ());
    assert_eq!(app.component::<Pos>(a), Some(&Pos::default()));
    assert_eq!(
        app.take_changed::<Pos>().count(),
        0,
        "default is not a write"
    );
}

#[test]
#[should_panic(expected = "not registered")]
fn spawn_with_an_unregistered_type_panics() {
    let mut app = app();
    app.spawn_with(app.root(), Leaf, (Never,));
}

#[test]
fn a_builder_writes_its_own_component_and_is_flagged() {
    let mut app = app();
    let placed = app.spawn(app.root(), PlacedBuilder { x: 42 });
    assert_eq!(app.component::<Pos>(placed), Some(&Pos { x: 42, y: 0 }));
    assert_eq!(
        app.take_changed::<Pos>().collect::<Vec<_>>(),
        vec![placed.id()]
    );
}

#[test]
fn a_builder_spawns_a_child_with_initial_values() {
    let mut app = app();
    let placed = app.spawn(app.root(), PlacedBuilder { x: 1 });
    let child = app.widget::<Placed>(placed).unwrap().child;
    assert_eq!(app.component::<Size>(child), Some(&Size { w: 3, h: 4 }));
    assert_eq!(
        app.take_changed::<Size>().collect::<Vec<_>>(),
        vec![child.id()]
    );
}

// ── widgets read during a build ─────────────────────────────────────────

/// Exposes a slot: a child its parent is meant to spawn into.
struct Slotted {
    slot: Handle<Leaf>,
}
struct SlottedBuilder;
impl Build for SlottedBuilder {
    type Widget = Slotted;
}
impl Widget for Slotted {
    type Builder = SlottedBuilder;
    fn build(_b: SlottedBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        Slotted {
            slot: s.spawn(me, Leaf),
        }
    }
}

/// Spawns a `Slotted`, reads its slot handle and fills it; also records
/// what reading itself mid-build returned.
struct Filler {
    filled: Handle<Leaf>,
    self_seen: bool,
}
struct FillerBuilder;
impl Build for FillerBuilder {
    type Widget = Filler;
}
impl Widget for Filler {
    type Builder = FillerBuilder;
    fn build(_b: FillerBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let slotted = s.spawn(me, SlottedBuilder);
        let slot = s.widget::<Slotted>(slotted).expect("built child").slot;
        let filled = s.spawn(slot, Leaf);
        Filler {
            filled,
            self_seen: s.widget::<Filler>(me).is_some(),
        }
    }
}

#[test]
fn spawner_reads_a_child_built_earlier_in_the_same_build() {
    let mut app = app();
    let filler = app.spawn(app.root(), FillerBuilder);
    let f = app.widget::<Filler>(filler).unwrap();
    let (filled, self_seen) = (f.filled, f.self_seen);
    let slotted = app.children(filler).unwrap()[0];
    let slot = app.widget::<Slotted>(slotted).unwrap().slot;
    assert_eq!(app.parent(filled), Some(slot.into()));
    assert!(!self_seen, "me is not readable while its build runs");
}
