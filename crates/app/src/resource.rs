//! Resources: app-wide data, one value per type, with a changed bit.

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

/// App-wide data outside the tree: one value per type for the whole app,
/// where a [`Component`](crate::Component) is one value per node. A
/// marker; the runtime never calls into a resource. No `Default` bound:
/// a resource is inserted by value through
/// [`App::insert_resource`](crate::App::insert_resource), and
/// [`App::init_resource`](crate::App::init_resource) adds the bound at
/// its own call site.
///
/// A type may be a `Component` and a `Resource` at once; the two are
/// separate values with separate change tracking.
///
/// ```
/// # use app::Resource;
/// struct Windows { open: Vec<u32> }
/// impl Resource for Windows {}
/// ```
pub trait Resource: 'static {}

/// One resource: the value and whether it was written since the last
/// drain.
pub(crate) struct Entry<R> {
    pub(crate) value: R,
    pub(crate) changed: bool,
}

/// Exclusive access to a resource, and the only way to a `&mut R`.
///
/// Derefs both ways. The first `DerefMut` sets the changed bit; every
/// later `DerefMut` on the same guard repeats the same unconditional
/// store. Assigning an equal value through `DerefMut` still counts as a
/// change. Use [`ResourceMut::set_if_neq`] when that matters.
pub struct ResourceMut<'a, R> {
    value: &'a mut R,
    changed: &'a mut bool,
}

impl<'a, R: Resource> ResourceMut<'a, R> {
    pub(crate) fn new(entry: &'a mut Entry<R>) -> Self {
        Self {
            value: &mut entry.value,
            changed: &mut entry.changed,
        }
    }

    /// Write `value` only if it differs from what is there, flagging the
    /// resource only then. Returns whether it wrote.
    pub fn set_if_neq(&mut self, value: R) -> bool
    where
        R: PartialEq,
    {
        if *self.value == value {
            return false;
        }
        *self.value = value;
        *self.changed = true;
        true
    }
}

impl<R: Resource> Deref for ResourceMut<'_, R> {
    type Target = R;

    fn deref(&self) -> &R {
        self.value
    }
}

impl<R: Resource> DerefMut for ResourceMut<'_, R> {
    fn deref_mut(&mut self) -> &mut R {
        *self.changed = true;
        self.value
    }
}

/// Every inserted resource, by type. One hash and one downcast per
/// access. No column and no slot: a resource has one value, so there is
/// nothing per node to grow or free.
pub(crate) struct Resources {
    /// Each box is an `Entry<R>` for the `R` of its key.
    map: HashMap<TypeId, Box<dyn Any>>,
}

impl Resources {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Store `value`, flagged as changed, and return what was there.
    pub fn insert<R: Resource>(&mut self, value: R) -> Option<R> {
        match self.map.get_mut(&TypeId::of::<R>()) {
            Some(boxed) => {
                let entry = boxed
                    .downcast_mut::<Entry<R>>()
                    .expect("an entry holds its type");
                entry.changed = true;
                Some(std::mem::replace(&mut entry.value, value))
            }
            None => {
                self.map.insert(
                    TypeId::of::<R>(),
                    Box::new(Entry {
                        value,
                        changed: true,
                    }),
                );
                None
            }
        }
    }

    /// Store `R::default()`, unflagged, if `R` is absent. Returns whether
    /// it inserted.
    pub fn init<R: Resource + Default>(&mut self) -> bool {
        if self.map.contains_key(&TypeId::of::<R>()) {
            return false;
        }
        self.map.insert(
            TypeId::of::<R>(),
            Box::new(Entry {
                value: R::default(),
                changed: false,
            }),
        );
        true
    }

