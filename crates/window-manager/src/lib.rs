mod globals;
pub mod prelude;
mod render;
mod window;

use app::{RegisteredModule, prelude::State};
use io_ring::RingProxy;
use renderer::Renderer;
use renderer::commands::{ClearColor, DrawMonochromeSprite, DrawQuad, DrawRect, DrawText};
use std::collections::HashMap;
use ui::WidgetList;
use wayland::{Interface, *};

use globals::WaylandGlobals;
use window::{AnyWindow, Window, WindowKindHandles};

pub use renderer::commands::Color;
pub use ui::WidgetList as WindowUi;
pub use window::{
    WindowId, WindowKind, WindowSettings, ZwlrLayerShellV1Layer, ZwlrLayerSurfaceV1Anchor,
    ZwlrLayerSurfaceV1KeyboardInteractivity,
};

#[derive(State)]
pub struct WindowManager {
    wayland: Wayland,
    globals: WaylandGlobals,
    renderer: Renderer,
    pending: Vec<(WindowSettings, Box<dyn AnyWindow>)>,
    windows: HashMap<WindowId, Box<dyn AnyWindow>>,
    frame_callbacks: HashMap<ObjectId, WindowId>,
    #[lens(skip)]
    wl_surfaces: HashMap<ObjectId, WindowId>,
    current_pointer_window: Option<WindowId>,
    #[lens(skip)]
    current_keyboard_window: Option<WindowId>,
    touch_window_map: HashMap<i32, WindowId>,
}

impl WindowManager {
    pub fn new(ring_proxy: RingProxy) -> Self {
        let wayland = Wayland::new(ring_proxy.clone());
        let renderer = Renderer::new().expect("renderer init failed");
        Self {
            wayland,
            globals: WaylandGlobals::default(),
            renderer,
            pending: Vec::new(),
            windows: HashMap::new(),
            frame_callbacks: HashMap::new(),
            wl_surfaces: HashMap::new(),
            current_pointer_window: None,
            current_keyboard_window: None,
            touch_window_map: HashMap::new(),
        }
    }

    pub fn start(&mut self) {
        self.renderer.init_command_queue::<ClearColor>();
        self.renderer.init_command_queue::<DrawRect>();
        self.renderer.init_command_queue::<DrawQuad>();
        self.renderer.init_command_queue::<DrawMonochromeSprite>();
        self.renderer.init_command_queue::<DrawText>();

        let display = self.wayland.display();
        display.get_registry();
        display.sync();
    }

    pub fn pre_poll(&mut self) {
        self.wayland.proxy().flush();
    }

    pub fn poll(&mut self) {}

    pub fn upload_atlas(&mut self, atlas: &assets::AtlasData) {
        self.renderer
            .upload_atlas(atlas)
            .expect("atlas upload failed");
    }

    pub fn spawn_window<T: WidgetList + 'static>(&mut self, settings: WindowSettings, ui: T) {
        let touch_config = settings.touch_config.or_else(|| ui.touch_config());
        let gesture_config = settings.gesture_config.or_else(|| ui.gesture_config());

        let window = Box::new(Window::new(
            settings.width,
            settings.height,
            settings.clear_color,
            ui,
            touch_config,
            gesture_config,
        ));
        self.pending.push((settings, window));
    }

    pub fn request_frame(&mut self, id: WindowId) {
        let window = self.windows.get(&id).expect("window exists");
        if !window.is_configured() {
            return;
        }
        let cb = window.request_frame();
        let obj_id = cb.object_id().expect("live callback");
        self.frame_callbacks.insert(obj_id, id);
    }

    fn flush_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for (settings, mut window) in pending {
            let WindowSettings {
                width,
                height,
                kind,
                ..
            } = settings;
            match kind {
                WindowKind::LayerShell {
                    layer,
                    anchor,
                    exclusive_zone,
                    namespace,
                    keyboard_interactivity,
                } => {
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
                    layer_surface.set_keyboard_interactivity(keyboard_interactivity);
                    surface.commit();

                    let id = WindowId(layer_surface.object_id().expect("just allocated"));
                    window.init(surface, WindowKindHandles::LayerShell { layer_surface });
                    let surface_id = window.surface().object_id().expect("surface initialized");
                    self.wl_surfaces.insert(surface_id, id);
                    self.windows.insert(id, window);
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
                    window.init(
                        surface,
                        WindowKindHandles::Xdg {
                            xdg_surface,
                            toplevel,
                        },
                    );
                    let surface_id = window.surface().object_id().expect("surface initialized");
                    self.wl_surfaces.insert(surface_id, id);
                    self.windows.insert(id, window);
                }
            }
        }
    }

    fn configure_window(&mut self, window_id: WindowId, w: u32, h: u32) {
        let dmabuf = self.globals.dmabuf.clone().expect("dmabuf global missing");
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.configure(&mut self.renderer, &dmabuf, w, h);
        }
    }

    fn do_render_frame(&mut self, window_id: WindowId) {
        if let Some(window) = self.windows.get_mut(&window_id) {
            let cb = window.render_frame(&mut self.renderer);
            let cb_id = cb.object_id().expect("live callback");
            self.frame_callbacks.insert(cb_id, window_id);
        }
    }
}

