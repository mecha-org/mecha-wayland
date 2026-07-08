use app::{RegisteredModule, prelude::*};
use wayland::WlDisplayRequest;

use crate::Compositor;

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new().on(|compositor: &mut Compositor, ev: &WlDisplayRequest| {
        if let WlDisplayRequest::Sync {
            client_id,
            callback,
            ..
        } = ev
        {
            compositor
                .server
                .set_pending_sync(*client_id, callback.clone());
        }
        hlist![]
    })
}
