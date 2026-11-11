#![forbid(unsafe_code)]
//! `zwlr_foreign_toplevel_manager_v1`: the compositor's list of open
//! windows, with activate and close.
//!
//! [`ToplevelsModule`] binds the manager and keeps a [`Toplevels`]
//! resource current. The protocol is double-buffered per handle: title,
//! app id, state and parent accumulate until the handle's `done`, which
//! publishes them together. A toplevel is not listed before its first
//! `done`. [`ToplevelsChanged`] is signalled whenever a `done` changes
//! the list or a `closed`/`finished` removes from it.
//!
//! Install after `WaylandModule`. [`Toplevels::activate`] takes a
//! `WlSeat`, which the caller binds. A compositor without the manager
//! yields an empty, unavailable [`Toplevels`].
//!
//! A closed toplevel's handle is destroyed as soon as its `closed`
//! arrives, while callers only learn of it at the next
//! [`ToplevelsChanged`]; requests made with its id in between are
//! dropped rather than sent on a dead object.
//!
//! `phosh_private` (version 8 or later) is bound too, when offered, for
//! live window previews: see [`Thumbnail`](prelude::Thumbnail).

use app::prelude::*;
use wayland::prelude::*;

mod thumbnail;

pub mod prelude {
    pub use crate::thumbnail::Thumbnail;
    pub use crate::{ToplevelId, ToplevelInfo, Toplevels, ToplevelsChanged, ToplevelsModule};
}

/// Identifies one toplevel: its protocol handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ToplevelId(ZwlrForeignToplevelHandleV1);

impl ToplevelId {
    pub fn handle(&self) -> ZwlrForeignToplevelHandleV1 {
        self.0
    }

    /// An id from a raw object id, for tests that have no connection.
    #[doc(hidden)]
    pub fn from_raw(id: u32) -> ToplevelId {
        ToplevelId(ZwlrForeignToplevelHandleV1(ObjectId(id)))
    }
}

/// What the compositor last reported for one toplevel, as of its `done`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToplevelInfo {
    pub id: ToplevelId,
    pub title: String,
    pub app_id: String,
    pub activated: bool,
    pub parent: Option<ToplevelId>,
    /// A counter bumped at each `done` that newly reports `activated`;
    /// 0 means never activated.
    pub last_activated: u64,
}

/// Sent at most once per dispatch, after the events of that dispatch,
/// when a `done`, a `closed` or the manager's `finished` changed the
/// list.
pub struct ToplevelsChanged;
impl Signal for ToplevelsChanged {}

/// The fields the events since the last `done` have set.
struct Pending {
    title: String,
    app_id: String,
    activated: bool,
    parent: Option<ToplevelId>,
}

struct Entry {
    info: ToplevelInfo,
    pending: Pending,
    /// Whether a `done` has published this toplevel.
    mapped: bool,
}

/// The open windows. See the module docs.
pub struct Toplevels {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    /// `phosh_private`, bound only when it is version 8 or later.
    phosh: Option<PhoshPrivate>,
    entries: Vec<Entry>,
    stamp: u64,
    /// The list changed since the last `ToplevelsChanged`.
    dirty: bool,
}
impl Resource for Toplevels {}

impl Toplevels {
    /// Toplevels that have had a `done`, in the order they appeared.
    pub fn iter(&self) -> impl Iterator<Item = &ToplevelInfo> {
        self.entries.iter().filter(|e| e.mapped).map(|e| &e.info)
    }

    pub fn get(&self, id: ToplevelId) -> Option<&ToplevelInfo> {
        self.entries
            .iter()
            .find(|e| e.mapped && e.info.id == id)
            .map(|e| &e.info)
    }

    /// Ask the compositor to activate `id` for `seat`. Does nothing once
    /// `id` has closed: its handle is already destroyed.
    pub fn activate(&self, wl: &mut Wayland, seat: WlSeat, id: ToplevelId) {
        if self.holds(id) {
            id.0.activate(wl, seat);
        }
    }

    /// Ask the compositor to close `id`. Does nothing once `id` has
    /// closed.
    pub fn close(&self, wl: &mut Wayland, id: ToplevelId) {
        if self.holds(id) {
            id.0.close(wl);
        }
    }

    /// Whether the compositor offers the manager (and still serves it).
    pub fn available(&self) -> bool {
        self.manager.is_some()
    }

    /// Whether `id`'s handle is still alive. An entry is removed in the
    /// same step that destroys its handle, so this is exact even before
    /// the next `ToplevelsChanged` reaches the caller.
    pub(crate) fn holds(&self, id: ToplevelId) -> bool {
        self.entries.iter().any(|e| e.info.id == id)
    }

    fn entry(&mut self, id: ToplevelId) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.info.id == id)
    }
}

/// Binds the manager if the compositor advertises it.
pub struct ToplevelsModule;