pub fn module<S>() -> impl app::RegisteredModule<WindowManager, S> {
    app::Module::new()
        .mount(wayland::module::<S>().into_module())
        .on(|wm: &mut WindowManager, _: &app::Start| wm.start())
        .on(|wm: &mut WindowManager, _: &app::PrePoll| wm.pre_poll())
        .on(|wm: &mut WindowManager, _: &app::Poll| wm.poll())
        .on(|wm: &mut WindowManager, event: &wayland::WlRegistryEvent| {
            if let wayland::WlRegistryEvent::Global {
                sender,
                name,
                interface,
                version,
            } = event
            {
                match interface.as_str() {
                    WlCompositor::NAME => {
                        wm.globals.compositor = Some(sender.bind(*name, *version))
                    }
                    ZwlrLayerShellV1::NAME => {
                        wm.globals.layer_shell = Some(sender.bind(*name, *version))
                    }
                    WlOutput::NAME => wm.globals.output = Some(sender.bind(*name, *version)),
                    XdgWmBase::NAME => wm.globals.xdg_wm_base = Some(sender.bind(*name, *version)),
                    ZwpLinuxDmabufV1::NAME => {
                        wm.globals.dmabuf = Some(sender.bind(*name, *version))
                    }
                    WlSeat::NAME => wm.globals.seat = Some(sender.bind(*name, *version)),
                    _ => {}
                }
            }
        })
        .on(|wm: &mut WindowManager, event: &wayland::WlSeatEvent| {
            if let wayland::WlSeatEvent::Capabilities { capabilities, .. } = event {
                let seat = wm
                    .globals
                    .seat
                    .clone()
                    .expect("seat bound before capabilities");
                if capabilities.contains(WlSeatCapability::Pointer) && wm.globals.pointer.is_none()
                {
                    wm.globals.pointer = Some(seat.get_pointer());
                }
                if capabilities.contains(WlSeatCapability::Keyboard)
                    && wm.globals.keyboard.is_none()
                {
                    wm.globals.keyboard = Some(seat.get_keyboard());
                }
                if capabilities.contains(WlSeatCapability::Touch) && wm.globals.touch.is_none() {
                    wm.globals.touch = Some(seat.get_touch());
                }
            }
        })
        .on(
            |wm: &mut WindowManager, event: &wayland::WlPointerEvent| match event {
                WlPointerEvent::Enter { surface, .. } => {
                    let surface_id = surface.object_id().expect("live surface");
                    wm.current_pointer_window = wm.wl_surfaces.get(&surface_id).copied();
                    if let Some(id) = wm.current_pointer_window {
                        if let Some(w) = wm.windows.get_mut(&id) {
                            w.on_pointer_event(event);
                        }
                    }
                }
                WlPointerEvent::Leave { .. } => {
                    if let Some(id) = wm.current_pointer_window.take() {
                        if let Some(w) = wm.windows.get_mut(&id) {
                            w.on_pointer_event(event);
                        }
                    }
                }
                _ => {
                    if let Some(id) = wm.current_pointer_window {
                        if let Some(w) = wm.windows.get_mut(&id) {
                            w.on_pointer_event(event);
                        }
                    }
                }
            },
        )
        .on(
            |wm: &mut WindowManager, event: &wayland::WlKeyboardEvent| match event {
                WlKeyboardEvent::Enter { surface, .. } => {
                    let surface_id = surface.object_id().expect("live surface");
                    wm.current_keyboard_window = wm.wl_surfaces.get(&surface_id).copied();
                    if let Some(id) = wm.current_keyboard_window {
                        if let Some(w) = wm.windows.get_mut(&id) {
                            w.on_keyboard_event(event);
                        }
                    }
                }
                WlKeyboardEvent::Leave { .. } => {
                    if let Some(id) = wm.current_keyboard_window.take() {
                        if let Some(w) = wm.windows.get_mut(&id) {
                            w.on_keyboard_event(event);
                        }
                    }
                }
                _ => {
                    if let Some(id) = wm.current_keyboard_window {
                        if let Some(w) = wm.windows.get_mut(&id) {
                            w.on_keyboard_event(event);
                        }
                    }
                }
            },
        )
        .on(
            |wm: &mut WindowManager, event: &wayland::WlTouchEvent| match event {
                WlTouchEvent::Down { surface, id, .. } => {
                    if let Some(surface_id) = surface.object_id() {
                        if let Some(&window_id) = wm.wl_surfaces.get(&surface_id) {
                            wm.touch_window_map.insert(*id, window_id);
                            if let Some(w) = wm.windows.get_mut(&window_id) {
                                w.on_touch_event(event);
                            }
                        }
                    }
                }
                WlTouchEvent::Up { id, .. } => {
                    if let Some(window_id) = wm.touch_window_map.remove(id) {
                        if let Some(w) = wm.windows.get_mut(&window_id) {
                            w.on_touch_event(event);
                        }
                    }
                }
                WlTouchEvent::Motion { id, .. } => {
                    if let Some(&window_id) = wm.touch_window_map.get(id) {
                        if let Some(w) = wm.windows.get_mut(&window_id) {
                            w.on_touch_event(event);
                        }
                    }
                }
                WlTouchEvent::Frame { .. } => {
                    let mut seen = std::collections::HashSet::new();
                    for &window_id in wm.touch_window_map.values() {
                        if seen.insert(window_id) {
                            if let Some(w) = wm.windows.get_mut(&window_id) {
                                w.on_touch_event(event);
                            }
                        }
                    }
                }
                WlTouchEvent::Cancel { .. } => {
                    let window_ids: Vec<WindowId> = wm.touch_window_map.values().copied().collect();
                    wm.touch_window_map.clear();
                    let mut seen = std::collections::HashSet::new();
                    for window_id in window_ids {
                        if seen.insert(window_id) {
                            if let Some(w) = wm.windows.get_mut(&window_id) {
                                w.on_touch_event(event);
                            }
                        }
                    }
                }
                _ => {}
            },
        )
        .on(|wm: &mut WindowManager, event: &wayland::WlCallbackEvent| {
            let wayland::WlCallbackEvent::Done { sender, .. } = event;
            let obj_id = sender.object_id().expect("live callback");

            if let Some(window_id) = wm.frame_callbacks.remove(&obj_id) {
                if wm
                    .windows
                    .get(&window_id)
                    .map_or(false, |w| w.is_back_released())
                {
                    wm.do_render_frame(window_id);
                }
            } else {
                wm.flush_pending();
            }
        })
        .on(|wm: &mut WindowManager, event: &wayland::WlBufferEvent| {
            let wayland::WlBufferEvent::Release { sender } = event;
            let obj_id = sender.object_id().expect("live buffer");
            for window in wm.windows.values_mut() {
                window.on_buffer_release(obj_id);
            }
        })
        .on(
            |wm: &mut WindowManager, event: &wayland::ZwlrLayerSurfaceV1Event| {
                if let wayland::ZwlrLayerSurfaceV1Event::Configure {
                    sender,
                    serial,
                    width,
                    height,
                } = event
                {
                    let id = WindowId(sender.object_id().expect("live handle"));
                    let (stored_w, stored_h) = wm
                        .windows
                        .get(&id)
                        .expect("window exists for configure")
                        .dimensions();
                    let w = if *width == 0 { stored_w } else { *width };
                    let h = if *height == 0 { stored_h } else { *height };
                    sender.ack_configure(*serial);
                    wm.configure_window(id, w, h);
                    wm.do_render_frame(id);
                }
            },
        )
        .on(|wm: &mut WindowManager, event: &wayland::XdgSurfaceEvent| {
            let wayland::XdgSurfaceEvent::Configure { sender, serial } = event;
            let id = WindowId(sender.object_id().expect("live handle"));
            let (w, h) = wm
                .windows
                .get(&id)
                .expect("window exists for configure")
                .dimensions();
            sender.ack_configure(*serial);
            wm.configure_window(id, w, h);
            wm.do_render_frame(id);
        })
        .on(|_: &mut WindowManager, event: &wayland::XdgWmBaseEvent| {
            let wayland::XdgWmBaseEvent::Ping { sender, serial } = event;
            sender.pong(*serial);
        })
}
