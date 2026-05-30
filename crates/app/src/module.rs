use std::marker::PhantomData;

use frunk::{HCons, HNil};

use crate::dispatch::{HandleList, Handler};
use crate::event::{Emit, Event};

pub struct Module<S, Emitted, Handlers> {
    pub(crate) handlers: Handlers,
    _phantom: PhantomData<(S, Emitted)>,
}

impl<S> Module<S, (), HNil> {
    pub fn new() -> Self {
        Self {
            handlers: HNil,
            _phantom: PhantomData,
        }
    }
}

impl<S, Emitted, Handlers: HandleList<S, Emitted>> Module<S, Emitted, Handlers> {
    pub fn on<E: Event, Ret: Emit>(
        self,
        f: impl Fn(&mut S, &E) -> Ret,
    ) -> Module<
        S,
        HCons<Ret::Output, Emitted>,
        HCons<Handler<S, E, Ret, impl Fn(&mut S, &E) -> Ret>, Handlers>,
    > {
        Module {
            handlers: HCons {
                head: Handler {
                    f,
                    _phantom: PhantomData,
                },
                tail: self.handlers,
            },
            _phantom: PhantomData,
        }
    }
}
