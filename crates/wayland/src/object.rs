pub trait Object {
    fn object_id(&self) -> u32;
}

impl<T: Object> Object for &T {
    fn object_id(&self) -> u32 {
        (*self).object_id()
    }
}
