use app::{RegisteredModule, prelude::*};

use crate::Compositor;

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
}
