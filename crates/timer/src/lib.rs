#![forbid(unsafe_code)]
//! Timeouts and intervals for widgets, on the ring: no threads, no
//! async runtime, and an idle app sleeps until its next timer is due.
//!
//! # Model
//!
//! - A widget *defines* its timers in [`Widget::build`] through
//!   [`SpawnerTimers`]: `set_timeout` and `set_interval` start right away,
//!   `add_timeout` and `add_interval` register stopped. Each is an
//!   `on::<Timeout>` handler on the node being built that runs only for
//!   its own timer, and each returns a [`TimerId`] the widget keeps.
//! - Its handlers *control* them through [`ContextTimers`]: start (a
//!   running timer restarts from now), start with a new duration, cancel,
//!   ask whether running. A handler cannot register a timer: handlers
//!   are only ever added at build, so starting a timer a thousand times
//!   adds nothing.
//! - A timer's deadline is the truth and it has at most one ring timeout
//!   in flight. A restart that moves the deadline later keeps that op,
//!   which re-arms for the rest when it completes early; only a deadline
//!   moved earlier, or a cancel, leaves an op stale, dropped when it
//!   completes.
//! - When a deadline comes, [`Timeout`] is emitted at the owner node. An
//!   interval re-arms from the fire, so a late fire shifts the rest later
//!   and never fires twice to catch up.
//! - A timer lives as long as its node: removing the node drops it, and
//!   an id kept past that is a quiet no-op returning `false`.
//! - [`TimerModule`] installs after `RingModule`. Ops are submitted on
//!   `BeforeWait`, so a timer started in a tick is armed before the
//!   runner blocks.
//!
//! # Quick start
//!
//! ```
//! use std::time::Duration;
//! use app::prelude::*;
//! use ring::prelude::*;
//! use timer::prelude::*;
//!
//! #[derive(Default)]
//! struct Rang(bool);
//! impl Resource for Rang {}
//!
//! struct Alarm;
//! impl Build for Alarm {
//!     type Widget = Alarm;
//! }
//! impl Widget for Alarm {
//!     type Builder = Alarm;
//!     fn build(b: Alarm, _: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
//!         s.set_timeout(Duration::from_millis(10), |ctx, _| {
//!             ctx.resource_mut::<Rang>().0 = true;
//!         });
//!         b
//!     }
//! }
//!
//! let mut app = App::new();
//! app.add_module(RingModule::default())
//!     .add_module(TimerModule)
//!     .init_resource::<Rang>();
//! let root = app.root();
//! app.spawn(root, Alarm);
//! app.tick();
//! Ring::turn(&mut app); // blocks until the timeout, then runs its handler
//! assert!(app.resource::<Rang>().0);
//! ```

use app::prelude::*;
use ring::prelude::*;

mod ext;
mod systems;
mod timers;

pub use ext::{ContextTimers, SpawnerTimers};
pub use timers::Timers;

pub mod prelude {
    pub use crate::{ContextTimers, SpawnerTimers, Timeout, TimerId, TimerModule, Timers};
}

/// The name of one timer. Never reused within a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerId(u64);

/// A timer's deadline came. Emitted at the node that registered it.
#[derive(Clone, Copy, Debug)]
pub struct Timeout {
    pub id: TimerId,
}
impl Event for Timeout {}

/// Inserts [`Timers`] and its systems. Installs after `RingModule`.
pub struct TimerModule;

impl Module for TimerModule {
    fn install(self, app: &mut App) {
        assert!(
            app.has_resource::<Ring>(),
            "TimerModule installs after RingModule"
        );
        app.insert_resource(Timers::default());
        app.system(systems::submit)
            .system(systems::complete)
            .system(systems::removed)
            .pending_check(|app| app.resource::<Timers>().needs_submit());
    }
}
