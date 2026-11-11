//! The `widgets` crate: `Div`, `Button`, `Text`, `Icon`, `Image` and `Input` — the
//! primitive widgets a spawn site names instead of hand-writing
//! `LayoutStyle` and `Paint` itself.
//!
//! # Model
//!
//! - The widgets write only `LayoutStyle`, `Measure` and `Paint` —
//!   columns `layout` and `paint` already register. `widgets` installs
//!   no `Module`, `Component` or `Resource` of its own. `Button` requires
//!   `interactivity` events to drive its state machine. `Input` also
//!   registers handlers for `InputEdit` (from `text-input`'s
//!   `zwp_text_input_v3` end) and `KeyPress`/`KeyRepeat` (from the
//!   keyboard path), which do nothing without those crates installed.
//! - Using any widget requires the caller to have installed
//!   `LayoutModule` and `PaintModule` and inserted an `Atlas` resource.
//!   `Button` also requires `InteractivityModule`.
//! - Mutation after spawn happens through each widget's own `*Context`
//!   trait ([`DivContext`], [`ButtonContext`], [`TextContext`],
//!   [`IconContext`], [`ImageContext`], [`InputContext`]),
//!   plus the generic `layout::StyleContext` and
//!   `paint::PaintContext`, which apply to every widget.
//!
//! # Quick start
//!
//! ```
//! use app::prelude::*;
//! use atlas::prelude::*;
//! use layout::prelude::*;
//! use paint::prelude::*;
//! use widgets::prelude::*;
//!
//! let mut app = App::new();
//! app.add_module(LayoutModule).add_module(PaintModule);
//! app.insert_resource(Atlas::new());
//!
//! let root = app.spawn_with(
//!     app.root(),
//!     div().style(LayoutStyle::default().size(px(200.0), px(100.0))),
//!     (LayoutRoot(true),),
//! );
//!
//! let font = app
//!     .resource_mut::<Atlas>()
//!     .add_font(include_bytes!("../../atlas/tests/fixtures/Inter-Regular.ttf"))
//!     .unwrap();
//! let sprite = app
//!     .resource_mut::<Atlas>()
//!     .insert(
//!         Class::Icon,
//!         &Bitmap { width: 8, height: 8, format: Format::R8, pixels: vec![255; 64] },
//!     )
//!     .unwrap();
//!
//! let label = app.spawn(root, text(font, "hi"));
//! let glyph = app.spawn(root, icon(sprite));
//!
//! app.tick();
//! assert!(app.component::<ComputedLayout>(label).unwrap().rect.width() > 0.0);
//! assert!(app.component::<ComputedLayout>(glyph).unwrap().rect.width() > 0.0);
//! ```

mod button;
mod div;
mod icon;
mod image;
mod input;
mod state;
mod text;

pub use button::{Button, ButtonBuilder, ButtonContext, ButtonProps, ButtonStateStyle, button};
pub use div::{Div, DivBuilder, DivContext, div};
pub use icon::{Icon, IconBuilder, IconContext, icon};
pub use image::{Image, ImageBuilder, ImageContext, image};
pub use input::{
    ContentHint, ContentPurpose, Input, InputBuilder, InputContext, InputEdit, InputFocus,
    InputSubmit, input,
};
pub use state::WidgetState;
pub use text::{
    Text, TextAlign, TextBuilder, TextContext, TextDecoration, TextOverflow, TextProps, TextWrap,
    VerticalTrim, text,
};

pub mod prelude {
    pub use crate::{
        Button, ButtonBuilder, ButtonContext, ButtonProps, ButtonStateStyle, ContentHint,
        ContentPurpose, Div, DivBuilder, DivContext, Icon, IconBuilder, IconContext, Image,
        ImageBuilder, ImageContext, Input, InputBuilder, InputContext, InputEdit, InputFocus,
        InputSubmit, Text, TextAlign, TextBuilder, TextContext, TextDecoration, TextOverflow,
        TextProps, TextWrap, VerticalTrim, WidgetState, button, div, icon, image, input, text,
    };
}
