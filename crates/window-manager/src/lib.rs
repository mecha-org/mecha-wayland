mod globals;
mod render;
mod window;
pub mod prelude;

use app::{RegisteredModule, prelude::State};
use io_ring::RingProxy;
use std::{collections::HashMap, marker::PhantomData};
use wayland::{Interface, *};

use globals::WaylandGlobals;
use window::{Window, WindowKindHandles};

pub use window::{WindowId, WindowKind, WindowSettings, ZwlrLayerShellV1Layer, ZwlrLayerSurfaceV1Anchor};

#[derive(State)]
pub struct WindowManager {
    wayland: Wayland,
    globals: WaylandGlobals,
    pending: Vec<WindowSettings>,
    windows: HashMap<WindowId, Window<()>>,
}

impl WindowManager {
    pub fn new(ring_proxy: RingProxy) -> Self {
        let wayland = Wayland::new(ring_proxy.clone());
        Self {
            wayland,
            globals: WaylandGlobals::default(),
            pending: Vec::new(),
            windows: HashMap::new(),
        }
    }

    pub fn start(&mut self) {
        let display = self.wayland.display();
        display.get_registry();
        display.sync();
    }

    pub fn pre_poll(&mut self) {
        self.wayland.proxy().flush();
    }

    pub fn poll(&mut self) {}

    pub fn create_window(&mut self, settings: WindowSettings) {
        self.pending.push(settings);
    }

    fn flush_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for settings in pending {
            let WindowSettings { width, height, color, kind } = settings;
            match kind {
                WindowKind::LayerShell { layer, anchor, exclusive_zone, namespace } => {
                    let compositor = self
                        .globals
                        .compositor
                        .clone()
                        .unwrap_or_else(|| panic!("compositor global missing"));
                    let layer_shell = self
                        .globals
                        .layer_shell
                        .clone()
                        .unwrap_or_else(|| panic!("layer_shell global missing"));

                    let surface = compositor.create_surface();
                    let layer_surface =
                        layer_shell.get_layer_surface(&surface, None, layer, &namespace);
                    layer_surface.set_size(width, height);
                    layer_surface.set_anchor(anchor);
                    layer_surface.set_exclusive_zone(exclusive_zone);
                    surface.commit();

                    let id = WindowId(layer_surface.object_id().expect("just allocated"));
                    self.windows.insert(
                        id,
                        Window {
                            surface,
                            buffer: None,
                            color,
                            width,
                            height,
                            kind: WindowKindHandles::LayerShell { layer_surface },
                            _phantom: PhantomData,
                        },
                    );
                }
                WindowKind::Xdg { title } => {
                    let compositor = self
                        .globals
                        .compositor
                        .clone()
                        .unwrap_or_else(|| panic!("compositor global missing"));
                    let xdg_wm_base = self
                        .globals
                        .xdg_wm_base
                        .clone()
                        .unwrap_or_else(|| panic!("xdg_wm_base global missing"));

                    let surface = compositor.create_surface();
                    let xdg_surface = xdg_wm_base.get_xdg_surface(&surface);
                    let toplevel = xdg_surface.get_toplevel();
                    toplevel.set_title(&title);
                    surface.commit();

                    let id = WindowId(xdg_surface.object_id().expect("just allocated"));
                    self.windows.insert(
                        id,
                        Window {
                            surface,
                            buffer: None,
                            color,
                            width,
                            height,
                            kind: WindowKindHandles::Xdg { xdg_surface, toplevel },
                            _phantom: PhantomData,
                        },
                    );
                }
            }
        }
    }
}

pub fn module<S>() -> impl app::RegisteredModule<WindowManager, S> {
    app::Module::new()
        .mount(wayland::module::<S>().into_module())
        .on(|wm: &mut WindowManager, _: &app::Start| wm.start())
        .on(|wm: &mut WindowManager, _: &app::PrePoll| wm.pre_poll())
        .on(|wm: &mut WindowManager, _: &app::Poll| wm.poll())
        .on(|_: &mut WindowManager, event: &wayland::WlDisplayEvent| {
            println!("{:?}", event);
        })
        .on(|wm: &mut WindowManager, event: &wayland::WlRegistryEvent| {
            if let wayland::WlRegistryEvent::Global { sender, name, interface, version } = event {
                match interface.as_str() {
                    WlCompositor::NAME => wm.globals.compositor = Some(sender.bind(*name, *version)),
                    WlShm::NAME => wm.globals.shm = Some(sender.bind(*name, *version)),
                    ZwlrLayerShellV1::NAME => wm.globals.layer_shell = Some(sender.bind(*name, *version)),
                    WlOutput::NAME => wm.globals.output = Some(sender.bind(*name, *version)),
                    XdgWmBase::NAME => wm.globals.xdg_wm_base = Some(sender.bind(*name, *version)),
                    _ => {}
                }
            }
        })
        .on(|wm: &mut WindowManager, _: &wayland::WlCallbackEvent| {
            wm.flush_pending();
        })
        .on(|wm: &mut WindowManager, event: &wayland::ZwlrLayerSurfaceV1Event| {
            if let wayland::ZwlrLayerSurfaceV1Event::Configure { sender, serial, width, height } = event {
                let id = WindowId(sender.object_id().expect("live handle"));
                let (w, h) = {
                    let window = wm.windows.get(&id).expect("window exists for configure");
                    let w = if *width == 0 { window.width } else { *width };
                    let h = if *height == 0 { window.height } else { *height };
                    (w, h)
                };
                let shm = wm.globals.shm.clone().expect("shm global");
                sender.ack_configure(*serial);
                render::render_window(wm.windows.get_mut(&id).unwrap(), &shm, w, h);
            }
        })
        .on(|wm: &mut WindowManager, event: &wayland::XdgSurfaceEvent| {
            let wayland::XdgSurfaceEvent::Configure { sender, serial } = event;
            let id = WindowId(sender.object_id().expect("live handle"));
            let (w, h) = {
                let window = wm.windows.get(&id).expect("window exists for configure");
                (window.width, window.height)
            };
            let shm = wm.globals.shm.clone().expect("shm global");
            sender.ack_configure(*serial);
            render::render_window(wm.windows.get_mut(&id).unwrap(), &shm, w, h);
        })
        .on(|_: &mut WindowManager, event: &wayland::XdgWmBaseEvent| {
            let wayland::XdgWmBaseEvent::Ping { sender, serial } = event;
            sender.pong(*serial);
        })
}
