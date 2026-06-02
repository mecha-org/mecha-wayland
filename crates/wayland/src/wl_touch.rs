use app::Event;

use crate::proto::wl_touch as proto;
use crate::proto::Handle;
use crate::{SharedConnection, WaylandRawEvent, parse};

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
    _conn: SharedConnection,
    handle: Handle<proto::WlTouch>,
}

impl WlTouch {
    pub fn new(conn: SharedConnection) -> Self {
        Self { _conn: conn, handle: Handle::new(0) }
    }

    pub fn id(&self) -> u32 {
        self.handle.id
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<TouchEvent> {
        if raw.sender_id != self.handle.id {
            return None;
        }
        let ev = if let Some(e) = parse::<proto::event::Down>(raw) {
            TouchEvent::Down { serial: e.serial, time: e.time, surface: e.surface, id: e.id, x: e.x, y: e.y }
        } else if let Some(e) = parse::<proto::event::Up>(raw) {
            TouchEvent::Up { serial: e.serial, time: e.time, id: e.id }
        } else if let Some(e) = parse::<proto::event::Motion>(raw) {
            TouchEvent::Motion { time: e.time, id: e.id, x: e.x, y: e.y }
        } else if parse::<proto::event::Frame>(raw).is_some() {
            TouchEvent::Frame
        } else if parse::<proto::event::Cancel>(raw).is_some() {
            TouchEvent::Cancel
        } else if let Some(e) = parse::<proto::event::Shape>(raw) {
            TouchEvent::Shape { id: e.id, major: e.major, minor: e.minor }
        } else if let Some(e) = parse::<proto::event::Orientation>(raw) {
            TouchEvent::Orientation { id: e.id, orientation: e.orientation }
        } else {
            return None;
        };
        println!("[wl_touch] {:?}", ev);
        Some(ev)
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<WlTouch, AppState> {
    app::Module::<WlTouch, _, _>::new()
        .on(|s: &mut WlTouch, ev: &crate::WaylandRawEvent| s.process(ev))
}
