use std::any::{Any, TypeId, type_name};
use std::cell::Cell;
use std::collections::HashMap;
use std::vec::Drain;

use crate::NodeId;
use crate::query::CompMut;
use crate::slots::Slots;
use crate::store::{AnyStore, Store};

/// Per-node data outside the widget. Every node carries one `C` per
/// registered component type: `Default` when the node is spawned, when
/// `C` is registered after the node already exists, and again once the
/// node is removed. A marker; the runtime never calls into a component
/// beyond `Default`.
///
/// Registration is explicit ([`App::register_component`](crate::App::register_component))
/// so that a type whose module never registered it fails loudly on first
/// use instead of silently doing nothing.
///
/// ```
/// # use app::Component;
/// #[derive(Default)]
/// struct Rect { x: f32, y: f32, w: f32, h: f32 }
/// impl Component for Rect {}
/// ```
pub trait Component: Default + 'static {}

/// One component type's data: a value per slot, a changed bit per slot,
/// and the list of ids written since the last drain.
///
/// Invariant: a set bit means the id is in `list`. A writer pushes before
/// it sets the bit, so a set bit always has its entry; the reverse can
/// hold briefly and only costs a duplicate entry, which the drain
/// tolerates. Removing a node clears its bit but leaves its (now stale)
/// id in the list; the drain skips it.
pub(crate) struct Column<C: Default> {
    pub(crate) values: Store<C>,
    pub(crate) changed: Store<bool>,
    /// A `Cell` so that many write guards from one `iter_mut` can share
    /// it while each owns its own bit. A push is `take`, push, `set`:
    /// three pointer moves. Only ever reached through a borrow of the
    /// column, so nothing is shared past a view's lifetime.
    pub(crate) list: Cell<Vec<NodeId>>,
}

impl<C: Component> Column<C> {
    /// A column grown to `len`, every slot `Default`.
    pub fn new(len: u64) -> Self {
        let mut column = Self {
            values: Store::new(),
            changed: Store::new(),
            list: Cell::new(Vec::new()),
        };
        column.grow(len);
        column
    }

    pub fn grow(&mut self, len: u64) {
        self.values.grow(len);
        self.changed.grow(len);
    }

    /// `None` for a stale id.
    pub fn get(&self, slots: &Slots, id: NodeId) -> Option<&C> {
        slots.is_live(id).then(|| self.values.get(id.slot()))
    }

    /// Live slots only, in slot order. Walks every slot, dead ones
    /// included, so the cost is the arena's high-water mark.
    pub fn iter<'a>(&'a self, slots: &'a Slots) -> impl Iterator<Item = (NodeId, &'a C)> + 'a {
        self.values
            .as_slice()
            .iter()
            .enumerate()
            .filter_map(move |(index, value)| Some((slots.id(index as u64)?, value)))
    }

    /// A write guard for one node. `None` for a stale id.
    pub fn get_mut<'a>(&'a mut self, slots: &Slots, id: NodeId) -> Option<CompMut<'a, C>> {
        if !slots.is_live(id) {
            return None;
        }
        let index = id.slot();
        Some(CompMut::new(
            id,
            self.values.get_mut(index),
            self.changed.get_mut(index),
            &self.list,
        ))
    }

    /// Live slots only, in slot order, each as a write guard. The guards
    /// may coexist: every item owns its own value and bit, and they share
    /// the changed list through its `Cell`.
    pub fn iter_mut<'a>(
        &'a mut self,
        slots: &'a Slots,
    ) -> impl Iterator<Item = (NodeId, CompMut<'a, C>)> + 'a {
        let list = &self.list;
        self.values
            .as_mut_slice()
            .iter_mut()
            .zip(self.changed.as_mut_slice().iter_mut())
            .enumerate()
            .filter_map(move |(index, (value, bit))| {
                let id = slots.id(index as u64)?;
                Some((id, CompMut::new(id, value, bit, list)))
            })
    }

    /// Whether a write is waiting for the drain.
    pub fn has_changes(&self) -> bool {
        let list = self.list.take();
        let changed = !list.is_empty();
        self.list.set(list);
        changed
    }

    /// Drain the changed list. See [`Changed`].
    pub fn take_changed<'a>(&'a mut self, slots: &'a Slots) -> Changed<'a> {
        Changed {
            slots,
            bits: &mut self.changed,
            drain: self.list.get_mut().drain(..),
        }
    }
}

