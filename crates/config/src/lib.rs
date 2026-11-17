#![forbid(unsafe_code)]
//! Typed settings from mxconfig on the ring: no threads, no polling.
//!
//! # Model
//!
//! - [`ConfigModule`] installs after `RingModule`. It inserts [`Config`],
//!   the app's one [`mxconfig::Source`].
//! - `DBusModule` is optional. Install it before [`ConfigModule`] to
//!   reach the daemon. Without it, an app's own namespace still works
//!   from its files, but shared namespaces, such as desktop settings,
//!   only ever have their schema defaults: they never get the real
//!   values or changes, and writes to them fail.
//! - [`Config::request`] asks for a generated client, on local files or
//!   through the daemon as mxconfig chooses. Everything about `T`'s
//!   namespace arrives as a [`ConfigEvent<T>`] carrying an [`Event<T>`].
//!   `Event::Opened` brings the client, which stays as the [`Client<T>`]
//!   resource. Each change, by any writer, is an `Event::Changed`.
//! - On `Event::Opened`, `client.settings().state()` says whether the
//!   client holds real values and can write, and if not, why.
//! - A client's getters read synchronously. Its setters return nothing:
//!   a write's values arrive as `Event::Changed`, like anyone else's.
//!   A failed write arrives as `Event::WriteFailed`, with the error and
//!   what the keys it touched still hold.
//! - Every client type requested for a namespace gets its events.
//! - A host of several app namespaces, or none, names itself with
//!   [`ConfigModule::app`].
//! - Local files are watched by one inotify read kept armed on the ring.
//!   Like `dbus`, it arranges a wake when it sent calls or raised signals
//!   in `BeforeWait`.
//!
//! # Failure
//!
//! Nothing a settings file or the daemon sends panics. Without inotify, or
//! once a read of it fails, settings load but don't follow file changes,
//! and each file namespace says so as `Event::Diagnosed`. A failed open
//! arrives as `Event::OpenFailed`. Problems found in the settings arrive
//! once, as `Event::Diagnosed`; nothing logs them for you.
//!
//! # Quick start
//!
//! ```no_run
//! use app::prelude::*;
//! use config::mxconfig::{self, Event};
//! use config::prelude::*;
//! use ring::prelude::*;
//!
//! mxconfig::client!("tests/demo.toml");
//!
//! fn on_demo(_app: &mut App, ConfigEvent(event): &ConfigEvent<Demo>) {
//!     match event {
//!         Event::Opened(demo) => println!("greeting: {}", demo.greeting()),
//!         Event::Changed(DemoChange::Greeting(greeting)) => {
//!             println!("greeting is now {greeting}");
//!         }
//!         Event::WriteFailed { error, .. } => eprintln!("not saved: {error}"),
//!         Event::Diagnosed(problems) => {
//!             for problem in problems {
//!                 eprintln!("settings: {problem}");
//!             }
//!         }
//!         _ => {}
//!     }
//! }
//!
//! let mut app = App::new();
//! app.add_module(RingModule::default())
//!     .add_module(ConfigModule::new())
//!     .system(on_demo);
//! app.resource_mut::<Config>().request::<Demo>();
//! app.run();
//! ```

use std::any::TypeId;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::time::Duration;

use app::prelude::*;
use dbus::{BusDisconnected, DBus, MethodReply, SignalReceived};
use mxconfig::runtime::Drive;
use mxconfig::{Event, Namespace, Outcome, Source, TicketId};
use ring::prelude::*;

mod link;

/// Re-exported for generated clients, which refer to `mxconfig::`.
pub use mxconfig;

use link::Link;

pub mod prelude {
    pub use crate::{Client, Config, ConfigEvent, ConfigModule};
}

/// An event in `T`'s namespace.
pub struct ConfigEvent<T: Namespace>(pub Event<T>);
impl<T: Namespace + 'static> Signal for ConfigEvent<T> {}

/// The opened client `T`, kept for any system to read.
pub struct Client<T>(pub T);
impl<T: 'static> Resource for Client<T> {}

/// Raises one client type's signal for an outcome.
type Raise = fn(&mut App, &Outcome);

/// A client type the app requested.
struct Requested {
    namespace: &'static str,
    id: TypeId,
    raise: Raise,
    /// The tickets of its opens still unanswered.
    opens: Vec<TicketId>,
}

/// The buffer size for one inotify read.
const INOTIFY_BUFFER: usize = 4096;

/// The app's settings source, inotify read and daemon links.
pub struct Config {
    source: Source,
    requested: Vec<Requested>,
    inotify: Option<OwnedFd>,
    reading: Option<Token>,
    buf: Vec<u8>,
    /// A zero timeout in flight, so the ring turns again.
    wake: Option<Token>,
    links: Vec<Link>,
}
impl Resource for Config {}

