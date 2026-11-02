#![forbid(unsafe_code)]
//! iOS-style touch scrolling: a [`Scrollable`] box whose children follow
//! the finger, glide on a flick, and rubber-band past either end.
//!
//! # Model
//!
//! - A [`Scrollable`] is built from [`scrollable()`] and scrolls along
//!   one [`Axis`], vertical by default. Its children are ordinary nodes,
//!   spawned under its handle as under a `Div`'s. Its default style is a
//!   block for `Vertical`, which keeps children at their natural height,
//!   and a non-wrapping row for `Horizontal`, whose children set
//!   `shrink(0.0)` to keep their width. [`ScrollableBuilder::style`]
//!   replaces it whole.
//! - The offset is the `layout::Viewport` the build writes, `clip` on:
//!   render and the hit test shift the children by it and cut them to
//!   the padding box. It grows as the content moves toward its end.
//!   Nothing relays out while scrolling. There is no API to read or set it.
//! - A frame where only the offset moved reuses the last one: the
//!   backend copies it shifted and draws just the strip that came into
//!   view. That holds only while the `Scrollable` sits on one flat colour
//!   with nothing drawn over or under its box by anything outside it;
//!   otherwise every scroll frame redraws the box, correct but slower. A
//!   header, scrim or popup that floats over a big scroll belongs in a
//!   separate window.
//! - The range is `0 ..= max(0, extent - viewport)`: the extent runs from
//!   the padding box's start to the furthest end of a direct child plus
//!   the end padding, and the viewport is the padding box's length, both
//!   along the axis, both taken on each `LayoutDone` for a window that
//!   was laid out. Content that fits does not move or bounce.
//! - A press starts tracking; the row under it gets its `Press` as usual.
//!   Every contact that lands before a drag starts is tracked. The first
//!   to move past the slop along the axis drives: the `Scrollable` claims
//!   every tracked contact (their rows get `PressCancelled`, so neither a
//!   swipe nor a finger it moved from under clicks) and the content
//!   follows the driver 1:1 from there, with resistance past either end.
//!   Motion across the axis never starts a drag. The other contacts are
//!   ignored from then on.
//! - On release the finger's speed, read from the input timestamps,
//!   becomes a fling: it coasts under friction and springs back at an
//!   end. A release past an end springs back at once, carrying the
//!   finger's speed; a cancel never flings. Each `Frame` of its window
//!   advances the glide and asks for the next; once settled it rests
//!   exactly at its end and asks for none.
//! - While dragging or gliding the `Scrollable` sets
//!   `interactivity::GrabPresses`: a contact landing on it then stops the
//!   glide dead and drags from there, and taps nothing underneath, and a
//!   second finger during a drag taps nothing.
//! - When a layout changes the range, an idle offset is clamped into it
//!   without animation; a glide carries on against the new range from
//!   where it is, and a drag on its next move.
//! - [`ScrollModule`] installs after `InteractivityModule` and
//!   `AnimationModule`, and before `RenderModule`, so a frame's glide is
//!   written before render draws it.
//!
//! # Quick start
//!
//! ```
//! use animation::AnimationModule;
//! use app::prelude::*;
//! use geometry::Point;
//! use interactivity::prelude::*;
//! use layout::prelude::*;
//! use paint::prelude::*;
//! use scroll::prelude::*;
//! use window::prelude::*;
//!
//! # struct Row;
//! # impl Build for Row { type Widget = Row; }
//! # impl Widget for Row {
//! #     type Builder = Row;
//! #     fn build(b: Row, _: Handle<Self>, _: &mut Spawner<'_, Self>) -> Self { b }
//! # }
//! let mut app = App::new();
//! app.add_module(LayoutModule)
//!     .add_module(PaintModule)
//!     .add_module(WindowModule)
//!     .add_module(InteractivityModule)
//!     .add_module(AnimationModule)
//!     .add_module(ScrollModule);
//!
//! let win = app.spawn(
//!     app.root(),
//!     window().layout(LayoutStyle::default().column().size(px(200.0), px(400.0))),
//! );
//! let list = app.spawn(win, scrollable().style(LayoutStyle::default().block().fill()));
//! for _ in 0..20 {
//!     app.spawn_with(list, Row, (LayoutStyle::default().height(px(50.0)),));
//! }
//! app.tick();
//!
//! // A finger lands, crosses the slop, then drags the content up 40 px.
//! let mut touch = |phase, y, time| {
//!     let position = Point::new(100.0, y);
//!     let contact = ContactId::Touch(0);
//!     app.signal(ContactInput { window: win.id(), contact, phase, position, time });
//!     app.flush();
//! };
//! touch(ContactPhase::Moved, 300.0, 0);
//! touch(ContactPhase::Pressed, 300.0, 0);
//! touch(ContactPhase::Moved, 289.0, 8);
//! touch(ContactPhase::Moved, 249.0, 16);
//! assert_eq!(app.component::<Viewport>(list).unwrap().scroll, Point::new(0.0, 40.0));
//! ```

mod module;
mod physics;
mod scrollable;
mod velocity;

#[doc(hidden)]
pub use module::ScrollClock;
pub use module::ScrollModule;
#[doc(hidden)]
pub use physics::Tuning;
pub use scrollable::{Axis, Scrollable, ScrollableBuilder, scrollable};
#[doc(hidden)]
pub use velocity::VelocityTracker;

pub mod prelude {
    pub use crate::{Axis, ScrollModule, Scrollable, ScrollableBuilder, scrollable};
}
