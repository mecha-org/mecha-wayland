//! A bottom sheet holding a Wi-Fi password field, over a scrim. Close it
//! by dragging the handle or title down, or tapping the scrim; "Close" in
//! the footer closes it from code, and "Show sheet" brings it back. Run
//! under a Wayland session: `cargo run --example bottom_sheet`.
//!
//! The shape to copy: spawn the sheet, read its `body()` / `footer()`
//! slots with `Spawner::widget`, spawn the content into them, and answer
//! `SheetClosed`, which means the user closed it: a real prompt would
//! reply "cancelled" and remove the sheet there. When the prompt closes
//! the sheet itself (the password was accepted), it does its cleanup
//! right after calling `close()`.

use mecha_wayland::mechanix_widgets::prelude::{
    BottomSheet, BottomSheetContextExt, ButtonVariant, FontBook, SheetClosed, TextContextExt,
    bottom_sheet, button, text,
};
use mecha_wayland::prelude::*;
use theme::{ColorRole, TextVariant};

struct Demo;
struct DemoBuilder;
impl Build for DemoBuilder {
    type Widget = Demo;
}

impl Widget for Demo {
    type Builder = DemoBuilder;
    fn build(_: DemoBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        *s.component_mut::<LayoutStyle>(me).unwrap() = LayoutStyle::default().relative().fill();

        // Behind the sheet: a way to bring it back once closed, and a
        // line saying what happened.
        let backdrop = s.spawn(
            me,
            div().style(
                LayoutStyle::default()
                    .absolute()
                    .inset(Insets::new(px(24.0), px(24.0), auto(), px(24.0)))
                    .column()
                    .gap(px(12.0)),
            ),
        );
        let show = s.spawn(
            backdrop,
            button().variant(ButtonVariant::Filled).label("Show sheet"),
        );
        let status = s.spawn(
            backdrop,
            text("")
                .variant(TextVariant::BodyMedium)
                .color(ColorRole::OnSurface),
        );

        let sheet = s.spawn(me, bottom_sheet().title("OfficeWifi 3").scrim(true));
        let (body, footer) = {
            let w = s.widget::<BottomSheet>(sheet).expect("sheet is built");
            (w.body(), w.footer())
        };

        // The password field: an outlined box around the masked text.
        let field = s.spawn(
            body,
            div()
                .style(
                    LayoutStyle::default()
                        .row()
                        .align_items(Align::Center)
                        .min_height(px(56.0))
                        .padding(Insets::new(px(0.0), px(16.0), px(0.0), px(16.0))),
                )
                .radius(8.0)
                .border(1.0, s.color(ColorRole::Outline)),
        );
        s.spawn(
            field,
            text("••••••••")
                .variant(TextVariant::BodyLarge)
                .color(ColorRole::OnSurfaceVariant),
        );
        s.spawn(
            body,
            text("Drag down or tap outside to close")
                .variant(TextVariant::BodyMedium)
                .color(ColorRole::OnSurfaceVariant),
        );
        // Closing from code, as an agent does once the password is accepted.
        let close = s.spawn(
            footer,
            button()
                .variant(ButtonVariant::Text)
                .label("Close")
                .width(percent(100.0)),
        );

        // Only the user's close arrives here: a prompt's "cancelled".
        s.on::<SheetClosed>(sheet, move |ctx, _| {
            ctx.at(status)
                .unwrap()
                .set_text("Closed by the user: a prompt would cancel");
        });
        // Closing and opening from code emit nothing; the caller follows up
        // itself.
        s.on::<Clicked>(close, move |ctx, _| {
            ctx.at(sheet).unwrap().close();
            ctx.at(status).unwrap().set_text("Closed from code");
        });
        s.on::<Clicked>(show, move |ctx, _| {
            ctx.at(sheet).unwrap().open();
            ctx.at(status).unwrap().set_text("");
        });

        Demo
    }
}

struct Shell;
struct ShellBuilder {
    root: NodeId,
}
impl Build for ShellBuilder {
    type Widget = Shell;
}
impl Widget for Shell {
    type Builder = ShellBuilder;
    fn build(b: ShellBuilder, _me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let backdrop = s.color(ColorRole::Surface);
        let win = s.spawn(
            b.root,
            window()
                .title("bottom sheet")
                .clear(backdrop)
                .layout(LayoutStyle::default().size(px(400.0), px(640.0))),
        );
        s.on::<CloseRequested>(win, |ctx, _| ctx.signal(Stop));
        s.spawn(win, DemoBuilder);
        Shell
    }
}

fn main() {
    let mut app = App::new();
    app.add_module(LayoutModule)
        .add_module(PaintModule)
        .add_module(WindowModule)
        .add_module(InteractivityModule)
        .add_module(AnimationModule)
        .add_module(RenderModule::default())
        .insert_resource(Atlas::new());
    app.add_module(MechanixTheme::light());
    let font = app
        .resource_mut::<Atlas>()
        .add_font(include_bytes!(
            "../crates/atlas/tests/fixtures/Inter-Regular.ttf"
        ))
        .expect("Inter loads");
    app.insert_resource(FontBook::new(font));

    app.add_module(RingModule::default())
        .add_module(
            WaylandModule::new()
                .bind::<WlCompositor>()
                .bind::<ZwpLinuxDmabufV1>()
                .bind::<XdgWmBase>()
                .bind::<WlSeat>(),
        )
        .add_module(PresentationModule {
            app_id: "mecha.bottom_sheet".into(),
            budget: Budget::default(),
        });

    let root = app.root();
    app.spawn(root, ShellBuilder { root });
    app.run();
}
