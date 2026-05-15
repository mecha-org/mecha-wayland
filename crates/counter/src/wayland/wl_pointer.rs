use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

#[derive(Debug, Clone)]
pub enum PointerEvent {
    Enter {
        serial: u32,
        surface: u32,
        surface_x: f64,
        surface_y: f64,
    },
    Leave {
        serial: u32,
        surface: u32,
    },
    Motion {
        time: u32,
        surface_x: f64,
        surface_y: f64,
    },
    Button {
        serial: u32,
        time: u32,
        button: u32,
        state: u32,
    },
    Axis {
        time: u32,
        axis: u32,
        value: f64,
    },
    Frame,
    AxisSource {
        axis_source: u32,
    },
    AxisStop {
        time: u32,
        axis: u32,
    },
    AxisDiscrete {
        axis: u32,
        discrete: i32,
    },
}

impl Event for PointerEvent {}

pub struct WlPointer {
    conn: SharedConnection,
    pub id: u32,
}

impl WlPointer {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, id: 0 }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    pub fn set_cursor(&self, serial: u32, surface: u32, hotspot_x: i32, hotspot_y: i32) {
        self.conn
            .borrow_mut()
            .message_builder(self.id, 0)
            .write_u32(serial)
            .write_u32(surface)
            .write_i32(hotspot_x)
            .write_i32(hotspot_y)
            .build();
    }

    pub fn release(&self) {
        self.conn.borrow_mut().message_builder(self.id, 1).build();
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<PointerEvent> {
        if ev.sender_id != self.id {
            return None;
        }

        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);

        let event = match ev.opcode {
            0 => PointerEvent::Enter {
                serial: r.read_u32()?,
                surface: r.read_u32()?,
                surface_x: r.read_fixed()?,
                surface_y: r.read_fixed()?,
            },
            1 => PointerEvent::Leave {
                serial: r.read_u32()?,
                surface: r.read_u32()?,
            },
            2 => PointerEvent::Motion {
                time: r.read_u32()?,
                surface_x: r.read_fixed()?,
                surface_y: r.read_fixed()?,
            },
            3 => PointerEvent::Button {
                serial: r.read_u32()?,
                time: r.read_u32()?,
                button: r.read_u32()?,
                state: r.read_u32()?,
            },
            4 => PointerEvent::Axis {
                time: r.read_u32()?,
                axis: r.read_u32()?,
                value: r.read_fixed()?,
            },
            5 => PointerEvent::Frame,
            6 => PointerEvent::AxisSource {
                axis_source: r.read_u32()?,
            },
            7 => PointerEvent::AxisStop {
                time: r.read_u32()?,
                axis: r.read_u32()?,
            },
            8 => PointerEvent::AxisDiscrete {
                axis: r.read_u32()?,
                discrete: r.read_i32()?,
            },
            _ => return None,
        };

        println!("[wl_pointer] {:?}", event);
        Some(event)
    }
}

#[macro_export]
macro_rules! register_wl_pointer {
    () => {
        app::module::Module::<crate::wayland::WlPointer>::new().processor(
            |s: &mut crate::wayland::WlPointer, ev: &crate::wayland::WaylandRawEvent| s.process(ev),
        )
    };
}
