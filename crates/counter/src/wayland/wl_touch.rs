use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

#[derive(Debug, Clone)]
pub enum TouchEvent {
    Down { serial: u32, time: u32, surface: u32, id: i32, x: f64, y: f64 },
    Up { serial: u32, time: u32, id: i32 },
    Motion { time: u32, id: i32, x: f64, y: f64 },
    Frame,
    Cancel,
    Shape { id: i32, major: f64, minor: f64 },
    Orientation { id: i32, orientation: f64 },
}

impl Event for TouchEvent {}

pub struct WlTouch {
    conn: SharedConnection,
    pub id: u32,
}

impl WlTouch {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            id: 0,
        }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    pub fn release(&self) {
        self.conn.borrow_mut().message_builder(self.id, 0).build();
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<TouchEvent> {
        if ev.sender_id != self.id {
            return None;
        }

        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);

        let event = match ev.opcode {
            0 => TouchEvent::Down {
                serial: r.read_u32()?,
                time: r.read_u32()?,
                surface: r.read_u32()?,
                id: r.read_i32()?,
                x: r.read_fixed()?,
                y: r.read_fixed()?,
            },
            1 => TouchEvent::Up {
                serial: r.read_u32()?,
                time: r.read_u32()?,
                id: r.read_i32()?,
            },
            2 => TouchEvent::Motion {
                time: r.read_u32()?,
                id: r.read_i32()?,
                x: r.read_fixed()?,
                y: r.read_fixed()?,
            },
            3 => TouchEvent::Frame,
            4 => TouchEvent::Cancel,
            5 => TouchEvent::Shape {
                id: r.read_i32()?,
                major: r.read_fixed()?,
                minor: r.read_fixed()?,
            },
            6 => TouchEvent::Orientation {
                id: r.read_i32()?,
                orientation: r.read_fixed()?,
            },
            _ => return None,
        };

        println!("[wl_touch] {:?}", event);
        Some(event)
    }
}

#[macro_export]
macro_rules! register_wl_touch {
    () => {
        app::module::Module::<crate::wayland::WlTouch>::new()
            .processor(|s: &mut crate::wayland::WlTouch, ev: &crate::wayland::WaylandRawEvent| {
                s.process(ev)
            })
    };
}