impl Module for ToplevelsModule {
    fn install(self, app: &mut App) {
        // The list needs only foreign-toplevel; without `phosh_private`
        // new enough for thumbnails, there are just no previews.
        let (manager, phosh): (Option<ZwlrForeignToplevelManagerV1>, Option<PhoshPrivate>) = {
            let (globals, mut wl) = app.query::<(Res<Globals>, ResMut<Wayland>)>();
            (
                globals.bind_if_offered(&mut wl, 1),
                globals.bind_if_offered(&mut wl, thumbnail::MIN_VERSION),
            )
        };
        let available = manager.is_some();
        app.insert_resource(Toplevels {
            manager,
            phosh,
            entries: Vec::new(),
            stamp: 0,
            dirty: false,
        });
        if available {
            app.system(on_manager)
                .system(on_handle)
                .system(on_post_tick);
        }
    }
}

fn on_manager(app: &mut App, e: &ZwlrForeignToplevelManagerV1Event) {
    match e {
        ZwlrForeignToplevelManagerV1Event::Toplevel { toplevel, .. } => {
            let id = ToplevelId(*toplevel);
            app.resource_mut::<Toplevels>().entries.push(Entry {
                info: ToplevelInfo {
                    id,
                    title: String::new(),
                    app_id: String::new(),
                    activated: false,
                    parent: None,
                    last_activated: 0,
                },
                pending: Pending {
                    title: String::new(),
                    app_id: String::new(),
                    activated: false,
                    parent: None,
                },
                mapped: false,
            });
        }
        ZwlrForeignToplevelManagerV1Event::Finished { .. } => {
            let (mut t, mut wl) = app.query::<(ResMut<Toplevels>, ResMut<Wayland>)>();
            t.manager = None;
            let entries = std::mem::take(&mut t.entries);
            let changed = entries.iter().any(|e| e.mapped);
            for e in &entries {
                e.info.id.0.destroy(&mut wl);
            }
            drop((t, wl));
            if changed {
                app.resource_mut::<Toplevels>().dirty = true;
            }
        }
    }
}

/// One `ToplevelsChanged` for everything the events of this dispatch
/// changed.
fn on_post_tick(app: &mut App, _: &PostTick) {
    let dirty = std::mem::take(&mut app.resource_mut::<Toplevels>().dirty);
    if dirty {
        app.signal(ToplevelsChanged);
    }
}

fn on_handle(app: &mut App, e: &ZwlrForeignToplevelHandleV1Event) {
    use ZwlrForeignToplevelHandleV1Event as E;
    let mut changed = false;
    let mut t = app.resource_mut::<Toplevels>();
    match e {
        E::Title {
            foreign_toplevel_handle: h,
            title,
        } => {
            if let Some(e) = t.entry(ToplevelId(*h)) {
                e.pending.title = title.clone();
            }
        }
        E::AppId {
            foreign_toplevel_handle: h,
            app_id,
        } => {
            if let Some(e) = t.entry(ToplevelId(*h)) {
                e.pending.app_id = app_id.clone();
            }
        }
        E::State {
            foreign_toplevel_handle: h,
            state,
        } => {
            let activated = state.chunks_exact(4).any(|c| {
                let v = u32::from_ne_bytes([c[0], c[1], c[2], c[3]]);
                ZwlrForeignToplevelHandleV1State::try_from(v)
                    == Ok(ZwlrForeignToplevelHandleV1State::Activated)
            });
            if let Some(e) = t.entry(ToplevelId(*h)) {
                e.pending.activated = activated;
            }
        }
        E::Parent {
            foreign_toplevel_handle: h,
            parent,
        } => {
            if let Some(e) = t.entry(ToplevelId(*h)) {
                e.pending.parent = parent.map(ToplevelId);
            }
        }
        E::Done {
            foreign_toplevel_handle: h,
        } => {
            let id = ToplevelId(*h);
            let stamp = t.stamp + 1;
            if let Some(e) = t.entry(id) {
                let p = &e.pending;
                let newly = p.activated && !e.info.activated;
                let next = ToplevelInfo {
                    id,
                    title: p.title.clone(),
                    app_id: p.app_id.clone(),
                    activated: p.activated,
                    parent: p.parent,
                    last_activated: if newly { stamp } else { e.info.last_activated },
                };
                if !e.mapped || next != e.info {
                    e.info = next;
                    e.mapped = true;
                    changed = true;
                    if newly {
                        t.stamp = stamp;
                    }
                }
            }
        }
        E::Closed {
            foreign_toplevel_handle: h,
        } => {
            let id = ToplevelId(*h);
            if let Some(at) = t.entries.iter().position(|e| e.info.id == id) {
                changed = t.entries.remove(at).mapped;
                drop(t);
                let mut wl = app.resource_mut::<Wayland>();
                h.destroy(&mut wl);
            }
        }
        // Outputs are not tracked.
        _ => {}
    }
    if changed {
        app.resource_mut::<Toplevels>().dirty = true;
    }
}
