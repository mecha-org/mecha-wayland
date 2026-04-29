#![allow(unused_variables, unused_mut, dead_code)]

pub mod wl_callback;
pub mod wl_display;
pub mod wl_pointer;
pub mod wl_registry;
pub mod wl_seat;
pub mod wl_shm;
pub mod wl_touch;
pub mod xdg_surface;
pub mod xdg_toplevel;
pub mod xdg_wm_base;
pub mod zwp_linux_dmabuf;

// pub mod connection;
pub mod connection;
pub mod helpers;
pub mod object;
pub mod wire;
pub mod wlr_layer_shell;

include!(concat!(env!("OUT_DIR"), "/wayland_protocol.rs"));

/// Route a received Wayland event to the matching handler by object_id.
///
/// Usage: `dispatch_to!(conn, obj_id, opcode, &body; handler_a, handler_b, ...);`
/// Each handler must implement `.object_id()` (via `Object`) and `.dispatch(...)`.
#[macro_export]
macro_rules! dispatch_to {
    ($conn:expr, $obj_id:expr, $opcode:expr, $body:expr; $($handler:expr),+ $(,)?) => {{
        use $crate::object::Object as _;
        $(
            if $obj_id == $handler.object_id() {
                $handler.dispatch(&mut $conn, $opcode, $body)?;
            } else
        )+
        {
            tracing::debug!(obj_id = $obj_id, opcode = $opcode, "unhandled event");
        }
    }};
}

/// Route a received Wayland event to matching optional handlers (Option<T>) by object_id.
/// Returns true if the event was dispatched to one of the handlers.
///
/// Usage: `let handled = dispatch_optional!(conn, obj_id, opcode, &body; pointer, keyboard);`
#[macro_export]
macro_rules! dispatch_optional {
    ($conn:expr, $obj_id:expr, $opcode:expr, $body:expr; $($opt_handler:expr),+ $(,)?) => {{
        use $crate::object::Object as _;
        let mut _handled = false;
        $(
            if let Some(h) = &mut $opt_handler {
                if $obj_id == h.object_id() {
                    h.dispatch(&mut $conn, $opcode, $body)?;
                    _handled = true;
                }
            }
        )+
        _handled
    }};
}
