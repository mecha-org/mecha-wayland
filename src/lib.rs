//! The `mecha-wayland` facade: one crate to depend on, one prelude to import.
//! Each member crate's prelude is folded into [`prelude`] as it is rewritten
//! against the new `app` core.

pub use dbus;
pub use mechanix_widgets;

pub mod prelude {
    pub use animation::prelude::*;
    pub use app::prelude::*;
    pub use atlas::prelude::*;
    pub use dbus::prelude::*;
    pub use geometry::prelude::*;
    pub use gles::prelude::*;
    pub use interactivity::prelude::*;
    pub use layout::prelude::*;
    pub use paint::prelude::*;
    pub use presentation::prelude::*;
    pub use render::prelude::*;
    pub use ring::prelude::*;
    pub use scroll::prelude::*;
    pub use text_input::prelude::*;
    pub use toplevels::prelude::*;
    pub use theme::prelude::*;
    pub use timer::prelude::*;
    pub use wayland::prelude::*;
    pub use widgets::prelude::*;
    pub use window::prelude::*;
}
