use std::any::TypeId;
use std::marker::PhantomData;

use frunk::{HCons, HNil};

use crate::event::{Emit, Event, Many};
use crate::module::Module;

pub(crate) trait ModuleList<S> {
    fn dispatch<E: Event>(&self, event: &E, state: &mut S);
    fn dispatch_inner<E: Event, Root: ModuleList<S>>(&self, event: &E, state: &mut S, root: &Root);
}

impl<S> ModuleList<S> for HNil {
    #[inline(always)]
    fn dispatch<E: Event>(&self, _: &E, _: &mut S) {}
    #[inline(always)]
    fn dispatch_inner<E: Event, Root: ModuleList<S>>(&self, _: &E, _: &mut S, _: &Root) {}
}

impl<S, SubState, Emitted, Handlers, LensFn, Tail> ModuleList<S>
    for HCons<MountedModule<S, SubState, Emitted, Handlers, LensFn>, Tail>
where
    Handlers: HandleList<SubState, Emitted>,
    Emitted: Propagate<S>,
    LensFn: Fn(&mut S) -> &mut SubState,
    Tail: ModuleList<S>,
{
    #[inline(always)]
    fn dispatch<E: Event>(&self, event: &E, state: &mut S) {
        self.dispatch_inner(event, state, self);
    }

    #[inline(always)]
    fn dispatch_inner<E: Event, Root: ModuleList<S>>(&self, event: &E, state: &mut S, root: &Root) {
        let sub = (self.head.lens)(state);
        let emitted = self.head.module.handlers.handle(event, sub);
        emitted.propagate(root, state);
        self.tail.dispatch_inner(event, state, root);
    }
}

pub(crate) trait Propagate<S> {
    fn propagate<ML: ModuleList<S>>(self, root: &ML, state: &mut S);
}

impl<S> Propagate<S> for () {
    #[inline(always)]
    fn propagate<ML: ModuleList<S>>(self, _: &ML, _: &mut S) {}
}

impl<S> Propagate<S> for HNil {
    #[inline(always)]
    fn propagate<ML: ModuleList<S>>(self, _: &ML, _: &mut S) {}
}

impl<S, E: Event, Tail: Propagate<S>> Propagate<S> for HCons<Option<E>, Tail> {
    #[inline(always)]
    fn propagate<ML: ModuleList<S>>(self, root: &ML, state: &mut S) {
        if let Some(e) = self.head {
            root.dispatch(&e, state);
        }
        self.tail.propagate(root, state);
    }
}

impl<S, H, T, Tail: Propagate<S>> Propagate<S> for HCons<Option<HCons<H, T>>, Tail>
where
    HCons<H, T>: Propagate<S>,
{
    #[inline(always)]
    fn propagate<ML: ModuleList<S>>(self, root: &ML, state: &mut S) {
        if let Some(inner) = self.head {
            inner.propagate(root, state);
        }
        self.tail.propagate(root, state);
    }
}

impl<S, Iter, Tail: Propagate<S>> Propagate<S> for HCons<Many<Iter>, Tail>
where
    Iter: IntoIterator,
    Iter::Item: Event,
{
    #[inline(always)]
    fn propagate<ML: ModuleList<S>>(self, root: &ML, state: &mut S) {
        for e in self.head.0 {
            root.dispatch(&e, state);
        }
        self.tail.propagate(root, state);
    }
}

impl<S, Iter, Tail: Propagate<S>> Propagate<S> for HCons<Option<Many<Iter>>, Tail>
where
    Iter: IntoIterator,
    Iter::Item: Event,
{
    #[inline(always)]
    fn propagate<ML: ModuleList<S>>(self, root: &ML, state: &mut S) {
        if let Some(many) = self.head {
            for e in many.0 {
                root.dispatch(&e, state);
            }
        }
        self.tail.propagate(root, state);
    }
}

pub(crate) trait HandleList<S, Emitted> {
    fn handle<E: Event>(&self, event: &E, state: &mut S) -> Emitted;
}

impl<S> HandleList<S, ()> for HNil {
    #[inline(always)]
    fn handle<E: Event>(&self, _: &E, _: &mut S) {}
}

impl<S, RegisteredEvent: Event, Ret: Emit, F, Emitted, Tail>
    HandleList<S, HCons<Ret::Output, Emitted>>
    for HCons<Handler<S, RegisteredEvent, Ret, F>, Tail>
where
    F: Fn(&mut S, &RegisteredEvent) -> Ret,
    Tail: HandleList<S, Emitted>,
{
    #[inline(always)]
    fn handle<DispatchedEvent: Event>(
        &self,
        event: &DispatchedEvent,
        state: &mut S,
    ) -> HCons<Ret::Output, Emitted> {
        let head = if TypeId::of::<DispatchedEvent>() == TypeId::of::<RegisteredEvent>() {
            // SAFETY: TypeId equality guarantees DispatchedEvent and RegisteredEvent are the same
            // type, so reinterpreting the pointer is sound.
            let e = unsafe { &*(event as *const DispatchedEvent as *const RegisteredEvent) };
            Ret::emit((self.head.f)(state, e))
        } else {
            Ret::empty()
        };
        HCons {
            head,
            tail: self.tail.handle(event, state),
        }
    }
}

#[doc(hidden)]
pub struct Handler<S, E, Ret, F: Fn(&mut S, &E) -> Ret> {
    pub(crate) f: F,
    pub(crate) _phantom: PhantomData<fn(&mut S, &E) -> Ret>,
}

#[doc(hidden)]
pub struct MountedModule<S, SubState, Emitted, Handlers, LensFn> {
    pub(crate) lens: LensFn,
    pub(crate) module: Module<SubState, Emitted, Handlers>,
    pub(crate) _phantom: PhantomData<(S, Emitted)>,
}