    pub fn has<R: Resource>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<R>())
    }

    /// The key of `R`, for a query that borrows entries by key.
    ///
    /// # Panics
    ///
    /// If `R` was never inserted: a setup bug, not a state.
    pub fn type_of<R: Resource>(&self) -> TypeId {
        let type_id = TypeId::of::<R>();
        assert!(
            self.map.contains_key(&type_id),
            "resource {} is not inserted",
            type_name::<R>()
        );
        type_id
    }

    /// Panics if `R` is not inserted.
    fn entry<R: Resource>(&self) -> &Entry<R> {
        match self.map.get(&TypeId::of::<R>()) {
            Some(boxed) => boxed.downcast_ref().expect("an entry holds its type"),
            None => panic!("resource {} is not inserted", type_name::<R>()),
        }
    }

    /// Panics if `R` is not inserted.
    fn entry_mut<R: Resource>(&mut self) -> &mut Entry<R> {
        match self.map.get_mut(&TypeId::of::<R>()) {
            Some(boxed) => boxed.downcast_mut().expect("an entry holds its type"),
            None => panic!("resource {} is not inserted", type_name::<R>()),
        }
    }

    /// Panics if `R` is not inserted.
    pub fn get<R: Resource>(&self) -> &R {
        &self.entry::<R>().value
    }

    /// Panics if `R` is not inserted.
    pub fn get_mut<R: Resource>(&mut self) -> ResourceMut<'_, R> {
        ResourceMut::new(self.entry_mut::<R>())
    }

    /// Whether `R` was written since its last drain. Panics if `R` is not
    /// inserted.
    pub fn is_changed<R: Resource>(&self) -> bool {
        self.entry::<R>().changed
    }

    /// Clear the bit and return what it was. Panics if `R` is not
    /// inserted.
    pub fn take_changed<R: Resource>(&mut self) -> bool {
        std::mem::take(&mut self.entry_mut::<R>().changed)
    }

    /// Every entry with its key, for a query that borrows several at
    /// once. Map order: meaningless, and different from run to run.
    pub fn entries_mut(&mut self) -> impl Iterator<Item = (&TypeId, &mut Box<dyn Any>)> {
        self.map.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Debug, PartialEq)]
    struct Count(u32);
    impl Resource for Count {}

    #[derive(Debug, PartialEq)]
    struct Name(&'static str);
    impl Resource for Name {}

    #[test]
    fn insert_returns_the_old_value_and_flags_both_times() {
        let mut rs = Resources::new();
        assert_eq!(rs.insert(Count(1)), None);
        assert!(rs.take_changed::<Count>());
        assert_eq!(rs.insert(Count(2)), Some(Count(1)));
        assert!(rs.take_changed::<Count>());
        assert_eq!(rs.get::<Count>(), &Count(2));
        assert!(!rs.take_changed::<Count>(), "take clears");
    }

    #[test]
    fn init_inserts_unflagged_once() {
        let mut rs = Resources::new();
        assert!(!rs.has::<Count>());
        assert!(rs.init::<Count>());
        assert!(rs.has::<Count>());
        assert!(!rs.take_changed::<Count>(), "init does not flag");
        rs.get_mut::<Count>().0 = 3;
        assert!(!rs.init::<Count>(), "present: a no-op");
        assert_eq!(rs.get::<Count>(), &Count(3));
    }

    #[test]
    fn types_are_separate_entries() {
        let mut rs = Resources::new();
        rs.insert(Count(1));
        rs.insert(Name("a"));
        assert_eq!(rs.get::<Count>(), &Count(1));
        assert_eq!(rs.get::<Name>(), &Name("a"));
        assert_eq!(rs.type_of::<Name>(), TypeId::of::<Name>());
        assert_eq!(rs.entries_mut().count(), 2);
    }

    #[test]
    fn the_guard_flags_on_deref_mut_and_set_if_neq_on_difference() {
        let mut rs = Resources::new();
        rs.init::<Count>();
        let guard = rs.get_mut::<Count>();
        assert_eq!(guard.0, 0);
        drop(guard);
        assert!(!rs.take_changed::<Count>(), "a read does not flag");
        rs.get_mut::<Count>().0 = 1;
        assert!(rs.take_changed::<Count>());
        assert!(!rs.get_mut::<Count>().set_if_neq(Count(1)));
        assert!(!rs.take_changed::<Count>());
        assert!(rs.get_mut::<Count>().set_if_neq(Count(2)));
        assert!(rs.take_changed::<Count>());
    }

    #[test]
    #[should_panic(expected = "not inserted")]
    fn get_of_an_absent_resource_panics() {
        let rs = Resources::new();
        rs.get::<Count>();
    }

    #[test]
    #[should_panic(expected = "not inserted")]
    fn get_mut_of_an_absent_resource_panics() {
        let mut rs = Resources::new();
        rs.get_mut::<Count>();
    }

    #[test]
    #[should_panic(expected = "not inserted")]
    fn type_of_an_absent_resource_panics() {
        let rs = Resources::new();
        rs.type_of::<Count>();
    }
}
