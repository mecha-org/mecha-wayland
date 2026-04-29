use crate::object::Object;
use crate::{WlSeat, WlSeatHandler};

const CAP_POINTER: u32 = 1;
const CAP_KEYBOARD: u32 = 2;
const CAP_TOUCH: u32 = 4;

pub struct Seat {
    pub inner: WlSeat,

    pub has_pointer: bool,
    pub has_keyboard: bool,
    pub has_touch: bool,
    pub name: String,
}

impl Seat {
    pub fn new(object_id: u32) -> Self {
        Seat {
            inner: WlSeat::new(object_id),
            has_pointer: false,
            has_keyboard: false,
            has_touch: false,
            name: String::new(),
        }
    }
}

impl Object for Seat {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl WlSeatHandler for Seat {
    fn on_capabilities(&mut self, event: crate::WlSeatCapabilitiesEvent) {
        let caps = event.capabilities;

        self.has_pointer = (caps & CAP_POINTER) != 0;
        self.has_keyboard = (caps & CAP_KEYBOARD) != 0;
        self.has_touch = (caps & CAP_TOUCH) != 0;

        tracing::info!(
            pointer = self.has_pointer,
            keyboard = self.has_keyboard,
            touch = self.has_touch,
            "wl_seat capability updated"
        );
    }

    fn on_name(&mut self, event: crate::WlSeatNameEvent) {
        tracing::info!(name = %event.name, "wl_seat name received");
        self.name = event.name;
    }
}
