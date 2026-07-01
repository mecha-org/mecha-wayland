use std::any::Any;
use std::marker::PhantomData;

use frunk::{HCons, HNil};

use crate::event::Event;
use crate::module::{Lens, RegisteredModule};

type DynHandler<S> = Box<dyn Fn(&mut S, &dyn Any, &mut Vec<Box<dyn Any>>)>;

pub struct App<S, Modules> {
    pub(crate) state: S,
    handlers: Vec<DynHandler<S>>,
    stack: Vec<Box<dyn Any>>,
    _phantom: PhantomData<fn() -> Modules>,
}

impl<S> App<S, HNil> {
    pub fn new(state: S) -> Self {
        Self {
            state,
            handlers: Vec::new(),
            stack: Vec::new(),
            _phantom: PhantomData,
        }
    }
}

impl<S, Modules> App<S, Modules> {
    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn mount<SubState: 'static, M>(
        self,
        module: M,
    ) -> App<S, HCons<(), Modules>>
    where
        S: Lens<SubState> + 'static,
        M: RegisteredModule<SubState, S>,
    {
        let sub_handlers = module.into_handlers();
        let mounted: DynHandler<S> = Box::new(move |state, event, stack| {
            let sub_ptr: *mut SubState = state.lens();
            for h in &sub_handlers {
                h(unsafe { &mut *sub_ptr }, event, stack);
            }
        });
        let mut handlers = self.handlers;
        handlers.push(mounted);
        App {
            state: self.state,
            handlers,
            stack: self.stack,
            _phantom: PhantomData,
        }
    }

    pub fn dispatch<E: Event>(&mut self, event: &E) {
        let App { state, handlers, stack, .. } = self;
        for h in handlers.iter() {
            h(state, event as &dyn Any, stack);
        }
        while let Some(boxed) = stack.pop() {
            for h in handlers.iter() {
                h(state, &*boxed, stack);
            }
        }
    }
}
