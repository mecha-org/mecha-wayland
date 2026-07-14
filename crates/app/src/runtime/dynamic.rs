use std::any::Any;
use std::collections::VecDeque;
use std::marker::PhantomData;

use frunk::{HCons, HNil};

use crate::event::Event;
use crate::module::{Lens, RegisteredModule};

type DynHandler<S> = Box<dyn Fn(&mut S, &dyn Any, &mut VecDeque<Box<dyn Any>>)>;

pub struct App<S, Modules> {
    pub(crate) state: S,
    handlers: Vec<DynHandler<S>>,
    queue: VecDeque<Box<dyn Any>>,
    _phantom: PhantomData<fn() -> Modules>,
}

impl<S> App<S, HNil> {
    pub fn new(state: S) -> Self {
        Self {
            state,
            handlers: Vec::new(),
            queue: VecDeque::new(),
            _phantom: PhantomData,
        }
    }
}

impl<S, Modules> App<S, Modules> {
    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn mount<SubState: 'static, M>(self, module: M) -> App<S, HCons<(), Modules>>
    where
        S: Lens<SubState> + 'static,
        M: RegisteredModule<SubState, S>,
    {
        let sub_handlers = module.into_handlers();
        let mounted: DynHandler<S> = Box::new(move |state, event, queue| {
            let sub_ptr: *mut SubState = state.lens();
            for h in &sub_handlers {
                h(unsafe { &mut *sub_ptr }, event, queue);
            }
        });
        let mut handlers = self.handlers;
        handlers.push(mounted);
        App {
            state: self.state,
            handlers,
            queue: self.queue,
            _phantom: PhantomData,
        }
    }

    pub fn dispatch<E: Event>(&mut self, event: &E) {
        let App {
            state,
            handlers,
            queue,
            ..
        } = self;
        for h in handlers.iter() {
            h(state, event as &dyn Any, queue);
        }
        while let Some(boxed) = queue.pop_front() {
            for h in handlers.iter() {
                h(state, &*boxed, queue);
            }
        }
    }
}
