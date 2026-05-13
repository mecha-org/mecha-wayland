use frunk::{HCons, HNil};
use std::marker::PhantomData;

use crate::{
    event::{DispatchEvent, DispatchProcessed, Event, ProcessHandlers},
    module::{IsModule, MountedModule},
};

pub mod event;
pub mod module;

pub struct Poll;
impl Event for Poll {}

pub struct App<S, Modules = HNil> {
    pub state: S,
    pub modules: Modules,
}

impl<S> App<S, HNil> {
    pub fn new(state: S) -> Self {
        App {
            state,
            modules: HNil,
        }
    }
}

impl<S, Modules> App<S, Modules> {
    pub fn register_module<T, L, M>(
        self,
        lens: L,
        module: M,
    ) -> App<S, HCons<MountedModule<S, T, L, M::Handlers>, Modules>>
    where
        L: Fn(&mut S) -> &mut T,
        M: IsModule<T>,
    {
        App {
            state: self.state,
            modules: HCons {
                head: MountedModule {
                    lens,
                    handlers: module.into_handlers(),
                    _marker: PhantomData,
                },
                tail: self.modules,
            },
        }
    }

    pub fn dispatch<E: Event>(&mut self, event: E)
    where
        Modules: DispatchEvent<S, E>,
    {
        self.modules.dispatch(&mut self.state, &event);
    }

    pub fn process<E: Event>(&mut self, event: E) -> <Modules as ProcessHandlers<S, E>>::Out
    where
        Modules: ProcessHandlers<S, E>,
    {
        self.modules.process(&mut self.state, &event)
    }

    pub fn run(&mut self)
    where
        Modules: DispatchEvent<S, Poll> + ProcessHandlers<S, Poll>,
        <Modules as ProcessHandlers<S, Poll>>::Out: DispatchProcessed<S, Modules>,
    {
        loop {
            let processed = self.modules.process(&mut self.state, &Poll);
            processed.dispatch_processed(&mut self.state, &mut self.modules);
            self.modules.dispatch(&mut self.state, &Poll);
        }
    }
}
