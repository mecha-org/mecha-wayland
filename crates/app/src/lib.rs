mod dispatch;
mod event;
mod module;
mod runtime;

pub use dispatch::{Handler, MountedModule};
pub use event::{Emit, Event, Many};
pub use module::Module;
pub use runtime::App;

pub mod prelude {
    pub use crate::{App, Event, Many, Module};
    pub use frunk::hlist;
}
