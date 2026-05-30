//! Internal dispatch machinery.
//!
//! This module contains the traits and HList impls that drive event dispatch
//! and propagation. None of these types are part of the public API.
//!
//! ## How dispatch works
//!
//! [`ModuleList`] is the top-level entry point. When [`App::dispatch`] is
//! called it delegates to `ModuleList::dispatch`, which iterates over each
//! mounted module via `dispatch_inner`. For every module:
//!
//! 1. The lens extracts the child state pointer.
//! 2. [`HandleList::handle`] runs all matching handlers, collecting emitted
//!    event outputs into an `Emitted` HList.
//! 3. [`Propagate::propagate`] re-dispatches each emitted event back through
//!    the root `ModuleList` (depth-first, so chains resolve inline).
//! 4. [`OuterDispatch`] does the same walk for sub-modules, but keeps a
//!    reference to the *outer* root so emitted events escape to the right scope.
//!
//! The raw-pointer dance in `dispatch_inner` / `dispatch_outer` is required
//! because the lens borrows `state` while propagation also needs `&mut state`.
//! The SAFETY comment at each site justifies the disjointness.

use std::any::TypeId;
#[cfg(feature = "tracing")]
use std::cell::Cell;
use std::marker::PhantomData;

#[cfg(feature = "tracing")]
thread_local! {
    static DISPATCH_DEPTH: Cell<usize> = const { Cell::new(0) };
}

#[cfg(feature = "tracing")]
fn log_dispatch<E: Event>(event: &E) {
    DISPATCH_DEPTH.with(|d| {
        let depth = d.get();
        eprintln!("{:indent$}[depth={depth}] Dispatched {:?}", "", event, indent = depth * 2);
    });
}

use frunk::{HCons, HNil};

use crate::event::{Emit, Event, Many};
use crate::module::Module;

/// Sealed trait implemented by the HList of modules attached to an [`App`](crate::App).
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

impl<S, SubState, Emitted, Handlers, LensFn, SubModules, Tail> ModuleList<S>
    for HCons<MountedModule<S, SubState, Emitted, Handlers, LensFn, SubModules>, Tail>
where
    Handlers: HandleList<SubState, Emitted>,
    Emitted: Propagate<S>,
    LensFn: Fn(&mut S) -> &mut SubState,
    SubModules: OuterDispatch<SubState, S>,
    Tail: ModuleList<S>,
{
    #[inline(always)]
    fn dispatch<E: Event>(&self, event: &E, state: &mut S) {
        #[cfg(feature = "tracing")]
        {
            log_dispatch(event);
            DISPATCH_DEPTH.with(|d| d.set(d.get() + 1));
        }
        self.dispatch_inner(event, state, self);
        #[cfg(feature = "tracing")]
        DISPATCH_DEPTH.with(|d| d.set(d.get() - 1));
    }

    #[inline(always)]
    fn dispatch_inner<E: Event, Root: ModuleList<S>>(&self, event: &E, state: &mut S, root: &Root) {
        // SAFETY: lens returns a reference to a subfield of state. Casting to raw pointer
        // releases the borrow on state, letting us pass state separately to propagate and
        // dispatch_outer. sub_ptr and state are guaranteed disjoint by the lens contract.
        let sub_ptr: *mut SubState = (self.head.lens)(state);
        let emitted = self.head.module.handlers.handle(event, unsafe { &mut *sub_ptr });
        emitted.propagate(root, state);
        self.head.module.sub_modules.dispatch_outer(event, unsafe { &mut *sub_ptr }, root, state);
        self.tail.dispatch_inner(event, state, root);
    }
}

/// Like [`ModuleList`] but for sub-modules: carries a reference to the
/// *outer* root so emitted events propagate beyond the current module tree.
pub(crate) trait OuterDispatch<S, OuterS> {
    fn dispatch_outer<E: Event, Root: ModuleList<OuterS>>(
        &self,
        event: &E,
        state: &mut S,
        outer_root: &Root,
        outer_state: &mut OuterS,
    );
}

impl<S, OuterS> OuterDispatch<S, OuterS> for HNil {
    #[inline(always)]
    fn dispatch_outer<E: Event, Root: ModuleList<OuterS>>(
        &self, _: &E, _: &mut S, _: &Root, _: &mut OuterS,
    ) {}
}

impl<S, OuterS, ChildState, ChildEmitted, ChildHandlers, ChildLens, ChildSubModules, Tail>
    OuterDispatch<S, OuterS>
    for HCons<MountedModule<S, ChildState, ChildEmitted, ChildHandlers, ChildLens, ChildSubModules>, Tail>
where
    ChildHandlers: HandleList<ChildState, ChildEmitted>,
    ChildEmitted: Propagate<OuterS>,
    ChildLens: Fn(&mut S) -> &mut ChildState,
    ChildSubModules: OuterDispatch<ChildState, OuterS>,
    Tail: OuterDispatch<S, OuterS>,
{
    #[inline(always)]
    fn dispatch_outer<E: Event, Root: ModuleList<OuterS>>(
        &self,
        event: &E,
        state: &mut S,
        outer_root: &Root,
        outer_state: &mut OuterS,
    ) {
        // SAFETY: same disjoint-subfield contract as dispatch_inner.
        let child_ptr: *mut ChildState = (self.head.lens)(state);
        let emitted = self.head.module.handlers.handle(event, unsafe { &mut *child_ptr });
        emitted.propagate(outer_root, outer_state);
        self.head.module.sub_modules.dispatch_outer(event, unsafe { &mut *child_ptr }, outer_root, outer_state);
        self.tail.dispatch_outer(event, state, outer_root, outer_state);
    }
}

/// Re-dispatches events collected from handler return values.
///
/// Implemented for `()`, `HNil`, `HCons<Option<E>, Tail>`,
/// `HCons<Many<Iter>, Tail>`, and their `Option<Many<…>>` variants.
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

/// Iterates over an HList of [`Handler`]s, running any whose registered event
/// type matches the dispatched event (compared by [`TypeId`]).
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
pub struct MountedModule<S, SubState, Emitted, Handlers, LensFn, SubModules = HNil> {
    pub(crate) lens: LensFn,
    pub(crate) module: Module<SubState, Emitted, Handlers, SubModules>,
    pub(crate) _phantom: PhantomData<(S, Emitted)>,
}
