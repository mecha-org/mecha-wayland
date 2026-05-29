use std::any::TypeId;
use std::marker::PhantomData;

use frunk::{HCons, HNil};

pub trait Event: 'static {}

// Monomorphic wrapper that binds handler to a specific (S, E) pair.
// Without this, HCons<F, T> can't constrain S and E1 in its impl.
pub struct Handler<S, E, F: Fn(&mut S, &E)> {
    f: F,
    _phantom: PhantomData<fn(&mut S, &E)>,
}

pub struct App<S, M: Dispatcher<S>> {
    pub(crate) state: S,
    pub(crate) modules: M,
}

impl<S> App<S, HNil> {
    pub fn new(state: S) -> Self {
        Self {
            state,
            modules: HNil,
        }
    }
}

impl<S, M: Dispatcher<S>> App<S, M> {
    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn register_module<T, H: Dispatcher<T>, L>(
        self,
        lens: L,
        module: Module<T, H>,
    ) -> App<S, HCons<RegisteredModule<S, T, H, L>, M>>
    where
        L: Fn(&mut S) -> &mut T,
        HCons<RegisteredModule<S, T, H, L>, M>: Dispatcher<S>,
    {
        App {
            state: self.state,
            modules: HCons {
                head: RegisteredModule {
                    lens,
                    module,
                    _phantom: PhantomData,
                },
                tail: self.modules,
            },
        }
    }

    pub fn dispatch<E: Event>(&mut self, event: &E) {
        self.modules.dispatch(event, &mut self.state);
    }
}

pub trait Dispatcher<S> {
    fn dispatch<E: Event>(&mut self, event: &E, state: &mut S);
}

impl<S> Dispatcher<S> for HNil {
    fn dispatch<E: Event>(&mut self, _: &E, _: &mut S) {}
}

impl<S, E1: Event, F: Fn(&mut S, &E1), Tail: Dispatcher<S>> Dispatcher<S>
    for HCons<Handler<S, E1, F>, Tail>
{
    fn dispatch<E2: Event>(&mut self, event: &E2, state: &mut S) {
        if TypeId::of::<E2>() == TypeId::of::<E1>() {
            // SAFETY: TypeId equality guarantees E2 and E1 are the same type,
            // so reinterpreting the pointer is sound.
            let e = unsafe { &*(event as *const E2 as *const E1) };
            (self.head.f)(state, e);
        }
        self.tail.dispatch(event, state);
    }
}

impl<S, T, H: Dispatcher<T>, L: Fn(&mut S) -> &mut T, Tail: Dispatcher<S>> Dispatcher<S>
    for HCons<RegisteredModule<S, T, H, L>, Tail>
{
    fn dispatch<E: Event>(&mut self, event: &E, state: &mut S) {
        let sub = (self.head.lens)(state);
        self.head.module.handlers.dispatch(event, sub);
        self.tail.dispatch(event, state);
    }
}

impl<S, H: Dispatcher<S>> Dispatcher<S> for Module<S, H> {
    fn dispatch<E: Event>(&mut self, event: &E, state: &mut S) {
        self.handlers.dispatch(event, state);
    }
}

pub struct RegisteredModule<S, T, H: Dispatcher<T>, L: Fn(&mut S) -> &mut T> {
    pub(crate) lens: L,
    pub(crate) module: Module<T, H>,
    pub(crate) _phantom: PhantomData<S>,
}

pub struct Module<S, H: Dispatcher<S>> {
    _phantom: PhantomData<S>,
    handlers: H,
}

impl<S> Module<S, HNil> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
            handlers: HNil,
        }
    }
}

impl<S, H: Dispatcher<S>> Module<S, H> {
    pub fn on<E: Event>(
        self,
        f: impl Fn(&mut S, &E),
    ) -> Module<S, HCons<Handler<S, E, impl Fn(&mut S, &E)>, H>> {
        Module {
            _phantom: PhantomData,
            handlers: HCons {
                head: Handler {
                    f,
                    _phantom: PhantomData,
                },
                tail: self.handlers,
            },
        }
    }
}
