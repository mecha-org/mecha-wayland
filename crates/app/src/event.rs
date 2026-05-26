use crate::module::MountedModule;
use frunk::{HCons, HNil};
use std::any::TypeId;
use std::marker::PhantomData;

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

pub struct TypedProcessor<T, E: Event, Out, F: Fn(&mut T, &E) -> Out> {
    f: F,
    _marker: PhantomData<fn(&mut T, &E) -> Out>,
}

impl<T, E: Event, Out, F: Fn(&mut T, &E) -> Out> TypedProcessor<T, E, Out, F> {
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

impl<T, E, E2, Out, F, Tail> DispatchHandlers<T, E> for HCons<TypedProcessor<T, E2, Out, F>, Tail>
where
    E: Event,
    E2: Event,
    F: Fn(&mut T, &E2) -> Out,
    Tail: DispatchHandlers<T, E>,
{
    fn dispatch(&self, state: &mut T, event: &E) {
        self.tail.dispatch(state, event);
    }
}

pub trait ProcessHandlers<T, E: Event> {
    type Out;
    fn process(&self, state: &mut T, event: &E) -> Self::Out;
}

impl<T, E: Event> ProcessHandlers<T, E> for HNil {
    type Out = HNil;
    fn process(&self, _: &mut T, _: &E) -> HNil {
        HNil
    }
}

impl<T, E, E2, F, Tail> ProcessHandlers<T, E> for HCons<TypedHandler<T, E2, F>, Tail>
where
    E: Event,
    E2: Event,
    F: Fn(&mut T, &E2),
    Tail: ProcessHandlers<T, E>,
{
    type Out = Tail::Out;
    fn process(&self, state: &mut T, event: &E) -> Self::Out {
        self.tail.process(state, event)
    }
}

impl<T, E, E2, ProcOut, F, Tail> ProcessHandlers<T, E>
    for HCons<TypedProcessor<T, E2, ProcOut, F>, Tail>
where
    E: Event,
    E2: Event,
    F: Fn(&mut T, &E2) -> ProcOut,
    ProcOut: Default,
    Tail: ProcessHandlers<T, E>,
{
    type Out = HCons<ProcOut, Tail::Out>;
    fn process(&self, state: &mut T, event: &E) -> Self::Out {
        let head = if TypeId::of::<E>() == TypeId::of::<E2>() {
            // SAFETY: TypeId equality guarantees E == E2.
            let e2 = unsafe { &*(event as *const E as *const E2) };
            (self.head.f)(state, e2)
        } else {
            ProcOut::default()
        };
        HCons {
            head,
            tail: self.tail.process(state, event),
        }
    }
}

impl<T, U, L, H, E> ProcessHandlers<T, E> for MountedModule<T, U, L, H>
where
    E: Event,
    L: Fn(&mut T) -> &mut U,
    H: ProcessHandlers<U, E>,
{
    type Out = H::Out;
    fn process(&self, state: &mut T, event: &E) -> Self::Out {
        let sub = (self.lens)(state);
        self.handlers.process(sub, event)
    }
}

impl<T, U, L, H, E, Tail> ProcessHandlers<T, E> for HCons<MountedModule<T, U, L, H>, Tail>
where
    E: Event,
    L: Fn(&mut T) -> &mut U,
    H: ProcessHandlers<U, E>,
    Tail: ProcessHandlers<T, E>,
{
    type Out = HCons<H::Out, Tail::Out>;
    fn process(&self, state: &mut T, event: &E) -> Self::Out {
        HCons {
            head: self.head.process(state, event),
            tail: self.tail.process(state, event),
        }
    }
}

pub struct Zero;
pub struct Succ<N>(std::marker::PhantomData<N>);

macro_rules! depth {
    () => { Zero };
    (_ $($rest:tt)*) => { Succ<depth!($($rest)*)> };
}
pub type MaxDepth = depth!(
    _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
);

// pub type MaxDepth = depth!(
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
//     _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
// );

pub trait DispatchProduced<D, S, M> {
    fn dispatch_produced(self, state: &mut S, modules: &mut M);
}

impl<S, M> DispatchProduced<Zero, S, M> for HNil {
    fn dispatch_produced(self, _: &mut S, _: &mut M) {}
}

impl<S, M, Tail> DispatchProduced<Zero, S, M> for HCons<HNil, Tail>
where
    Tail: DispatchProduced<Zero, S, M>,
{
    fn dispatch_produced(self, state: &mut S, modules: &mut M) {
        self.tail.dispatch_produced(state, modules);
    }
}

impl<S, M, E, Tail> DispatchProduced<Zero, S, M> for HCons<Option<E>, Tail>
where
    E: Event,
    M: DispatchEvent<S, E>,
    Tail: DispatchProduced<Zero, S, M>,
{
    fn dispatch_produced(self, state: &mut S, modules: &mut M) {
        if let Some(event) = self.head {
            modules.dispatch(state, &event);
        }
        self.tail.dispatch_produced(state, modules);
    }
}

impl<S, M, E, Tail> DispatchProduced<Zero, S, M> for HCons<Vec<E>, Tail>
where
    E: Event,
    M: DispatchEvent<S, E>,
    Tail: DispatchProduced<Zero, S, M>,
{
    fn dispatch_produced(self, state: &mut S, modules: &mut M) {
        for event in self.head.into_iter() {
            modules.dispatch(state, &event);
        }
        self.tail.dispatch_produced(state, modules);
    }
}

impl<S, M, E, Tail> DispatchProduced<Zero, S, M> for HCons<Vec<Option<E>>, Tail>
where
    E: Event,
    M: DispatchEvent<S, E>,
    Tail: DispatchProduced<Zero, S, M>,
{
    fn dispatch_produced(self, state: &mut S, modules: &mut M) {
        for event in self.head.into_iter().flatten() {
            modules.dispatch(state, &event);
        }
        self.tail.dispatch_produced(state, modules);
    }
}

impl<S, M, H, T, Tail> DispatchProduced<Zero, S, M> for HCons<HCons<H, T>, Tail>
where
    HCons<H, T>: DispatchProduced<Zero, S, M>,
    Tail: DispatchProduced<Zero, S, M>,
{
    fn dispatch_produced(self, state: &mut S, modules: &mut M) {
        self.head.dispatch_produced(state, modules);
        self.tail.dispatch_produced(state, modules);
    }
}

impl<N, S, M> DispatchProduced<Succ<N>, S, M> for HNil {
    fn dispatch_produced(self, _: &mut S, _: &mut M) {}
}

impl<N, S, M, Tail> DispatchProduced<Succ<N>, S, M> for HCons<HNil, Tail>
where
    Tail: DispatchProduced<Succ<N>, S, M>,
{
    fn dispatch_produced(self, state: &mut S, modules: &mut M) {
        self.tail.dispatch_produced(state, modules);
    }
}

impl<N, S, M, E, Tail> DispatchProduced<Succ<N>, S, M> for HCons<Option<E>, Tail>
where
    E: Event,
    M: DispatchEvent<S, E> + ProcessHandlers<S, E>,
    <M as ProcessHandlers<S, E>>::Out: DispatchProduced<N, S, M>,
    Tail: DispatchProduced<Succ<N>, S, M>,
{
    fn dispatch_produced(self, state: &mut S, modules: &mut M) {
        if let Some(event) = self.head {
            let produced = modules.process(state, &event);
            produced.dispatch_produced(state, modules);
            modules.dispatch(state, &event);
        }
        self.tail.dispatch_produced(state, modules);
    }
}

impl<N, S, M, E, Tail> DispatchProduced<Succ<N>, S, M> for HCons<Vec<E>, Tail>
where
    E: Event,
    M: DispatchEvent<S, E> + ProcessHandlers<S, E>,
    <M as ProcessHandlers<S, E>>::Out: DispatchProduced<N, S, M>,
    Tail: DispatchProduced<Succ<N>, S, M>,
{
    fn dispatch_produced(self, state: &mut S, modules: &mut M) {
        for event in self.head.into_iter() {
            let produced = modules.process(state, &event);
            produced.dispatch_produced(state, modules);
            modules.dispatch(state, &event);
        }
        self.tail.dispatch_produced(state, modules);
    }
}

impl<N, S, M, E, Tail> DispatchProduced<Succ<N>, S, M> for HCons<Vec<Option<E>>, Tail>
where
    E: Event,
    M: DispatchEvent<S, E> + ProcessHandlers<S, E>,
    <M as ProcessHandlers<S, E>>::Out: DispatchProduced<N, S, M>,
    Tail: DispatchProduced<Succ<N>, S, M>,
{
    fn dispatch_produced(self, state: &mut S, modules: &mut M) {
        for event in self.head.into_iter().flatten() {
            let produced = modules.process(state, &event);
            produced.dispatch_produced(state, modules);
            modules.dispatch(state, &event);
        }
        self.tail.dispatch_produced(state, modules);
    }
}

impl<N, S, M, H, T, Tail> DispatchProduced<Succ<N>, S, M> for HCons<HCons<H, T>, Tail>
where
    HCons<H, T>: DispatchProduced<Succ<N>, S, M>,
    Tail: DispatchProduced<Succ<N>, S, M>,
{
    fn dispatch_produced(self, state: &mut S, modules: &mut M) {
        self.head.dispatch_produced(state, modules);
        self.tail.dispatch_produced(state, modules);
    }
}
