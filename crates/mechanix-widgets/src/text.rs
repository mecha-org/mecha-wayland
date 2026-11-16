use crate::color::{ColorSource, IntoColorSource};
use crate::font::FontContextExt;
use app::{Build, Context, Handle, Spawner, Widget};
use layout::LayoutStyle;
use theme::{ColorRole, FontWeight, SpawnerThemeExt, TextVariant, ThemeReader};
pub use widgets::TextProps;
use widgets::prelude::{
    Text as NativeText, TextAlign, TextBuilder as NativeTextBuilder,
    TextContext as NativeTextContext, TextDecoration, TextOverflow, TextWrap, VerticalTrim,
};

/// Themed Mechanix Design Text widget.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub variant: TextVariant,
    pub emphasised: bool,
    pub color: Option<ColorSource>,
    pub weight: Option<FontWeight>,
    pub size: Option<f32>,
    pub(crate) native: Handle<NativeText>,
}

impl Text {
    pub fn native(&self) -> Handle<NativeText> {
        self.native
    }
}

pub fn text(s: impl Into<String>) -> TextBuilder {
    TextBuilder::new(s)
}

/// Builder for a themed [`Text`].
#[derive(Debug, Clone, PartialEq)]
pub struct TextBuilder {
    pub variant: TextVariant,
    pub emphasised: bool,
    pub color: Option<ColorSource>,
    pub size: Option<f32>,
    pub weight: Option<FontWeight>,
    pub props: TextProps,
    pub style: LayoutStyle,
}

impl std::ops::Deref for TextBuilder {
    type Target = TextProps;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.props
    }
}

impl TextBuilder {
    pub fn new(s: impl Into<String>) -> Self {
        Self {
            variant: TextVariant::BodyLarge,
            emphasised: false,
            color: None,
            size: None,
            weight: None,
            props: TextProps::new(atlas::FontId(0), s),
            style: LayoutStyle::default(),
        }
    }

    pub fn variant(mut self, variant: TextVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn emphasised(mut self) -> Self {
        self.emphasised = true;
        self
    }

    pub fn color(mut self, color: impl IntoColorSource) -> Self {
        self.color = color.into_color_source();
        self
    }

    pub fn size(mut self, px: f32) -> Self {
        self.size = Some(px);
        self
    }

    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }

    pub fn letter_spacing(mut self, spacing: f32) -> Self {
        self.props.letter_spacing = Some(spacing);
        self
    }

    pub fn line_height(mut self, height: f32) -> Self {
        self.props.line_height = Some(height);
        self
    }

    pub fn align(mut self, align: TextAlign) -> Self {
        self.props.align = align;
        self
    }

    pub fn wrap(mut self, wrap: TextWrap) -> Self {
        self.props.wrap = wrap;
        self
    }

    pub fn overflow(mut self, overflow: TextOverflow) -> Self {
        self.props.overflow = overflow;
        self
    }

    pub fn max_lines(mut self, n: usize) -> Self {
        self.props.max_lines = Some(n);
        self
    }

    pub fn vertical_trim(mut self, trim: VerticalTrim) -> Self {
        self.props.vertical_trim = trim;
        self
    }

    pub fn decoration(mut self, decoration: TextDecoration) -> Self {
        self.props.decoration = decoration;
        self
    }

    pub fn style(mut self, style: LayoutStyle) -> Self {
        self.style = style;
        self
    }

    /// See [`TextProps::opaque`]. Opaque by default.
    pub fn opaque(mut self, opaque: bool) -> Self {
        self.props.opaque = opaque;
        self
    }

    /// Resolves theme tokens and overrides into concrete [`TextProps`].
    pub fn resolve_native_props(&self, env: &(impl ThemeReader + FontContextExt)) -> TextProps {
        let (font, px, color) = resolve_text_visuals(
            self.variant,
            self.emphasised,
            self.color,
            self.weight,
            self.size,
            env,
        );
        let mut props = self.props.clone();
        props.font = font;
        props.px = px;
        props.color = color;
        props
    }
}

impl Build for TextBuilder {
    type Widget = Text;
}

impl Widget for Text {
    type Builder = TextBuilder;

