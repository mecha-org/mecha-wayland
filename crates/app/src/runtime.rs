use std::marker::PhantomData;

use frunk::{HCons, HNil};

use crate::dispatch::{HandleList, ModuleList, MountedModule, Propagate};
use crate::event::Event;
use crate::module::Module;

pub struct App<S, Modules: ModuleList<S>> {
    pub(crate) state: S,
    pub(crate) modules: Modules,
}

impl<S> App<S, HNil> {
    pub fn new(state: S) -> Self {
        Self { state, modules: HNil }
    }
}

impl<S, Modules: ModuleList<S>> App<S, Modules> {
    pub fn state(&self) -> &S {
        &self.state
    }

    #[allow(private_bounds)]
    pub fn mount<SubState, Emitted, Handlers, LensFn>(
        self,
        lens: LensFn,
        module: Module<SubState, Emitted, Handlers>,
    ) -> App<S, HCons<MountedModule<S, SubState, Emitted, Handlers, LensFn>, Modules>>
    where
        LensFn: Fn(&mut S) -> &mut SubState,
        Handlers: HandleList<SubState, Emitted>,
        Emitted: Propagate<S>,
        HCons<MountedModule<S, SubState, Emitted, Handlers, LensFn>, Modules>: ModuleList<S>,
    {
        App {
            state: self.state,
            modules: HCons {
                head: MountedModule {
                    lens,
                    module,
                    _phantom: PhantomData,
                },
                tail: self.modules,
            },
        }
    }

    pub fn dispatch<E: Event>(&mut self, event: &E)
    where
        Modules: ModuleList<S>,
    {
        ModuleList::dispatch(&mut self.modules, event, &mut self.state);
    }
}