/// Drains a column's changed list in first-write order. For each entry it
/// clears the slot's bit, then yields the id only if the node is still
/// live. O(changed). The list keeps its capacity.
///
/// Dropping it early finishes the clearing, so "bit set means listed"
/// holds afterwards. Leaking it (`mem::forget`) does not, and leaves
/// those nodes unable to flag again until their slot is freed.
pub(crate) struct Changed<'a> {
    slots: &'a Slots,
    bits: &'a mut Store<bool>,
    drain: Drain<'a, NodeId>,
}

impl Iterator for Changed<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        loop {
            let id = self.drain.next()?;
            self.bits.set(id.slot(), false);
            if self.slots.is_live(id) {
                return Some(id);
            }
        }
    }
}

impl Drop for Changed<'_> {
    fn drop(&mut self) {
        for id in self.drain.by_ref() {
            self.bits.set(id.slot(), false);
        }
    }
}

/// Object-safe view of a [`Column`], so the registry can grow and free
/// every column without knowing the component types.
pub(crate) trait AnyColumn {
    /// Resize both stores to the arena length. Never shrinks.
    fn grow(&mut self, len: u64);
    /// Value back to `Default`, bit cleared. Out of range is a no-op.
    fn free(&mut self, index: u64);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<C: Component> AnyColumn for Column<C> {
    fn grow(&mut self, len: u64) {
        Column::grow(self, len);
    }

    fn free(&mut self, index: u64) {
        AnyStore::free(&mut self.values, index);
        AnyStore::free(&mut self.changed, index);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Every registered component column. One hash per column lookup; the
/// per-node path below that is index-only.
pub(crate) struct Components {
    /// Type to column.
    columns: HashMap<TypeId, u32>,
    /// Column to store.
    stores: Vec<Box<dyn AnyColumn>>,
}

impl Components {
    pub fn new() -> Self {
        Self {
            columns: HashMap::new(),
            stores: Vec::new(),
        }
    }

    /// Allocate the column for `C`, grown to `len` so every existing slot
    /// holds `Default`.
    ///
    /// # Panics
    ///
    /// If `C` is already registered.
    pub fn register<C: Component>(&mut self, len: u64) {
        let type_id = TypeId::of::<C>();
        assert!(
            !self.columns.contains_key(&type_id),
            "component {} is already registered",
            type_name::<C>()
        );
        let column = self.stores.len() as u32;
        self.stores.push(Box::new(Column::<C>::new(len)));
        self.columns.insert(type_id, column);
    }

    /// The column number of `C`.
    ///
    /// # Panics
    ///
    /// If `C` was never registered: a setup bug, not a state.
    pub fn column_of<C: Component>(&self) -> u32 {
        match self.columns.get(&TypeId::of::<C>()) {
            Some(&column) => column,
            None => panic!("component {} is not registered", type_name::<C>()),
        }
    }

    /// Panics if `C` is not registered.
    pub fn column<C: Component>(&self) -> &Column<C> {
        self.stores[self.column_of::<C>() as usize]
            .as_any()
            .downcast_ref()
            .expect("a column holds its registered type")
    }

    /// Panics if `C` is not registered.
    pub fn column_mut<C: Component>(&mut self) -> &mut Column<C> {
        let column = self.column_of::<C>() as usize;
        self.stores[column]
            .as_any_mut()
            .downcast_mut()
            .expect("a column holds its registered type")
    }

    /// Grow every column to the arena length.
    pub fn grow(&mut self, len: u64) {
        for store in &mut self.stores {
            store.grow(len);
        }
    }

    /// Reset one slot in every column.
    pub fn free(&mut self, index: u64) {
        for store in &mut self.stores {
            store.free(index);
        }
    }

    /// The columns by number, for a query that borrows several at once.
    pub fn stores_mut(&mut self) -> &mut [Box<dyn AnyColumn>] {
        &mut self.stores
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Debug, PartialEq)]
    struct Pos(i32);
    impl Component for Pos {}

    #[derive(Default, Debug, PartialEq)]
    struct Tag(u8);
    impl Component for Tag {}

    #[test]
    fn column_grows_with_defaults_and_free_resets_value_and_bit() {
        let mut c = Column::<Pos>::new(2);
        c.grow(4);
        assert_eq!(*c.values.get(3), Pos(0));
        assert!(!*c.changed.get(3));
        c.values.set(3, Pos(9));
        c.changed.set(3, true);
        AnyColumn::free(&mut c, 3);
        assert_eq!(*c.values.get(3), Pos(0), "value back to default");
        assert!(!*c.changed.get(3), "bit cleared");
        AnyColumn::free(&mut c, 50); // out of range is a no-op
    }

    #[test]
    fn get_and_iter_follow_slot_liveness() {
        let mut slots = Slots::new();
        let (a, ga) = slots.alloc(1);
        let (b, _) = slots.alloc(1);
        let (c, gc) = slots.alloc(1);
        slots.free(b);
        let mut column = Column::<Pos>::new(slots.len());
        column.values.set(a, Pos(1));
        column.values.set(c, Pos(3));

        let id_a = NodeId::new(ga, 1, a);
        let id_c = NodeId::new(gc, 1, c);
        assert_eq!(column.get(&slots, id_a), Some(&Pos(1)));
        assert_eq!(column.get(&slots, NodeId::new(0, 1, b)), None, "dead slot");
        assert_eq!(column.get(&slots, NodeId::new(ga + 1, 1, a)), None, "stale");

        let seen: Vec<(NodeId, i32)> = column.iter(&slots).map(|(id, p)| (id, p.0)).collect();
        assert_eq!(
            seen,
            vec![(id_a, 1), (id_c, 3)],
            "live slots only, in slot order"
        );
    }

    #[test]
    fn register_assigns_columns_by_type() {
        let mut cs = Components::new();
        cs.register::<Pos>(3);
        cs.register::<Tag>(3);
        assert_eq!(cs.column_of::<Pos>(), 0);
        assert_eq!(cs.column_of::<Tag>(), 1);
        assert_eq!(*cs.column::<Pos>().values.get(2), Pos(0), "grown to len");
        cs.column_mut::<Tag>().values.set(1, Tag(7));
        assert_eq!(*cs.column::<Tag>().values.get(1), Tag(7));
    }

    #[test]
    #[should_panic(expected = "already registered")]
    fn registering_twice_panics() {
        let mut cs = Components::new();
        cs.register::<Pos>(0);
        cs.register::<Pos>(0);
    }

    #[test]
    #[should_panic(expected = "not registered")]
    fn column_of_an_unregistered_type_panics() {
        let cs = Components::new();
        cs.column_of::<Pos>();
    }

    #[test]
    fn grow_and_free_reach_every_column() {
        let mut cs = Components::new();
        cs.register::<Pos>(1);
        cs.register::<Tag>(1);
        cs.grow(4);
        cs.column_mut::<Pos>().values.set(3, Pos(1));
        cs.column_mut::<Tag>().values.set(3, Tag(2));
        cs.column_mut::<Tag>().changed.set(3, true);
        cs.free(3);
        assert_eq!(*cs.column::<Pos>().values.get(3), Pos(0));
        assert_eq!(*cs.column::<Tag>().values.get(3), Tag(0));
        assert!(!*cs.column::<Tag>().changed.get(3));
        assert_eq!(cs.stores_mut().len(), 2);
    }
}