    fn build(mut b: TextBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        *s.component_mut::<LayoutStyle>(me).unwrap() = b.style;

        let variant = b.variant;
        let emphasised = b.emphasised;
        let color = b.color;
        let weight = b.weight;
        let size = b.size;

        let (font, px, resolved_color) =
            resolve_text_visuals(variant, emphasised, color, weight, size, s);
        b.props.font = font;
        b.props.px = px;
        b.props.color = resolved_color;

        let native = s.spawn(me, NativeTextBuilder::from(b.props));

        s.on_theme(me, sync_visuals);

        Text {
            variant,
            emphasised,
            color,
            weight,
            size,
            native,
        }
    }
}

/// Resolves typography tokens and overrides against the theme and font book.
pub(crate) fn resolve_text_visuals(
    variant: TextVariant,
    emphasised: bool,
    color: Option<ColorSource>,
    weight: Option<FontWeight>,
    size: Option<f32>,
    env: &(impl ThemeReader + FontContextExt),
) -> (atlas::FontId, u16, geometry::Color) {
    let typo = env.typography(variant);
    let effective_weight = weight.unwrap_or(if emphasised {
        typo.weight_emphasised
    } else {
        typo.weight
    });
    let font = env.font(effective_weight);
    let px = size.unwrap_or(typo.font_size).round().max(1.0) as u16;
    let color = resolve_color(color, env);
    (font, px, color)
}

/// Synchronizes underlying native text with active theme and styling tokens.
fn sync_visuals(ctx: &mut Context<'_, Text>) {
    let (variant, emphasised, color_src, weight, size, native) = {
        let me = ctx.me();
        (
            me.variant,
            me.emphasised,
            me.color,
            me.weight,
            me.size,
            me.native,
        )
    };
    let (font, px, color) = resolve_text_visuals(variant, emphasised, color_src, weight, size, ctx);

    if let Some(mut c) = ctx.at(native) {
        c.set_font(font);
        c.set_size(px);
        c.set_color(color);
    }
}

/// Extension trait for mutating a spawned [`Text`].
pub trait TextContextExt {
    fn set_text(&mut self, text: impl Into<String>);
    fn set_color(&mut self, color: impl IntoColorSource);
    fn set_variant(&mut self, variant: TextVariant);
    fn set_size(&mut self, px: f32);
    fn set_weight(&mut self, weight: FontWeight);
    fn set_emphasised(&mut self, emphasised: bool);

    fn set_align(&mut self, align: TextAlign);
    fn set_wrap(&mut self, wrap: TextWrap);
    fn set_overflow(&mut self, overflow: TextOverflow);
    fn set_max_lines(&mut self, max_lines: Option<usize>);
    fn set_line_height(&mut self, height: f32);
    fn set_letter_spacing(&mut self, spacing: f32);
    fn set_decoration(&mut self, decoration: TextDecoration);
    fn set_vertical_trim(&mut self, trim: VerticalTrim);
    fn set_font(&mut self, font: atlas::FontId);

    fn sync_visuals(&mut self);
}

