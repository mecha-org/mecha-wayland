#![forbid(unsafe_code)]
//! The window module: the [`Window`] widget, which window a node belongs
//! to, the list of live windows, and the frame request loop between the
//! app and whatever presents a window.
//!
//! # Model
//!
//! - A window is a node built from [`window()`], a child of the app root
//!   and a layout root: its `LayoutStyle` is the box it asks for and its
//!   `ComputedLayout` is laid out from the origin in its own coordinates. The
//!   default style is a column sized to its content.
//! - [`InWindow`] is on every node: the window it belongs to, a window's
//!   being itself, `None` outside every window. Written once, at spawn.
//! - A window is opaque and clears to a colour, black unless
//!   [`WindowBuilder::clear`] says otherwise: what shows where nothing is
//!   drawn, and what a renderer knows is behind everything at the top
//!   level. Set at build; there is no setter after spawn.
//! - [`Windows`] is the live list, in spawn order, exact after every
//!   flush; any tick that changed it fires `OnChanged<Windows>` once.
//! - The loop: anyone signals [`RequestFrame`] as often as it likes.
//!   This module forwards the first as one [`FrameRequested`] to the WSI
//!   (whatever presents the window) and swallows the rest. The WSI signals
//!   [`Frame`] when its callback fires; the drawer draws inside it, and
//!   the module clears its pending bit so the next request opens a new
//!   cycle. A request raised by a `Frame` system runs after it and opens
//!   the next cycle by itself.
//! - The WSI reports facts as events at the window node: [`Resized`]
//!   rewrites the style's size, [`ScaleFactorChanged`] is stored on the
//!   widget and asks for a frame, [`CloseRequested`] is left to whoever
//!   spawned the window.
//!
//! Nothing here knows a compositor, a role, a paint or a renderer. Who
//! raises `RequestFrame` is the seam: layout, paint and animation each
//! have their reasons and none is this module's.
//!
//! # Quick start
//!
//! ```
//! use app::prelude::*;
//! use geometry::Rect;
//! use layout::prelude::*;
//! use window::prelude::*;
//!
//! let mut app = App::new();
//! app.add_module(LayoutModule).add_module(WindowModule);
//!
//! let win = app.spawn(
//!     app.root(),
//!     window()
//!         .title("hello")
//!         .layout(LayoutStyle::default().column().size(px(320.0), px(200.0))),
//! );
//! app.tick();
//! assert_eq!(
//!     app.component::<ComputedLayout>(win).unwrap().rect,
//!     Rect::new(0.0, 0.0, 320.0, 200.0)
//! );
//! assert_eq!(app.resource::<Windows>().len(), 1);
//!
//! app.signal(RequestFrame(win.id()));
//! app.flush();
//! assert!(
//!     app.widget::<Window>(win).unwrap().is_pending(),
//!     "one FrameRequested went out to the WSI"
//! );
//! app.signal(Frame(win.id())); // the WSI's frame callback fired
//! app.flush();
//! assert!(
//!     !app.widget::<Window>(win).unwrap().is_pending(),
//!     "the cycle is closed"
//! );
//! ```

use app::prelude::*;
use geometry::{Color, Size};
use layout::prelude::*;

pub mod prelude {
    pub use crate::{
        CloseRequested, Frame, FrameRequested, InWindow, RequestFrame, Resized, ScaleFactorChanged,
        Window, WindowBuilder, WindowModule, Windows, window,
    };
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// A node presented on its own surface. Everything only a window has lives
/// here: the WSI and the renderer read one window at a time through
/// `app.widget::<Window>(w)`.
#[derive(Debug)]
pub struct Window {
    pub title: String,
    /// The WSI's scale for the window's output; `1.0` until it says.
    scale: f32,
    /// A `FrameRequested` went out and its `Frame` has not come back.
    pending: bool,
    /// What the window shows where nothing is drawn: opaque, or
    /// [`Color::TRANSPARENT`].
    clear: Color,
}

impl Window {
    /// The scale factor the WSI last reported. Layout is in logical
    /// pixels; the drawer multiplies by this.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Whether a frame request is out and unanswered.
    pub fn is_pending(&self) -> bool {
        self.pending
    }

