use std::sync::mpsc;

use app::prelude::*;
use io_ring::{Ring, RingSettings};
use launcher_counter::CounterChanged;
use launcher_navbar::{NavbarUi, NAV_EXCLUSIVE_ZONE, NAV_SURFACE_HEIGHT};
use launcher_pagination::PaginationUi;
use launcher_status_bar::{
    StatusBarUi, ATLAS, UI_FONT_INTER_100, UI_FONT_INTER_14, UI_FONT_INTER_16, UI_FONT_INTER_24,
};
use notification::{create_notification_ui, PANEL_HEIGHT};
use ui::register_events;
use window_manager::{
    Color, WindowKind, WindowManager, WindowSettings, ZwlrLayerShellV1Layer,
    ZwlrLayerSurfaceV1Anchor, ZwlrLayerSurfaceV1KeyboardInteractivity,
};

const NOTIFICATION_NS: &str = "notification";

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
    let (notif_tx, notif_rx) = mpsc::channel();
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
            touch_config: Some(interactivity::touch::TouchConfig {
                tap_max_distance: 20.0,
                tap_max_duration: std::time::Duration::from_millis(400),
            }),
            gesture_config: None,
        },
        StatusBarUi::new(),
    );

    launcher.window_manager.spawn_window(
        WindowSettings {
            width: 0,
            height: 584,
            clear_color: Color::rgba(0.08, 0.08, 0.10, 1.0),
            kind: WindowKind::LayerShell {
                layer: ZwlrLayerShellV1Layer::Top,
                anchor: ZwlrLayerSurfaceV1Anchor::Top
                    | ZwlrLayerSurfaceV1Anchor::Left
                    | ZwlrLayerSurfaceV1Anchor::Right,
                exclusive_zone: 0,
                keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity::Exclusive,
                namespace: "pagination".to_string(),
            },
            touch_config: None,
            gesture_config: None,
        },
        PaginationUi::new(&UI_FONT_INTER_24, &UI_FONT_INTER_100),
    );

    launcher.window_manager.spawn_window(
        WindowSettings {
            width: 0,
            height: PANEL_HEIGHT as u32,
            clear_color: Color::TRANSPARENT,
            kind: WindowKind::LayerShell {
                layer: ZwlrLayerShellV1Layer::Top,
                anchor: ZwlrLayerSurfaceV1Anchor::Bottom
                    | ZwlrLayerSurfaceV1Anchor::Left
                    | ZwlrLayerSurfaceV1Anchor::Right,
                exclusive_zone: 0,
                namespace: NOTIFICATION_NS.to_string(),
                keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity::OnDemand,
            },
            touch_config: None,
            gesture_config: None,
        },
        create_notification_ui(
            &UI_FONT_INTER_24,
            &UI_FONT_INTER_16,
            &UI_FONT_INTER_14,
            notif_rx,
        ),
    );

    launcher.window_manager.spawn_window(
        WindowSettings {
            width: 0,
            height: NAV_SURFACE_HEIGHT,
            clear_color: Color::TRANSPARENT,
            kind: WindowKind::LayerShell {
                layer: ZwlrLayerShellV1Layer::Overlay,
                anchor: ZwlrLayerSurfaceV1Anchor::Bottom
                    | ZwlrLayerSurfaceV1Anchor::Left
                    | ZwlrLayerSurfaceV1Anchor::Right,
                exclusive_zone: NAV_EXCLUSIVE_ZONE,
                namespace: "navbar".to_string(),
                keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity::Exclusive,
            },
            touch_config: None,
            gesture_config: None,
        },
        NavbarUi::new(notif_tx),
    );

    let mut app = App::new(launcher)
        .mount(window_manager::module())
        .mount(
            Module::new().on(|_: &mut WindowManager, e: &CounterChanged| {
                eprintln!("[counter] value -> {}", e.0);
            }),
        )
        .mount(register_events!(CounterChanged))
        .mount(io_ring::module());

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}
