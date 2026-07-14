mod render;

use std::collections::HashSet;
use std::os::fd::AsRawFd;

use app::{RegisteredModule, prelude::*};
use io_ring::RingProxy;
use renderer::commands::{ClearColor, DrawMonochromeSprite, DrawQuad, DrawRect, DrawText};
use renderer::{DmaBuf, Renderer};
use wayland::{
    Handle, Interface, ObjectId, Wayland, WlBuffer, WlCompositor, WlSurface, XdgSurface,
    XdgToplevel, XdgWmBase, ZwpLinuxDmabufV1, *,
};

struct Globals {
    compositor: Option<Handle<WlCompositor>>,
    xdg_wm_base: Option<Handle<XdgWmBase>>,
    dmabuf: Option<Handle<ZwpLinuxDmabufV1>>,
}

pub struct Slot {
    pub surface: renderer::RenderableSurface<DmaBuf>,
    pub buffer: Handle<WlBuffer>,
    pub released: bool,
}

#[derive(State)]
pub struct ClientWindow {
    wayland: Wayland,
    globals: Globals,
    renderer: Renderer,
    width: u32,
    #[lens(skip)]
    height: u32,
    title: String,
    surface: Option<Handle<WlSurface>>,
    xdg_surface: Option<Handle<XdgSurface>>,
    toplevel: Option<Handle<XdgToplevel>>,
    slots: Option<[Slot; 2]>,
    buffer_ids: [Option<ObjectId>; 2],
    back: usize,
    frame_callbacks: HashSet<ObjectId>,
    blitting: bool,
}

impl ClientWindow {
    pub fn new(ring_proxy: RingProxy, width: u32, height: u32, title: impl Into<String>) -> Self {
        let wayland = Wayland::new(ring_proxy);
        let renderer = Renderer::new().expect("renderer init failed");
        Self {
            wayland,
            globals: Globals {
                compositor: None,
                xdg_wm_base: None,
                dmabuf: None,
            },
            renderer,
            width,
            height,
            title: title.into(),
            surface: None,
            xdg_surface: None,
            toplevel: None,
            slots: None,
            buffer_ids: [None, None],
            back: 0,
            frame_callbacks: HashSet::new(),
            blitting: false,
        }
    }