impl TextContextExt for Context<'_, Text> {
    fn set_text(&mut self, text: impl Into<String>) {
        let native = self.me().native;
        if let Some(mut c) = self.at(native) {
            c.set_text(text);
        }
    }

    fn set_color(&mut self, color: impl IntoColorSource) {
        let color = color.into_color_source();
        if self.me().color == color {
            return;
        }
        self.me().color = color;
        sync_visuals(self);
    }

    fn set_variant(&mut self, variant: TextVariant) {
        if self.me().variant == variant {
            return;
        }
        self.me().variant = variant;
        sync_visuals(self);
    }

    fn set_size(&mut self, px: f32) {
        if self.me().size == Some(px) {
            return;
        }
        self.me().size = Some(px);
        sync_visuals(self);
    }

    fn set_weight(&mut self, weight: FontWeight) {
        if self.me().weight == Some(weight) {
            return;
        }
        self.me().weight = Some(weight);
        sync_visuals(self);
    }

    fn set_emphasised(&mut self, emphasised: bool) {
        if self.me().emphasised == emphasised {
            return;
        }
        self.me().emphasised = emphasised;
        sync_visuals(self);
    }

    fn set_align(&mut self, align: TextAlign) {
        let native = self.me().native;
        if let Some(mut c) = self.at(native) {
            c.set_align(align);
        }
    }

    fn set_wrap(&mut self, wrap: TextWrap) {
        let native = self.me().native;
        if let Some(mut c) = self.at(native) {
            c.set_wrap(wrap);
        }
    }

    fn set_overflow(&mut self, overflow: TextOverflow) {
        let native = self.me().native;
        if let Some(mut c) = self.at(native) {
            c.set_overflow(overflow);
        }
    }

    fn set_max_lines(&mut self, max_lines: Option<usize>) {
        let native = self.me().native;
        if let Some(mut c) = self.at(native) {
            c.set_max_lines(max_lines);
        }
    }

    fn set_line_height(&mut self, height: f32) {
        let native = self.me().native;
        if let Some(mut c) = self.at(native) {
            c.set_line_height(height);
        }
    }

    fn set_letter_spacing(&mut self, spacing: f32) {
        let native = self.me().native;
        if let Some(mut c) = self.at(native) {
            c.set_letter_spacing(spacing);
        }
    }

    fn set_decoration(&mut self, decoration: TextDecoration) {
        let native = self.me().native;
        if let Some(mut c) = self.at(native) {
            c.set_decoration(decoration);
        }
    }

    fn set_vertical_trim(&mut self, trim: VerticalTrim) {
        let native = self.me().native;
        if let Some(mut c) = self.at(native) {
            c.set_vertical_trim(trim);
        }
    }

    fn set_font(&mut self, font: atlas::FontId) {
        let native = self.me().native;
        if let Some(mut c) = self.at(native) {
            c.set_font(font);
        }
    }

    fn sync_visuals(&mut self) {
        sync_visuals(self);
    }
}

