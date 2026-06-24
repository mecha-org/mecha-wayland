use app::prelude::*;
use io_ring::{Ring, RingSettings};
use window_manager::{
    WindowKind, WindowManager, WindowSettings, ZwlrLayerShellV1Layer, ZwlrLayerSurfaceV1Anchor,
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

    launcher.window_manager.create_window(WindowSettings {
        width: 0,
        height: 36,
        color: 0x00_1E_1E_2E,
        kind: WindowKind::LayerShell {
            layer: ZwlrLayerShellV1Layer::Top,
            anchor: ZwlrLayerSurfaceV1Anchor::Top
                | ZwlrLayerSurfaceV1Anchor::Left
                | ZwlrLayerSurfaceV1Anchor::Right,
            exclusive_zone: 36,
            namespace: "status-bar".to_string(),
        },
    });

    launcher.window_manager.create_window(WindowSettings {
        width: 100,
        height: 100,
        color: 0x00_1E_1E_2E,
        kind: WindowKind::Xdg {
            title: "Launcher Window".to_string(),
        },
    });

    let mut app = App::new(launcher)
        .mount(window_manager::module())
        .mount(io_ring::module());

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}
