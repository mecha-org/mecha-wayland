#![recursion_limit = "512"]

use frunk::{HCons, HNil};
use std::marker::PhantomData;

use crate::{
    event::{DispatchEvent, DispatchProduced, Event, MaxDepth, ProcessHandlers},
    module::{IsModule, MountedModule},
};

pub mod event;
pub mod module;

pub struct PrePoll;
impl Event for PrePoll {}

pub struct Poll;
impl Event for Poll {}

pub struct PostPoll;
impl Event for PostPoll {}

pub struct End;
impl Event for End {}

pub struct Start;
impl Event for Start {}

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
        Modules: DispatchEvent<S, E> + ProcessHandlers<S, E>,
        <Modules as ProcessHandlers<S, E>>::Out: DispatchProduced<MaxDepth, S, Modules>,
    {
        let produced = self.modules.process(&mut self.state, &event);
        produced.dispatch_produced(&mut self.state, &mut self.modules);
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
        Modules: DispatchEvent<S, Start> + ProcessHandlers<S, Start>,
        <Modules as ProcessHandlers<S, Start>>::Out: DispatchProduced<MaxDepth, S, Modules>,
        Modules: DispatchEvent<S, PrePoll> + ProcessHandlers<S, PrePoll>,
        <Modules as ProcessHandlers<S, PrePoll>>::Out: DispatchProduced<MaxDepth, S, Modules>,
        Modules: DispatchEvent<S, Poll> + ProcessHandlers<S, Poll>,
        <Modules as ProcessHandlers<S, Poll>>::Out: DispatchProduced<MaxDepth, S, Modules>,
        Modules: DispatchEvent<S, PostPoll> + ProcessHandlers<S, PostPoll>,
        <Modules as ProcessHandlers<S, PostPoll>>::Out: DispatchProduced<MaxDepth, S, Modules>,
        Modules: DispatchEvent<S, End> + ProcessHandlers<S, End>,
        <Modules as ProcessHandlers<S, End>>::Out: DispatchProduced<MaxDepth, S, Modules>,
    {
        self.dispatch(Start);
        loop {
            self.dispatch(PrePoll);
            self.dispatch(Poll);
            self.dispatch(PostPoll);
        }
        self.dispatch(End);
    }
}
