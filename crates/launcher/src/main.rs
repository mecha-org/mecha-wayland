use app::prelude::*;
use io_ring::{Ring, RingSettings};
use launcher_counter::CounterUi;
use launcher_navbar::NavbarUi;
use launcher_status_bar::{
    ATLAS, StatusBarUi, UI_FONT_INTER_16, UI_FONT_INTER_24, UI_FONT_INTER_100,
};
use window_manager::{
    Color, WindowKind, WindowManager, WindowSettings, ZwlrLayerShellV1Layer,
    ZwlrLayerSurfaceV1Anchor, ZwlrLayerSurfaceV1KeyboardInteractivity,
};

#[derive(State)]
pub struct Launcher {
    window_manager: WindowManager,
    ring: Ring,
}

impl Launcher {
    pub fn new() -> Self {
        let ring = Ring::new(RingSettings::default());
        let window_manager = WindowManager::new(ring.proxy());
        Launcher {
            ring,
            window_manager,
        }
    }
}

fn main() {
    let mut launcher = Launcher::new();

    launcher.window_manager.upload_atlas(&ATLAS);

    launcher.window_manager.spawn_window(
        WindowSettings {
            width: 0,
            height: 36,
            clear_color: Color::rgba(0.08, 0.08, 0.12, 0.95),
            kind: WindowKind::LayerShell {
                layer: ZwlrLayerShellV1Layer::Top,
                anchor: ZwlrLayerSurfaceV1Anchor::Top
                    | ZwlrLayerSurfaceV1Anchor::Left
                    | ZwlrLayerSurfaceV1Anchor::Right,
                exclusive_zone: 36,
                namespace: "status-bar".to_string(),
                keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity::None,
            },
        },
        StatusBarUi::new(),
    );

    launcher.window_manager.spawn_window(
        WindowSettings {
            width: 0,
            height: 0,
            clear_color: Color::rgba(0.16, 0.16, 0.18, 1.0),
            kind: WindowKind::LayerShell {
                layer: ZwlrLayerShellV1Layer::Bottom,
                anchor: ZwlrLayerSurfaceV1Anchor::Top
                    | ZwlrLayerSurfaceV1Anchor::Bottom
                    | ZwlrLayerSurfaceV1Anchor::Left
                    | ZwlrLayerSurfaceV1Anchor::Right,
                exclusive_zone: 0,
                namespace: "counter".to_string(),
                keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity::Exclusive,
            },
        },
        CounterUi::new(&UI_FONT_INTER_24, &UI_FONT_INTER_100),
    );

    launcher.window_manager.spawn_window(
        WindowSettings {
            width: 0,
            height: 60,
            clear_color: Color::rgba(0.08, 0.10, 0.14, 0.95),
            kind: WindowKind::LayerShell {
                layer: ZwlrLayerShellV1Layer::Top,
                anchor: ZwlrLayerSurfaceV1Anchor::Bottom
                    | ZwlrLayerSurfaceV1Anchor::Left
                    | ZwlrLayerSurfaceV1Anchor::Right,
                exclusive_zone: 60,
                namespace: "navbar".to_string(),
                keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity::Exclusive,
            },
        },
        NavbarUi::new(&UI_FONT_INTER_16),
    );

    let mut app = App::new(launcher)
        .mount(window_manager::module())
        .mount(io_ring::module());

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}
