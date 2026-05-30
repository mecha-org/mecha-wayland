//! A typed, event-driven application runtime.
//!
//! The core idea is simple: your application state is a plain Rust struct. You
//! partition it into *modules*, each owning a slice of that state and declaring
//! which events it cares about. When you call [`App::dispatch`] with an event,
//! every matching handler runs. Handlers can themselves emit new events, which
//! the runtime re-dispatches automatically — no manual wiring needed.
//!
//! # Building blocks
//!
//! | Type | Role |
//! |------|------|
//! | [`Event`] | Marker trait — any `'static` type can be an event |
//! | [`Module`] | A set of handlers over a slice of state |
//! | [`App`] | Top-level runtime: owns state and a list of mounted modules |
//! | [`Many`] | Emit a dynamic number of events from one handler |
//!
//! # Quick start
//!
//! ```rust
//! use app::prelude::*;
//!
//! #[derive(Debug)]
//! struct Increment;
//! impl Event for Increment {}
//!
//! let module = Module::new()
//!     .on(|count: &mut u32, _: &Increment| *count += 1);
//!
//! let mut app = App::new(0u32)
//!     .mount(|s: &mut u32| s, module);
//!
//! app.dispatch(&Increment);
//! assert_eq!(*app.state(), 1);
//! ```
//!
//! # Event propagation
//!
//! Handlers can return an event (or `Option<E>`, [`Many<Iter>`], or an
//! `hlist![…]` of those) to cause further dispatches within the same call:
//!
//! ```rust
//! use app::prelude::*;
//!
//! #[derive(Debug)] struct Tick;   impl Event for Tick {}
//! #[derive(Debug)] struct Render; impl Event for Render {}
//!
//! let module = Module::new()
//!     .on(|_: &mut u32, _: &Tick| Render)           // Tick → emit Render
//!     .on(|count: &mut u32, _: &Render| *count += 1); // Render → mutate state
//!
//! let mut app = App::new(0u32).mount(|s: &mut u32| s, module);
//! app.dispatch(&Tick);
//! assert_eq!(*app.state(), 1);
//! ```
//!
//! Emitted events propagate to **all** mounted modules, including those mounted
//! before the emitter.
//!
//! # Module composition
//!
//! Modules nest via [`Module::mount`]. A *lens* function extracts the child's
//! state from the parent's. Emitted events from a child always propagate up to
//! the application root.
//!
//! ```rust
//! use app::prelude::*;
//!
//! #[derive(Debug)] struct Tick; impl Event for Tick {}
//!
//! struct AppState { counter: u32 }
//!
//! let child = Module::new().on(|s: &mut u32, _: &Tick| *s += 1);
//! let parent = Module::<AppState, _, _>::new()
//!     .mount(|s: &mut AppState| &mut s.counter, child);
//!
//! let mut app = App::new(AppState { counter: 0 })
//!     .mount(|s: &mut AppState| s, parent);
//!
//! app.dispatch(&Tick);
//! assert_eq!(app.state().counter, 1);
//! ```

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
