use std::any::TypeId;
use std::marker::PhantomData;

use frunk::{HCons, HNil};

pub trait Event: 'static {}
impl Event for () {}

// Monomorphic wrapper that binds handler to a specific (S, RegisteredEvent) pair.
// Without this, HCons<HandlerFn, T> can't constrain S and RegisteredEvent in its impl.
pub struct Handler<S, E, HandlerFn: Fn(&mut S, &E)> {
    f: HandlerFn,
    _phantom: PhantomData<fn(&mut S, &E)>,
}

pub struct EmitingHandler<S, E, Out, HandlerFn: Fn(&mut S, &E) -> Out> {
    f: HandlerFn,
    _phantom: PhantomData<fn(&mut S, &E) -> Out>,
}

pub struct App<S, Modules: Dispatcher<S>> {
    pub(crate) state: S,
    pub(crate) modules: Modules,
}

impl<S> App<S, HNil> {
    pub fn new(state: S) -> Self {
        Self {
            state,
            modules: HNil,
        }
    }
}

impl<S, Modules: Dispatcher<S>> App<S, Modules> {
    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn register_module<
        SubState,
        Out,
        Handlers: Dispatcher<SubState>,
        EmitingHandlers: EmitingDispatcher<SubState, Out>,
        LensFn,
    >(
        self,
        lens: LensFn,
        module: Module<SubState, Out, Handlers, EmitingHandlers>,
    ) -> App<S, HCons<RegisteredModule<S, SubState, Out, Handlers, EmitingHandlers, LensFn>, Modules>>
    where
        LensFn: Fn(&mut S) -> &mut SubState,
        HCons<RegisteredModule<S, SubState, Out, Handlers, EmitingHandlers, LensFn>, Modules>:
            Dispatcher<S>,
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

    pub fn dispatch<E: Event>(&mut self, event: &E)
    where
        Modules: FullDispatcher<S>,
    {
        FullDispatcher::dispatch(&mut self.modules, event, &mut self.state);
    }
}

pub trait FullDispatcher<S> {
    fn dispatch<E: Event>(&mut self, event: &E, state: &mut S);
}

impl<S> FullDispatcher<S> for HNil {
    fn dispatch<E: Event>(&mut self, _: &E, _: &mut S) {}
}

impl<
    S,
    SubState,
    Out,
    Handlers: Dispatcher<SubState>,
    EmitingHandlers: EmitingDispatcher<SubState, Out>,
    LensFn: Fn(&mut S) -> &mut SubState,
    Tail: FullDispatcher<S>,
> FullDispatcher<S>
    for HCons<RegisteredModule<S, SubState, Out, Handlers, EmitingHandlers, LensFn>, Tail>
where
    Out: DispatchEmitted<S>,
{
    fn dispatch<E: Event>(&mut self, event: &E, state: &mut S) {
        let sub = (self.head.lens)(state);
        self.head.module.handlers.dispatch(event, sub);
        let emitted = self.head.module.producing_handlers.dispatch(event, sub);
        emitted.dispatch(self, state);
        self.tail.dispatch(event, state);
    }
}

pub trait DispatchEmitted<S> {
    fn dispatch<D: FullDispatcher<S>>(self, dispatcher: &mut D, state: &mut S);
}

impl<S> DispatchEmitted<S> for () {
    fn dispatch<D: FullDispatcher<S>>(self, _: &mut D, _: &mut S) {}
}

impl<S> DispatchEmitted<S> for HNil {
    fn dispatch<D: FullDispatcher<S>>(self, _: &mut D, _: &mut S) {}
}

impl<S, E: Event, Tail: DispatchEmitted<S>> DispatchEmitted<S> for HCons<Option<E>, Tail> {
    fn dispatch<D: FullDispatcher<S>>(self, dispatcher: &mut D, state: &mut S) {
        if let Some(e) = self.head {
            FullDispatcher::dispatch(dispatcher, &e, state);
        }
        self.tail.dispatch(dispatcher, state);
    }
}

pub trait EmitingDispatcher<S, Out> {
    fn dispatch<E: Event>(&mut self, event: &E, state: &mut S) -> Out;
}

impl<S> EmitingDispatcher<S, ()> for HNil {
    fn dispatch<E: Event>(&mut self, _: &E, _: &mut S) {}
}

pub trait Dispatcher<S> {
    fn dispatch<E: Event>(&mut self, event: &E, state: &mut S);
}

impl<S> Dispatcher<S> for HNil {
    fn dispatch<E: Event>(&mut self, _: &E, _: &mut S) {}
}

