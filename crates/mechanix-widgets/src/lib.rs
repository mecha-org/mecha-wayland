//! Mechanix Design-themed widgets and styling utilities.
//!
//! Provides theme-aware widgets built on top of `widgets` and `theme`.

pub mod button;
pub mod clock;
pub mod color;
pub mod font;
pub mod slider;
pub mod state;
pub mod text;

pub use button::{
    Button, ButtonBuilder, ButtonContextExt, ButtonOverrides, ButtonSize, ButtonVariant, button,
};
pub use clock::{Clock, ClockBuilder, ClockContextExt, ClockInterval, ClockTick, clock};
pub use color::{ColorSource, IntoColorSource};
pub use font::{FontBook, FontContextExt, IntoWeight};
pub use slider::{
    Slider, SliderBuilder, SliderChange, SliderContextExt, SliderOrientation, SliderSize,
    SliderVariant, slider,
};
pub use state::StateLayer;
pub use text::{Text, TextBuilder, TextContextExt, text};

// Re-export native primitives so callers have a single import path.
pub use widgets::{
    Button as NativeButton, ButtonBuilder as NativeButtonBuilder,
    ButtonContext as NativeButtonContext, ButtonProps as NativeButtonProps,
    ButtonStateStyle as NativeButtonStateStyle, Div, DivBuilder, DivContext, Icon, IconBuilder,
    IconContext, Image, ImageBuilder, ImageContext, Text as NativeText, TextAlign,
    TextBuilder as NativeTextBuilder, TextContext as NativeTextContext, TextDecoration,
    TextOverflow, TextProps as NativeTextProps, TextWrap, VerticalTrim, WidgetState,
    button as native_button, div, icon, image, text as native_text,
};

pub mod prelude {
    // Themed widgets & builders
    pub use crate::button::{
        Button, ButtonBuilder, ButtonContextExt, ButtonOverrides, ButtonVariant, button,
    };
    pub use crate::clock::{Clock, ClockBuilder, ClockContextExt, ClockInterval, ClockTick, clock};
    pub use crate::text::{Text, TextBuilder, TextContextExt, text};

    // Color, typography & font resolution
    pub use crate::color::{ColorSource, IntoColorSource};
    pub use crate::font::{FontBook, FontContextExt, IntoWeight};
    pub use crate::state::StateLayer;

    // Passthrough native layout & widget primitives
    pub use widgets::{
        Div, DivBuilder, DivContext, Icon, IconBuilder, IconContext, Image, ImageBuilder,
        ImageContext, TextAlign, TextDecoration, TextOverflow, TextWrap, VerticalTrim, WidgetState,
        div, icon, image,
    };

    // Commonly paired theme tokens
    pub use theme::{ColorRole, FontWeight, TextVariant};
}