fn resolve_color(color: Option<ColorSource>, reader: &impl ThemeReader) -> geometry::Color {
    color
        .unwrap_or_else(|| ColorSource::from(ColorRole::OnSurface))
        .resolve(reader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_defaults() {
        let b = text("Hello");
        assert_eq!(b.string, "Hello");
        assert_eq!(b.variant, TextVariant::BodyLarge);
        assert!(!b.emphasised);
        assert_eq!(b.color, None);
        assert_eq!(b.size, None);
        assert_eq!(b.weight, None);
        assert_eq!(b.letter_spacing, None);
        assert_eq!(b.line_height, None);
        assert_eq!(b.align, TextAlign::Left);
        assert_eq!(b.wrap, TextWrap::NoWrap);
        assert_eq!(b.overflow, TextOverflow::Clip);
        assert_eq!(b.max_lines, None);
    }

    #[test]
    fn builder_overrides() {
        let b = text("Custom")
            .variant(TextVariant::HeadlineMedium)
            .emphasised()
            .color(ColorRole::Primary)
            .size(28.0)
            .weight(FontWeight::Bold)
            .letter_spacing(0.5)
            .line_height(34.0)
            .align(TextAlign::Center)
            .wrap(TextWrap::Word)
            .overflow(TextOverflow::Ellipsis)
            .max_lines(3);

        assert_eq!(b.variant, TextVariant::HeadlineMedium);
        assert!(b.emphasised);
        assert_eq!(b.color, Some(ColorRole::Primary.into()));
        assert_eq!(b.size, Some(28.0));
        assert_eq!(b.weight, Some(FontWeight::Bold));
        assert_eq!(b.letter_spacing, Some(0.5));
        assert_eq!(b.line_height, Some(34.0));
        assert_eq!(b.align, TextAlign::Center);
        assert_eq!(b.wrap, TextWrap::Word);
        assert_eq!(b.overflow, TextOverflow::Ellipsis);
        assert_eq!(b.max_lines, Some(3));
    }

    #[test]
    fn props_deref_and_direct_access() {
        let b = text("Testing").variant(TextVariant::LabelSmall).size(12.0);

        // TextBuilder derefs to TextProps
        assert_eq!(b.string, "Testing");
        assert_eq!(b.variant, TextVariant::LabelSmall);
        assert_eq!(b.size, Some(12.0));
    }

    #[test]
    fn resolve_props_with_theme() {
        use crate::font::FontBook;
        use app::App;
        use atlas::Atlas;
        use theme::prelude::*;

        let mut app = App::new();
        app.add_module(MechanixTheme::dark());
        let mut atlas = Atlas::new();
        let font_id = atlas
            .add_font(include_bytes!(
                "../../../crates/atlas/tests/fixtures/Inter-Regular.ttf"
            ))
            .unwrap();
        app.insert_resource(atlas);
        app.insert_resource(FontBook::new(font_id));

        let b = text("Hello Mechanix")
            .variant(TextVariant::HeadlineMedium)
            .emphasised();

        let resolved = b.resolve_native_props(&app);
        assert_eq!(resolved.string, "Hello Mechanix");
        assert_eq!(resolved.font, font_id);
        assert_eq!(resolved.px, 24);
        assert_eq!(resolved.color, app.color(ColorRole::OnSurface));
    }

    #[test]
    fn text_widget_spawns_and_syncs_on_update_and_theme() {
        use crate::font::FontBook;
        use app::App;
        use atlas::Atlas;
        use layout::LayoutModule;
        use paint::PaintModule;
        use theme::prelude::*;

        let mut app = App::new();
        app.add_module(MechanixTheme::dark());
        app.add_module(LayoutModule);
        app.add_module(PaintModule);

        let mut atlas = Atlas::new();
        let font_id = atlas
            .add_font(include_bytes!(
                "../../../crates/atlas/tests/fixtures/Inter-Regular.ttf"
            ))
            .unwrap();
        app.insert_resource(atlas);
        app.insert_resource(FontBook::new(font_id));

        let text_h = app.spawn(app.root(), text("Initial Text"));
        app.tick();

        let native_h = app.widget::<Text>(text_h).unwrap().native();
        let initial_color = app.widget::<NativeText>(native_h).unwrap().color;
        assert_eq!(
            app.widget::<NativeText>(native_h).unwrap().string,
            "Initial Text"
        );
        assert_eq!(initial_color, app.color(ColorRole::OnSurface));

        struct Poke;
        impl app::Event for Poke {}

        struct Controller;
        struct ControllerBuilder(Box<dyn FnMut(&mut Context<'_, Controller>)>);
        impl Build for ControllerBuilder {
            type Widget = Controller;
        }
        impl Widget for Controller {
            type Builder = ControllerBuilder;
            fn build(b: ControllerBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
                let mut action = b.0;
                s.on::<Poke>(me, move |ctx, _| action(ctx));
                Controller
            }
        }

        let controller = app.spawn(
            app.root(),
            ControllerBuilder(Box::new(move |ctx: &mut Context<'_, Controller>| {
                let mut c = ctx.at(text_h).unwrap();
                c.set_text("Updated Text");
                c.set_size(32.0);
                c.set_align(TextAlign::Center);
                c.set_wrap(TextWrap::Word);
                c.set_overflow(TextOverflow::Ellipsis);
                c.set_max_lines(Some(2));
                c.set_line_height(40.0);
                c.set_letter_spacing(1.5);
                c.set_decoration(TextDecoration::Underline);
                c.set_vertical_trim(VerticalTrim::CapHeight);
            })),
        );

        app.emit(Poke, controller.id());
        app.flush();
        app.tick();

        let native_w = app.widget::<NativeText>(native_h).unwrap();
        assert_eq!(native_w.string, "Updated Text");
        assert_eq!(native_w.px, 32);
        assert_eq!(native_w.align, TextAlign::Center);
        assert_eq!(native_w.wrap, TextWrap::Word);
        assert_eq!(native_w.overflow, TextOverflow::Ellipsis);
        assert_eq!(native_w.max_lines, Some(2));
        assert_eq!(native_w.line_height, Some(40.0));
        assert_eq!(native_w.letter_spacing, Some(1.5));
        assert_eq!(native_w.decoration, TextDecoration::Underline);
        assert_eq!(native_w.vertical_trim, VerticalTrim::CapHeight);

        // Switch to light theme
        app.set_theme(MechanixTheme::light());
        app.tick();

        let light_color = app.widget::<NativeText>(native_h).unwrap().color;
        assert_eq!(light_color, app.color(ColorRole::OnSurface));
        assert_ne!(light_color, initial_color);
    }
}