impl<S, RegisteredEvent: Event, HandlerFn: Fn(&mut S, &RegisteredEvent), Tail: Dispatcher<S>>
    Dispatcher<S> for HCons<Handler<S, RegisteredEvent, HandlerFn>, Tail>
{
    fn dispatch<DispatchedEvent: Event>(&mut self, event: &DispatchedEvent, state: &mut S) {
        if TypeId::of::<DispatchedEvent>() == TypeId::of::<RegisteredEvent>() {
            // SAFETY: TypeId equality guarantees DispatchedEvent and RegisteredEvent are the same
            // type, so reinterpreting the pointer is sound.
            let e = unsafe { &*(event as *const DispatchedEvent as *const RegisteredEvent) };
            (self.head.f)(state, e);
        }
        self.tail.dispatch(event, state);
    }
}

impl<
    S,
    Out,
    EmitedEvent,
    RegisteredEvent: Event,
    HandlerFn: Fn(&mut S, &RegisteredEvent) -> EmitedEvent,
    Tail: EmitingDispatcher<S, Out>,
> EmitingDispatcher<S, HCons<Option<EmitedEvent>, Out>>
    for HCons<EmitingHandler<S, RegisteredEvent, EmitedEvent, HandlerFn>, Tail>
{
    fn dispatch<DispatchedEvent: Event>(
        &mut self,
        event: &DispatchedEvent,
        state: &mut S,
    ) -> HCons<Option<EmitedEvent>, Out> {
        let mut head = None;
        if TypeId::of::<DispatchedEvent>() == TypeId::of::<RegisteredEvent>() {
            // SAFETY: TypeId equality guarantees DispatchedEvent and RegisteredEvent are the same
            // type, so reinterpreting the pointer is sound.
            let e = unsafe { &*(event as *const DispatchedEvent as *const RegisteredEvent) };
            head = Some((self.head.f)(state, e));
        }

        HCons {
            head,
            tail: self.tail.dispatch(event, state),
        }
    }
}

impl<
    S,
    Out,
    SubState,
    Handlers: Dispatcher<SubState>,
    EmitingHandlers: EmitingDispatcher<SubState, Out>,
    LensFn: Fn(&mut S) -> &mut SubState,
    Tail: Dispatcher<S>,
> Dispatcher<S>
    for HCons<RegisteredModule<S, SubState, Out, Handlers, EmitingHandlers, LensFn>, Tail>
{
    fn dispatch<E: Event>(&mut self, event: &E, state: &mut S) {
        let sub = (self.head.lens)(state);
        self.head.module.handlers.dispatch(event, sub);
        self.tail.dispatch(event, state);
    }
}

impl<S, Out, Handlers: Dispatcher<S>, EmittingHandlers: EmitingDispatcher<S, Out>> Dispatcher<S>
    for Module<S, Out, Handlers, EmittingHandlers>
{
    fn dispatch<E: Event>(&mut self, event: &E, state: &mut S) {
        self.handlers.dispatch(event, state);
    }
}

pub struct RegisteredModule<
    S,
    SubState,
    Out,
    Handlers: Dispatcher<SubState>,
    EmitingHandlers: EmitingDispatcher<SubState, Out>,
    LensFn: Fn(&mut S) -> &mut SubState,
> {
    pub(crate) lens: LensFn,
    pub(crate) module: Module<SubState, Out, Handlers, EmitingHandlers>,
    pub(crate) _phantom: PhantomData<(S, Out)>,
}

pub struct Module<S, Out, Handlers: Dispatcher<S>, EmitingHandlers: EmitingDispatcher<S, Out>> {
    _phantom: PhantomData<(S, Out)>,
    producing_handlers: EmitingHandlers,
    handlers: Handlers,
}

impl<S> Module<S, (), HNil, HNil> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
            producing_handlers: HNil,
            handlers: HNil,
        }
    }
}

impl<S, Out, Handlers: Dispatcher<S>, EmitingHandlers: EmitingDispatcher<S, Out>>
    Module<S, Out, Handlers, EmitingHandlers>
{
    pub fn emit<E: Event, EmitedEvent: Event>(
        self,
        f: impl Fn(&mut S, &E) -> EmitedEvent,
    ) -> Module<
        S,
        HCons<Option<EmitedEvent>, Out>,
        Handlers,
        HCons<
            EmitingHandler<S, E, EmitedEvent, impl Fn(&mut S, &E) -> EmitedEvent>,
            EmitingHandlers,
        >,
    > {
        Module {
            _phantom: PhantomData,
            handlers: self.handlers,
            producing_handlers: HCons {
                head: EmitingHandler {
                    f,
                    _phantom: PhantomData,
                },
                tail: self.producing_handlers,
            },
        }
    }

    pub fn on<E: Event>(
        self,
        f: impl Fn(&mut S, &E),
    ) -> Module<S, Out, HCons<Handler<S, E, impl Fn(&mut S, &E)>, Handlers>, EmitingHandlers> {
        Module {
            _phantom: PhantomData,
            producing_handlers: self.producing_handlers,
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