    /// The colour the window clears to: black unless the builder said
    /// otherwise. What a renderer knows is behind everything at the top
    /// level.
    pub fn clear(&self) -> Color {
        self.clear
    }
}

/// A window with no title and the default window style: a column, sized
/// to its content.
pub fn window() -> WindowBuilder {
    WindowBuilder {
        title: String::new(),
        layout: LayoutStyle::default().column(),
        clear: Color::BLACK,
    }
}

/// What the spawn site knows about the window: its title and the box it
/// asks for. No style verbs; a whole `LayoutStyle` is given.
#[derive(Debug)]
pub struct WindowBuilder {
    title: String,
    layout: LayoutStyle,
    clear: Color,
}

impl WindowBuilder {
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The box the window asks for. Its size is a request until the WSI
    /// answers with `Resized`. The given style replaces the default
    /// entirely, the column included, so a caller who wants a column with
    /// a size writes `LayoutStyle::default().column().size(..)`.
    pub fn layout(mut self, style: LayoutStyle) -> Self {
        self.layout = style;
        self
    }

    /// The colour the window clears to. Black by default. An opaque
    /// colour, or one with no alpha for a see-through window, which is
    /// stored as [`Color::TRANSPARENT`] whatever its rgb. Anything inside
    /// the window composites onto it as onto any other solid. An alpha
    /// between the two is not rejected, but nothing is known behind the
    /// window's content and the WSI shows it opaque.
    pub fn clear(mut self, color: Color) -> Self {
        self.clear = if color.is_transparent() {
            Color::TRANSPARENT
        } else {
            color
        };
        self
    }
}

impl Build for WindowBuilder {
    type Widget = Window;
}

impl Widget for Window {
    type Builder = WindowBuilder;

