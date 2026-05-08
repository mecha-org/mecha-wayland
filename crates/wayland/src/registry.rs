use crate::{
    connection::WlEvent,
    object::{WlObject, WlObjectHandle, WlObjectProxy},
};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    hash::Hash,
};

type ObjectId = u32;
type BoxedSender = Box<dyn Fn(&dyn Any) + Send + Sync>;

pub struct ObjectRegistry {
    objects: HashMap<ObjectId, Box<dyn WlObjectHandle>>,
    next_id: ObjectId,
    free: Vec<ObjectId>,
}

impl ObjectRegistry {
    pub fn new() -> Self {
        Self {
            objects: HashMap::new(),
            next_id: 1, // start after reserved IDs
            free: Vec::new(),
        }
    }

    pub fn alloc(&mut self) -> ObjectId {
        if let Some(id) = self.free.pop() {
            return id;
        }
        let id = self.next_id;
        println!("Allocating object ID: {}", id);
        self.next_id = self.next_id.checked_add(1).expect("object ID overflow");
        id
    }

    pub fn remove(&mut self, id: ObjectId) {
        self.free.push(id);
    }

    pub fn create<T: WlObject>(&mut self) -> T::Proxy {
        let id = self.alloc();
        let (obj, proxy) = T::spawn(id);

        self.objects.insert(id, Box::new(obj));

        proxy
    }

    fn dispatch(&self, object_id: ObjectId, opcode: u16, args: &[u8]) -> std::io::Result<()> {
        if let Some(obj) = self.objects.get(&object_id) {
            obj.dispatch(opcode, args)?;
        }
        Ok(())
    }

    pub fn dispatch_all(&self, events: Vec<WlEvent>) -> std::io::Result<()> {
        for event in events {
            self.dispatch(event.object_id, event.opcode, &event.args)?;
        }
        Ok(())
    }

    pub fn get<T: WlObject>(&self, object_id: ObjectId) -> Option<&T> {
        self.objects.get(&object_id)?.as_any().downcast_ref::<T>()
    }
}