    pub fn renderer(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    fn start(&mut self) {
        self.renderer.init_command_queue::<ClearColor>();
        self.renderer.init_command_queue::<DrawRect>();
        self.renderer.init_command_queue::<DrawQuad>();
        self.renderer.init_command_queue::<DrawMonochromeSprite>();
        self.renderer.init_command_queue::<DrawText>();

        let display = self.wayland.display();
        display.get_registry();
        display.sync();
    }

    fn pre_poll(&mut self) {
        self.wayland.proxy().flush();
    }

    fn on_registry_global(
        &mut self,
        name: u32,
        interface: &str,
        version: u32,
        sender: &Handle<WlRegistry>,
    ) {
        match interface {
            WlCompositor::NAME => self.globals.compositor = Some(sender.bind(name, version)),
            XdgWmBase::NAME => self.globals.xdg_wm_base = Some(sender.bind(name, version)),
            ZwpLinuxDmabufV1::NAME => self.globals.dmabuf = Some(sender.bind(name, version)),
            _ => {}
        }
    }

    fn create_surface(&mut self) {
        let compositor = self
            .globals
            .compositor
            .clone()
            .expect("compositor global missing");
        let xdg_wm_base = self
            .globals
            .xdg_wm_base
            .clone()
            .expect("xdg_wm_base global missing");

        let surface = compositor.create_surface();
        let xdg_surface = xdg_wm_base.get_xdg_surface(&surface);
        let toplevel = xdg_surface.get_toplevel();
        toplevel.set_title(&self.title);
        surface.commit();

        self.surface = Some(surface);
        self.xdg_surface = Some(xdg_surface);
        self.toplevel = Some(toplevel);
    }

    fn configure(&mut self, w: u32, h: u32) {
        if self.slots.is_some() {
            return;
        }
        let dmabuf = self.globals.dmabuf.clone().expect("dmabuf global missing");
        let slots = render::alloc_slots(&mut self.renderer, &dmabuf, w, h);
        self.buffer_ids = [
            Some(slots[0].buffer.object_id().expect("live buffer")),
            Some(slots[1].buffer.object_id().expect("live buffer")),
        ];
        self.slots = Some(slots);
        self.width = w;
        self.height = h;
    }

    fn render_frame(&mut self) {
        let back = self.back;
        let width = self.width;
        let height = self.height;

        {
            let slots = self.slots.as_ref().expect("configured");
            self.renderer.active_surface(&slots[back].surface);
        }

        self.renderer.process_command_queue::<ClearColor>();
        self.renderer.process_command_queue::<DrawRect>();
        self.renderer.process_command_queue::<DrawQuad>();
        self.renderer
            .process_command_queue::<DrawMonochromeSprite>();
        self.renderer.process_command_queue::<DrawText>();
        self.renderer.finish();

        let surface = self.surface.as_ref().expect("configured");
        let slots = self.slots.as_mut().expect("configured");
        let next_frame = surface.frame();
        surface.attach(Some(&slots[back].buffer), 0, 0);
        surface.damage(0, 0, width as i32, height as i32);
        surface.commit();
        slots[back].released = false;
        self.frame_callbacks
            .insert(next_frame.object_id().expect("live callback"));
        self.back ^= 1;
    }

    fn is_back_released(&self) -> bool {
        self.slots.as_ref().map_or(false, |s| s[self.back].released)
    }

    pub fn commit_blitted_frame(&mut self) {
        let back = self.back;
        let width = self.width;
        let height = self.height;
        let surface = self.surface.as_ref().expect("surface created");
        let slots = self.slots.as_mut().expect("slots allocated");
        let next_frame = surface.frame();
        surface.attach(Some(&slots[back].buffer), 0, 0);
        surface.damage(0, 0, width as i32, height as i32);
        surface.commit();
        slots[back].released = false;
        self.frame_callbacks
            .insert(next_frame.object_id().expect("live callback"));
        self.back ^= 1;
        self.blitting = true;
    }

    pub fn back_buffer_info(&self) -> (std::os::fd::RawFd, u32, u32, u32) {
        let slots = self.slots.as_ref().expect("slots allocated");
        let slot = &slots[self.back];
        (
            slot.surface.backend.prime_fd.as_raw_fd(),
            slot.surface.backend.stride,
            slot.surface.width,
            slot.surface.height,
        )
    }

    fn on_buffer_release(&mut self, buffer_id: ObjectId) {
        for (i, id) in self.buffer_ids.iter().enumerate() {
            if *id == Some(buffer_id) {
                if let Some(slots) = self.slots.as_mut() {
                    slots[i].released = true;
                }
                break;
            }
        }
    }
}

pub fn module<S>() -> impl app::RegisteredModule<ClientWindow, S> {
    app::Module::new()
        .mount(wayland::module::<S>().into_module())
        .on(|cw: &mut ClientWindow, _: &app::Start| cw.start())
        .on(|cw: &mut ClientWindow, _: &app::PrePoll| cw.pre_poll())
        .on(|cw: &mut ClientWindow, event: &wayland::WlRegistryEvent| {
            if let wayland::WlRegistryEvent::Global {
                sender,
                name,
                interface,
                version,
            } = event
            {
                cw.on_registry_global(*name, interface, *version, sender);
            }
        })
        .on(|cw: &mut ClientWindow, event: &wayland::WlCallbackEvent| {
            let wayland::WlCallbackEvent::Done { sender, .. } = event;
            let obj_id = sender.object_id().expect("live callback");
            if cw.frame_callbacks.remove(&obj_id) {
                if !cw.blitting && cw.is_back_released() {
                    cw.render_frame();
                }
            } else {
                cw.create_surface();
            }
        })
        .on(|cw: &mut ClientWindow, event: &wayland::XdgSurfaceEvent| {
            let wayland::XdgSurfaceEvent::Configure { sender, serial } = event;
            sender.ack_configure(*serial);
            let (w, h) = (cw.width, cw.height);
            cw.configure(w, h);
            cw.render_frame();
        })
        .on(|_: &mut ClientWindow, event: &wayland::XdgWmBaseEvent| {
            let wayland::XdgWmBaseEvent::Ping { sender, serial } = event;
            sender.pong(*serial);
        })
        .on(|cw: &mut ClientWindow, event: &wayland::WlBufferEvent| {
            let wayland::WlBufferEvent::Release { sender } = event;
            let obj_id = sender.object_id().expect("live buffer");
            cw.on_buffer_release(obj_id);
        })
}