impl Config {
    /// Ask to open `T`'s namespace. The client arrives as `Event::Opened`.
    pub fn request<T: Namespace + Clone + 'static>(&mut self) {
        let ticket = self.source.request::<T>().id();
        let id = TypeId::of::<T>();
        match self.requested.iter_mut().find(|r| r.id == id) {
            Some(requested) => requested.opens.push(ticket),
            None => self.requested.push(Requested {
                namespace: T::NAMESPACE,
                id,
                raise: raise::<T>,
                opens: vec![ticket],
            }),
        }
    }

    /// The functions that raise `outcome`'s signals.
    fn raisers(&mut self, outcome: &Outcome) -> Vec<Raise> {
        let open = match outcome {
            Outcome::Opened { ticket, .. } | Outcome::OpenFailed { ticket, .. } => Some(*ticket),
            _ => None,
        };
        let mut chosen = Vec::new();
        for requested in &mut self.requested {
            if requested.namespace != outcome.namespace() {
                continue;
            }
            // An open goes only to the request it answers.
            if let Some(ticket) = open {
                let Some(i) = requested.opens.iter().position(|t| *t == ticket) else {
                    continue;
                };
                requested.opens.swap_remove(i);
            }
            chosen.push(requested.raise);
        }
        chosen
    }

    /// Keep a read armed on inotify.
    fn arm(&mut self, ring: &mut Ring) {
        if self.reading.is_none()
            && let Some(fd) = &self.inotify
        {
            let buf = std::mem::take(&mut self.buf);
            self.reading = Some(ring.read(fd.as_fd(), buf));
        }
    }

    /// Handle a completion. Returns whether it was our read.
    fn complete(&mut self, ring: &mut Ring, e: &IoEvent) -> bool {
        if self.wake == Some(e.token) {
            self.wake = None;
            ring.finish(e.token);
            return false;
        }
        if self.reading != Some(e.token) {
            return false;
        }
        self.reading = None;
        let done = ring
            .finish(e.token)
            .expect("config: a completed read is finishable");
        if e.result > 0 {
            self.source.feed_inotify(&done.buf);
        } else {
            let error = if e.result < 0 {
                io::Error::from_raw_os_error(-e.result)
            } else {
                io::Error::from(io::ErrorKind::UnexpectedEof)
            };
            // An interrupted read is armed again on the next turn. Any other
            // failure stops the reads, or they would complete forever, and
            // the file namespaces warn.
            if error.kind() != io::ErrorKind::Interrupted {
                self.inotify = None;
                self.source.inotify_lost(error);
            }
        }
        self.buf = done.buf;
        true
    }
}

/// Raise `T`'s signal for `outcome`.
fn raise<T: Namespace + Clone + 'static>(app: &mut App, outcome: &Outcome) {
    let Some(event) = outcome.event::<T>() else {
        return;
    };
    if let Event::Opened(client) = &event {
        app.insert_resource(Client(client.clone()));
    }
    app.signal(ConfigEvent::<T>(event));
}

/// Send queued requests and raise signals. Returns whether anything happened.
fn pump(app: &mut App) -> bool {
    let mut sent = false;
    if !app.resource::<Config>().links.is_empty() {
        let (mut config, mut dbus) = app.query::<(ResMut<Config>, ResMut<DBus>)>();
        let config = &mut *config;
        for link in &mut config.links {
            sent |= link.send(&config.source, &mut dbus);
        }
    }
    let outcomes = app.resource_mut::<Config>().source.outcomes();
    let raised = !outcomes.is_empty();
    for outcome in outcomes {
        let raisers = app.resource_mut::<Config>().raisers(&outcome);
        for raise in raisers {
            raise(app, &outcome);
        }
    }
    sent || raised
}

fn on_before_wait(app: &mut App, _: &BeforeWait) {
    // Wake the ring to flush calls just sent and handle events just raised.
    let wake = pump(app);
    let (mut config, mut ring) = app.query::<(ResMut<Config>, ResMut<Ring>)>();
    config.arm(&mut ring);
    if wake && config.wake.is_none() {
        config.wake = Some(ring.timeout(Duration::ZERO));
    }
}

fn on_io(app: &mut App, e: &IoEvent) {
    let read = {
        let (mut config, mut ring) = app.query::<(ResMut<Config>, ResMut<Ring>)>();
        config.complete(&mut ring, e)
    };
    if read {
        pump(app);
    }
}

/// Run `f` on `Config`, then raise signals if it handled something.
fn handle(app: &mut App, f: impl FnOnce(&mut Config) -> bool) {
    if f(&mut app.resource_mut::<Config>()) {
        pump(app);
    }
}

fn on_reply(app: &mut App, e: &MethodReply) {
    handle(app, |config| {
        let source = &config.source;
        config.links.iter_mut().any(|link| link.reply(source, e))
    });
}

fn on_signal(app: &mut App, e: &SignalReceived) {
    handle(app, |config| {
        let Some(link) = config.links.iter().find(|l| l.matches(e)) else {
            return false;
        };
        link.signal(&config.source, e);
        true
    });
}

fn on_disconnected(app: &mut App, e: &BusDisconnected) {
    handle(app, |config| {
        let Some(i) = config.links.iter().position(|l| l.id == e.bus) else {
            return false;
        };
        let link = config.links.remove(i);
        config.source.unreach(link.bus);
        true
    });
}

/// Inserts [`Config`] and watches its files.
#[derive(Default)]
pub struct ConfigModule {
    source: Option<Source>,
}

impl ConfigModule {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drive `source` instead of a new one.
    pub fn with_source(mut self, source: Source) -> Self {
        self.source = Some(source);
        self
    }

    /// Name the app, when it carries several app namespaces or none.
    pub fn app(mut self, id: &str) -> Self {
        self.source.get_or_insert_default().set_app(id);
        self
    }
}

impl Module for ConfigModule {
    fn install(self, app: &mut App) {
        let source = self.source.unwrap_or_default();
        // Without inotify, settings load but don't follow file changes.
        let inotify = source.inotify_fd().ok();
        // Without `DBusModule`, files only.
        let dbus = app.has_resource::<DBus>();
        let links = if dbus {
            link::connect(&source, &mut app.resource_mut::<DBus>())
        } else {
            Vec::new()
        };
        app.insert_resource(Config {
            source,
            requested: Vec::new(),
            inotify,
            reading: None,
            buf: Vec::with_capacity(INOTIFY_BUFFER),
            wake: None,
            links,
        });
        app.system(on_before_wait).system(on_io);
        if dbus {
            app.system(on_reply)
                .system(on_signal)
                .system(on_disconnected);
        }
    }
}
