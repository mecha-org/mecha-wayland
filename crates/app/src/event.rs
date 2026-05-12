use frunk::{HCons, HNil};
use std::any::TypeId;
use std::marker::PhantomData;
use crate::module::MountedModule;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EventId(TypeId);

impl EventId {
    pub fn of<E: Event>() -> Self {
        Self(TypeId::of::<E>())
    }
}

pub trait Event: 'static {}

pub struct TypedHandler<T, E: Event, F: Fn(&mut T, &E)> {
    f: F,
    _marker: PhantomData<fn(&mut T, &E)>,
}

impl<T, E: Event, F: Fn(&mut T, &E)> TypedHandler<T, E, F> {
    pub fn new(f: F) -> Self {
        Self {
            f,
            _marker: PhantomData,
        }
    }
}

pub trait DispatchHandlers<T, E: Event> {
    fn dispatch(&self, state: &mut T, event: &E);
}

impl<T, E: Event> DispatchHandlers<T, E> for HNil {
    fn dispatch(&self, _: &mut T, _: &E) {}
}

impl<T, E, E2, F, Tail> DispatchHandlers<T, E> for HCons<TypedHandler<T, E2, F>, Tail>
where
    E: Event,
    E2: Event,
    F: Fn(&mut T, &E2),
    Tail: DispatchHandlers<T, E>,
{
    fn dispatch(&self, state: &mut T, event: &E) {
        if TypeId::of::<E>() == TypeId::of::<E2>() {
            // SAFETY: TypeId equality guarantees E == E2.
            let e2 = unsafe { &*(event as *const E as *const E2) };
            (self.head.f)(state, e2);
        }
        self.tail.dispatch(state, event);
    }
}

pub trait DispatchEvent<S, E: Event> {
    fn dispatch(&mut self, state: &mut S, event: &E);
}

impl<S, E: Event> DispatchEvent<S, E> for HNil {
    fn dispatch(&mut self, _: &mut S, _: &E) {}
}

impl<S, E, Head, Tail> DispatchEvent<S, E> for HCons<Head, Tail>
where
    E: Event,
    Head: DispatchEvent<S, E>,
    Tail: DispatchEvent<S, E>,
{
    fn dispatch(&mut self, state: &mut S, event: &E) {
        self.head.dispatch(state, event);
        self.tail.dispatch(state, event);
    }
}

impl<S, T, L, H, E> DispatchEvent<S, E> for MountedModule<S, T, L, H>
where
    E: Event,
    L: Fn(&mut S) -> &mut T,
    H: DispatchHandlers<T, E>,
{
    fn dispatch(&mut self, state: &mut S, event: &E) {
        let sub = (self.lens)(state);
        self.handlers.dispatch(sub, event);
    }
}

impl<T, U, L, H, E> DispatchHandlers<T, E> for MountedModule<T, U, L, H>
where
    E: Event,
    L: Fn(&mut T) -> &mut U,
    H: DispatchHandlers<U, E>,
{
    fn dispatch(&self, state: &mut T, event: &E) {
        let sub = (self.lens)(state);
        self.handlers.dispatch(sub, event);
    }
}

impl<T, U, L, H, E, Tail> DispatchHandlers<T, E> for HCons<MountedModule<T, U, L, H>, Tail>
where
    E: Event,
    L: Fn(&mut T) -> &mut U,
    H: DispatchHandlers<U, E>,
    Tail: DispatchHandlers<T, E>,
{
    fn dispatch(&self, state: &mut T, event: &E) {
        self.head.dispatch(state, event);
        self.tail.dispatch(state, event);
    }
}
