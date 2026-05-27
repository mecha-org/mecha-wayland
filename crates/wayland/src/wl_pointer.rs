use app::event::Event;

use crate::proto::Handle;
use crate::proto::wl_pointer as proto;
use crate::{SharedConnection, WaylandRawEvent, parse};

pub type ButtonState = crate::proto::wl_pointer::WlPointerButtonState;
pub type PointerAxis = crate::proto::wl_pointer::WlPointerAxis;
pub type PointerAxisSource = crate::proto::wl_pointer::WlPointerAxisSource;

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
        state: ButtonState,
    },
    Axis {
        time: u32,
        axis: PointerAxis,
        value: f64,
    },
    Frame,
    AxisSource {
        axis_source: PointerAxisSource,
    },
    AxisStop {
        time: u32,
        axis: PointerAxis,
    },
    AxisDiscrete {
        axis: PointerAxis,
        discrete: i32,
    },
}

impl Event for PointerEvent {}

pub struct WlPointer {
    _conn: SharedConnection,
    handle: Handle<proto::WlPointer>,
}

impl WlPointer {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            _conn: conn,
            handle: Handle::new(0),
        }
    }

    pub fn id(&self) -> u32 {
        self.handle.id
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<PointerEvent> {
        if raw.sender_id != self.handle.id {
            return None;
        }
        let ev = if let Some(e) = parse::<proto::event::Enter>(raw) {
            PointerEvent::Enter {
                serial: e.serial,
                surface: e.surface,
                surface_x: e.surface_x,
                surface_y: e.surface_y,
            }
        } else if let Some(e) = parse::<proto::event::Leave>(raw) {
            PointerEvent::Leave {
                serial: e.serial,
                surface: e.surface,
            }
        } else if let Some(e) = parse::<proto::event::Motion>(raw) {
            PointerEvent::Motion {
                time: e.time,
                surface_x: e.surface_x,
                surface_y: e.surface_y,
            }
        } else if let Some(e) = parse::<proto::event::Button>(raw) {
            PointerEvent::Button {
                serial: e.serial,
                time: e.time,
                button: e.button,
                state: e.state,
            }
        } else if let Some(e) = parse::<proto::event::Axis>(raw) {
            PointerEvent::Axis {
                time: e.time,
                axis: e.axis,
                value: e.value,
            }
        } else if parse::<proto::event::Frame>(raw).is_some() {
            PointerEvent::Frame
        } else if let Some(e) = parse::<proto::event::AxisSource>(raw) {
            PointerEvent::AxisSource {
                axis_source: e.axis_source,
            }
        } else if let Some(e) = parse::<proto::event::AxisStop>(raw) {
            PointerEvent::AxisStop {
                time: e.time,
                axis: e.axis,
            }
        } else if let Some(e) = parse::<proto::event::AxisDiscrete>(raw) {
            PointerEvent::AxisDiscrete {
                axis: e.axis,
                discrete: e.discrete,
            }
        } else {
            return None;
        };
        println!("[wl_pointer] {:?}", ev);
        Some(ev)
    }
}

#[macro_export]
macro_rules! register_wl_pointer {
    () => {
        app::module::Module::<$crate::WlPointer>::new()
            .processor(|s: &mut $crate::WlPointer, ev: &$crate::WaylandRawEvent| s.process(ev))
    };
}
