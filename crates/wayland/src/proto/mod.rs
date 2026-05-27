use crate::wire::{MessageBuilder, MessageReader};
use std::marker::PhantomData;

pub trait WaylandInterface {
    const NAME: &'static str;
    const VERSION: u32;
}

pub trait WaylandSend {
    type Interface: WaylandInterface;
    const OPCODE: u16;
    fn serialize(&self, builder: MessageBuilder);
}

pub trait WaylandParse: Sized {
    const OPCODE: u16;
    fn deserialize(body: &[u8]) -> Option<Self>;
}

pub struct Handle<T: WaylandInterface> {
    pub id: u32,
    _marker: PhantomData<T>,
}

impl<T: WaylandInterface> Handle<T> {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }
}

impl<T: WaylandInterface> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self::new(self.id)
    }
}

impl<T: WaylandInterface> Copy for Handle<T> {}

// Include the generated protocol submodules
include!(concat!(env!("OUT_DIR"), "/protocols.rs"));
