use frunk::{HCons, HNil};
use std::marker::PhantomData;

use crate::{
    event::{DispatchEvent, Event},
    module::{IsModule, MountedModule},
};

pub mod event;
pub mod module;

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
}
