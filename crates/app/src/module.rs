use crate::event::{Event, TypedHandler};
use frunk::{HCons, HNil};
use std::marker::PhantomData;

pub struct Module<T, Handlers = HNil> {
    pub handlers: Handlers,
    _marker: PhantomData<fn(&mut T)>,
}

impl<T> Module<T> {
    pub fn new() -> Self {
        Module {
            handlers: HNil,
            _marker: PhantomData,
        }
    }
}

impl<T, H> Module<T, H> {
    pub fn on<E, F>(self, handler: F) -> Module<T, HCons<TypedHandler<T, E, F>, H>>
    where
        E: Event,
        F: Fn(&mut T, &E) + 'static,
    {
        Module {
            handlers: HCons {
                head: TypedHandler::new(handler),
                tail: self.handlers,
            },
            _marker: PhantomData,
        }
    }

    pub fn submodule<U, L, H2>(
        self,
        lens: L,
        module: Module<U, H2>,
    ) -> Module<T, HCons<MountedModule<T, U, L, H2>, H>>
    where
        L: Fn(&mut T) -> &mut U,
    {
        Module {
            handlers: HCons {
                head: MountedModule {
                    lens,
                    handlers: module.handlers,
                    _marker: PhantomData,
                },
                tail: self.handlers,
            },
            _marker: PhantomData,
        }
    }
}

pub trait IsModule<T> {
    type Handlers;
    fn into_handlers(self) -> Self::Handlers;
}

impl<T, H> IsModule<T> for Module<T, H> {
    type Handlers = H;
    fn into_handlers(self) -> H {
        self.handlers
    }
}

pub struct MountedModule<S, T, L, Handlers>
where
    L: Fn(&mut S) -> &mut T,
{
    pub(crate) lens: L,
    pub(crate) handlers: Handlers,
    pub(crate) _marker: PhantomData<fn(&mut S, &mut T)>,
}