    /// Writes the style and the root mark, claims itself as its own
    /// window, and returns the widget.
    fn build(b: WindowBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        *s.component_mut::<LayoutStyle>(me).unwrap() = b.layout;
        *s.component_mut::<LayoutRoot>(me).unwrap() = LayoutRoot(true);
        *s.component_mut::<InWindow>(me).unwrap() = InWindow(Some(me.id()));
        s.resource_mut::<Windows>().ids.push(me.id());
        s.on::<Resized>(me, on_resized);
        s.on::<ScaleFactorChanged>(me, on_scale_factor_changed);
        Window {
            title: b.title,
            scale: 1.0,
            pending: false,
            clear: b.clear,
        }
    }
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

/// The window a node belongs to. A window's is itself; `None` on the app
/// root and on anything hung outside every window. Written once, at spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InWindow(pub Option<NodeId>);
impl Component for InWindow {}

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

/// Every live window, in spawn order. A window's build pushes it and
/// `on_removed` drops what is gone, so the list is exact after every
/// flush. Any tick that changed it fires `OnChanged<Windows>` once.
#[derive(Debug, Default)]
pub struct Windows {
    ids: Vec<NodeId>,
}
impl Resource for Windows {}

impl Windows {
    pub fn iter(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.ids.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.ids.contains(&id)
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Please draw this window again. Signalled by anyone with a reason, any
/// number of times; while a cycle is open the extra ones are swallowed,
/// since the coming `Frame` draws the latest state anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestFrame(pub NodeId);
impl Signal for RequestFrame {}

/// This window wants to be drawn; tell me when. Signalled by this module
/// once per cycle, to the WSI, which asks its compositor for a callback
/// and answers with `Frame` when it fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRequested(pub NodeId);
impl Signal for FrameRequested {}

/// Draw this window now. Signalled by the WSI when its frame callback
/// fires, and by nothing in this module. Whoever draws attaches inside
/// it; what follows is the WSI's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame(pub NodeId);
impl Signal for Frame {}

/// The size the WSI settled, in logical pixels, never zero. Emitted by
/// the WSI at the window node once the compositor has configured it, and
/// again on every change. A size matching the style is still an event
/// (it is also the ack) but writes nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resized {
    pub size: Size,
}
impl Event for Resized {}

/// The scale of the window's output, as the WSI reports it. Layout stays
/// in logical pixels; the drawer reads `Window::scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleFactorChanged {
    pub scale: f32,
}
impl Event for ScaleFactorChanged {}

/// The shell asked for the window to close. Advice: the spawn site's own
/// handler removes the window, or ignores it. This module does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseRequested;
impl Event for CloseRequested {}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

/// Registers `InWindow` and attaches the systems. Installs after
/// `LayoutModule`, whose components a window's build writes.
pub struct WindowModule;

impl Module for WindowModule {
    fn install(self, app: &mut App) {
        app.register_component::<InWindow>()
            .init_resource::<Windows>()
            .system(on_spawned)
            .system(on_removed)
            .system(on_request_frame)
            .system(on_frame);
    }
}

/// A window set its own `InWindow` in `build`; every other node copies the
/// nearest ancestor's. The walk is needed because a subtree's `Spawned`
/// signals arrive children before parent, so a parent's own value may not
/// be written yet; a window's always is, so the walk ends at the right one.
fn on_spawned(app: &mut App, s: &Spawned) {
    if app
        .component::<InWindow>(s.id)
        .is_some_and(|w| w.0.is_some())
    {
        debug_assert_eq!(
            s.parent,
            app.root(),
            "a window must be a child of the app root: {:?}",
            s.id
        );
        return;
    }
    let found = app
        .ancestors(s.id)
        .find_map(|a| app.component::<InWindow>(a).and_then(|w| w.0));
    if let Some(w) = found
        && let Some(mut slot) = app.component_mut::<InWindow>(s.id)
    {
        slot.0 = Some(w);
    }
}

/// The removed id is stale and may have had windows anywhere under it, so
/// drop every stale id. The write guard is taken only if something is
/// stale, so `OnChanged<Windows>` fires only when the list shrank.
fn on_removed(app: &mut App, _: &Removed) {
    let any_stale = app.resource::<Windows>().iter().any(|w| !app.is_live(w));
    if !any_stale {
        return;
    }
    let live: Vec<NodeId> = app
        .resource::<Windows>()
        .iter()
        .filter(|&w| app.is_live(w))
        .collect();
    app.resource_mut::<Windows>().ids = live;
}

/// The first request of a cycle goes to the WSI; the rest wait for its
/// `Frame`. A stale id or a non-window is skipped. `pending` is set even
/// when nothing listens for `FrameRequested`: with no WSI installed, the
/// first `RequestFrame` leaves the window pending forever and later ones
/// are swallowed, so an app that wants the loop must install something
/// that answers `FrameRequested`.
fn on_request_frame(app: &mut App, r: &RequestFrame) {
    let Some(w) = app.widget_mut::<Window>(r.0) else {
        return;
    };
    if w.pending {
        return;
    }
    w.pending = true;
    app.signal(FrameRequested(r.0));
}

/// The cycle is closed. A `RequestFrame` queued by any `Frame` system runs
/// after every `Frame` system, finds `pending` clear, and opens the next
/// cycle by itself. A `Frame` for a window that was not pending is
/// accepted, so a WSI that draws on its own initiative is harmless.
fn on_frame(app: &mut App, f: &Frame) {
    if let Some(w) = app.widget_mut::<Window>(f.0) {
        w.pending = false;
    }
}

// ---------------------------------------------------------------------------
// Handlers, on the window's own node
// ---------------------------------------------------------------------------

/// The settled size is the window's new size request; layout picks it up
/// at the next `PostTick`. A matching size writes nothing, so the record
/// is not flagged and nothing is relaid out.
fn on_resized(ctx: &mut Context<'_, Window>, r: &Resized) {
    debug_assert!(
        r.size.width > 0.0 && r.size.height > 0.0,
        "Resized with a zero dimension: {:?}",
        r.size
    );
    let (width, height) = (px(r.size.width), px(r.size.height));
    if let Some(mut style) = ctx.component_mut::<LayoutStyle>()
        && (style.width != width || style.height != height)
    {
        style.width = width;
        style.height = height;
    }
}

/// A new scale means the content must be redrawn even though no layout
/// moved, so the window asks for a frame itself. The same scale again
/// does nothing.
fn on_scale_factor_changed(ctx: &mut Context<'_, Window>, e: &ScaleFactorChanged) {
    let me = ctx.handle().id();
    let changed = {
        let win = ctx.me();
        if win.scale == e.scale {
            false
        } else {
            win.scale = e.scale;
            true
        }
    };
    if changed {
        ctx.signal(RequestFrame(me));
    }
}
