use std::time::Instant;

use event_manager::{Builder, Component, EventContext, EventHandler};

pub struct WaitUntil(pub Instant);

pub struct Waiter;

impl EventHandler<WaitUntil> for Waiter {
    fn handle(&mut self, event: &WaitUntil, _ctx: &EventContext) {
        let now = Instant::now();
        if event.0 > now {
            std::thread::sleep(event.0 - now);
        }
    }
}

impl Component for Waiter {
    fn register(self, builder: &mut Builder) {
        builder.subscribe::<(WaitUntil,)>(self);
    }
}
