use std::any::Any;
use std::collections::VecDeque;
use std::marker::PhantomData;

use frunk::{HCons, HNil};

use crate::event::{Emit, Event, Many};

pub unsafe trait Lens<T> {
    fn lens(&mut self) -> &mut T;
}

unsafe impl<T> Lens<T> for T {
    fn lens(&mut self) -> &mut T {
        self
    }
}

pub(crate) type DynHandler<S> = Box<dyn Fn(&mut S, &dyn Any, &mut VecDeque<Box<dyn Any>>)>;

/// Converts a handler's return value into queue pushes.
#[allow(private_bounds)]
pub(crate) trait PushToStack {
    fn push_to_stack(self, queue: &mut VecDeque<Box<dyn Any>>);
}

impl<E: Event> PushToStack for E {
    fn push_to_stack(self, queue: &mut VecDeque<Box<dyn Any>>) {
        queue.push_back(Box::new(self));
    }
}

impl<E: Event> PushToStack for Option<E> {
    fn push_to_stack(self, queue: &mut VecDeque<Box<dyn Any>>) {
        if let Some(e) = self {
            queue.push_back(Box::new(e));
        }
    }
}

impl<Iter> PushToStack for Many<Iter>
where
    Iter: IntoIterator,
    Iter::Item: Event,
{
    fn push_to_stack(self, queue: &mut VecDeque<Box<dyn Any>>) {
        for e in self.0 {
            queue.push_back(Box::new(e));
        }
    }
}

impl<Iter> PushToStack for Option<Many<Iter>>
where
    Iter: IntoIterator,
    Iter::Item: Event,
{
    fn push_to_stack(self, queue: &mut VecDeque<Box<dyn Any>>) {
        if let Some(many) = self {
            for e in many.0 {
                queue.push_back(Box::new(e));
            }
        }
    }
}

impl PushToStack for HNil {
    fn push_to_stack(self, _: &mut VecDeque<Box<dyn Any>>) {}
}

impl PushToStack for Option<HNil> {
    fn push_to_stack(self, _: &mut VecDeque<Box<dyn Any>>) {}
}

impl<H: PushToStack, T: PushToStack> PushToStack for HCons<H, T> {
    fn push_to_stack(self, queue: &mut VecDeque<Box<dyn Any>>) {
        self.head.push_to_stack(queue);
        self.tail.push_to_stack(queue);
    }
}

impl<H: PushToStack, T: PushToStack> PushToStack for Option<HCons<H, T>> {
    fn push_to_stack(self, queue: &mut VecDeque<Box<dyn Any>>) {
        if let Some(hlist) = self {
            hlist.push_to_stack(queue);
        }
    }
}

pub trait RegisteredModule<S, AppState> {
    type Emitted;
    type Handlers;
    type SubModules;
    fn into_handlers(self) -> Vec<DynHandler<S>>;
    fn into_module(self) -> Module<S, Self::Emitted, Self::Handlers, Self::SubModules>
    where
        Self: Sized;
}

pub struct Module<S, Emitted, Handlers, SubModules = HNil> {
    pub(crate) handlers: Vec<DynHandler<S>>,
    _phantom: PhantomData<fn() -> (S, Emitted, Handlers, SubModules)>,
}

impl<S> Module<S, (), HNil, HNil> {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            _phantom: PhantomData,
        }
    }
}

impl<S, Emitted, Handlers, SubModules> Module<S, Emitted, Handlers, SubModules> {
    #[allow(private_bounds)]
    pub fn on<E: Event, Ret: Emit + PushToStack>(
        self,
        f: impl Fn(&mut S, &E) -> Ret + 'static,
    ) -> Module<S, HCons<Ret::Output, Emitted>, HCons<(), Handlers>, SubModules>
    where
        S: 'static,
    {
        let handler: DynHandler<S> = Box::new(move |state, event, queue| {
            if let Some(typed) = (event as &dyn Any).downcast_ref::<E>() {
                f(state, typed).push_to_stack(queue);
            }
        });
        let mut handlers = self.handlers;
        handlers.push(handler);
        Module {
            handlers,
            _phantom: PhantomData,
        }
    }

    pub fn mount<ChildState: 'static, CE, CH, CSM>(
        self,
        child: Module<ChildState, CE, CH, CSM>,
    ) -> Module<S, Emitted, Handlers, HCons<(), SubModules>>
    where
        S: Lens<ChildState> + 'static,
    {
        let child_handlers = child.handlers;
        let dispatcher: DynHandler<S> = Box::new(move |state, event, queue| {
            let child_ptr: *mut ChildState = state.lens();
            for h in &child_handlers {
                h(unsafe { &mut *child_ptr }, event, queue);
            }
        });
        let mut handlers = self.handlers;
        handlers.push(dispatcher);
        Module {
            handlers,
            _phantom: PhantomData,
        }
    }
}

impl<S: 'static, AppState, Emitted, Handlers, SubModules> RegisteredModule<S, AppState>
    for Module<S, Emitted, Handlers, SubModules>
{
    type Emitted = Emitted;
    type Handlers = Handlers;
    type SubModules = SubModules;

    fn into_handlers(self) -> Vec<DynHandler<S>> {
        self.handlers
    }

    fn into_module(self) -> Module<S, Emitted, Handlers, SubModules> {
        self
    }
}
