use interactivity::InteractivityState;
use interactivity::hit::{HitArea, HitAreaRegistry};
use renderer::commands::{
    ClearColor, Color, DrawMonochromeSprite, DrawQuad, DrawRect, DrawText, Rect, Size as RSize,
};
use renderer::{DmaBuf, Renderer};
use taffy::{AvailableSpace, NodeId, Size, Style};
use ui::{Point, RenderCommand, WidgetList, WidgetTree};
use wayland::{
    Handle, ObjectId, WlBuffer, WlCallback, WlKeyboardEvent, WlPointerEvent, WlSurface,
    WlTouchEvent, XdgSurface, XdgToplevel, ZwlrLayerSurfaceV1, ZwpLinuxDmabufV1,
};
pub use wayland::{
    ZwlrLayerShellV1Layer, ZwlrLayerSurfaceV1Anchor, ZwlrLayerSurfaceV1KeyboardInteractivity,
};

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub struct WindowId(pub(crate) ObjectId);

pub struct WindowSettings {
    pub width: u32,
    pub height: u32,
    pub clear_color: Color,
    pub kind: WindowKind,
    pub touch_config: Option<interactivity::touch::TouchConfig>,
    pub gesture_config: Option<interactivity::gesture::GestureConfig>,
}

pub enum WindowKind {
    Xdg {
        title: String,
    },
    LayerShell {
        layer: ZwlrLayerShellV1Layer,
        anchor: ZwlrLayerSurfaceV1Anchor,
        exclusive_zone: i32,
        namespace: String,
        keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity,
    },
}

pub(crate) enum WindowKindHandles {
    LayerShell {
        layer_surface: Handle<ZwlrLayerSurfaceV1>,
    },
    Xdg {
        xdg_surface: Handle<XdgSurface>,
        toplevel: Handle<XdgToplevel>,
    },
}

pub struct Slot {
    pub surface: renderer::RenderableSurface<DmaBuf>,
    pub buffer: Handle<WlBuffer>,
    pub released: bool,
}

pub(crate) trait AnyWindow {
    fn init(&mut self, surface: Handle<WlSurface>, kind: WindowKindHandles);
    fn configure(
        &mut self,
        renderer: &mut Renderer,
        dmabuf: &Handle<ZwpLinuxDmabufV1>,
        w: u32,
        h: u32,
    );
    fn is_configured(&self) -> bool;
    fn dimensions(&self) -> (u32, u32);
    fn request_frame(&self) -> Handle<WlCallback>;
    fn is_back_released(&self) -> bool;
    fn render_frame(&mut self, renderer: &mut Renderer) -> Handle<WlCallback>;
    fn on_buffer_release(&mut self, buffer_id: ObjectId);
    fn surface(&self) -> &Handle<WlSurface>;
    fn on_pointer_event(&mut self, ev: &WlPointerEvent);
    fn on_keyboard_event(&mut self, ev: &WlKeyboardEvent);
    fn on_touch_event(&mut self, ev: &WlTouchEvent);
    fn wants_input(&self) -> bool;
}

pub struct Window<T> {
    surface: Option<Handle<WlSurface>>,
    slots: Option<[Slot; 2]>,
    buffer_ids: [Option<ObjectId>; 2],
    back: usize,
    width: u32,
    height: u32,
    clear_color: Color,
    kind: Option<WindowKindHandles>,
    tree: WidgetTree,
    root_node: Option<NodeId>,
    pub ui: T,
    pub interactivity: InteractivityState,
    pub hit_areas: HitAreaRegistry,
}

impl<T: WidgetList> Window<T> {
    pub fn new(
        width: u32,
        height: u32,
        clear_color: Color,
        ui: T,
        touch_config: Option<interactivity::touch::TouchConfig>,
        gesture_config: Option<interactivity::gesture::GestureConfig>,
    ) -> Self {
        Self {
            surface: None,
            slots: None,
            buffer_ids: [None, None],
            back: 0,
            width,
            height,
            clear_color,
            kind: None,
            tree: WidgetTree::new(),
            root_node: None,
            ui,
            interactivity: InteractivityState::with_configs(touch_config, gesture_config),
            hit_areas: HitAreaRegistry::new(),
        }
    }
}

impl<T: WidgetList + 'static> AnyWindow for Window<T> {
    fn init(&mut self, surface: Handle<WlSurface>, kind: WindowKindHandles) {
        self.surface = Some(surface);
        self.kind = Some(kind);
    }

    fn configure(
        &mut self,
        renderer: &mut Renderer,
        dmabuf: &Handle<ZwpLinuxDmabufV1>,
        w: u32,
        h: u32,
    ) {
        if self.slots.is_some() {
            return;
        }

        let slots = crate::render::alloc_slots(renderer, dmabuf, w, h);
        self.buffer_ids = [
            Some(slots[0].buffer.object_id().expect("live buffer")),
            Some(slots[1].buffer.object_id().expect("live buffer")),
        ];
        self.slots = Some(slots);
        self.width = w;
        self.height = h;

        let child_ids = self.ui.build_children(&mut self.tree);
        let root_node = self
            .tree
            .new_with_children(
                Style {
                    size: Size {
                        width: taffy::Dimension::percent(1.0),
                        height: taffy::Dimension::percent(1.0),
                    },
                    ..Style::default()
                },
                &child_ids,
            )
            .expect("root node");
        self.root_node = Some(root_node);
    }

    fn is_configured(&self) -> bool {
        self.slots.is_some()
    }

    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn request_frame(&self) -> Handle<WlCallback> {
        self.surface.as_ref().expect("surface initialized").frame()
    }

    fn is_back_released(&self) -> bool {
        self.slots.as_ref().map_or(false, |s| s[self.back].released)
    }

    fn render_frame(&mut self, renderer: &mut Renderer) -> Handle<WlCallback> {
        let back = self.back;
        let root_node = self.root_node.expect("configured");
        let width = self.width;
        let height = self.height;
        let clear_color = self.clear_color;

        ui::compute_layout(
            &mut self.tree,
            root_node,
            Size {
                width: AvailableSpace::Definite(width as f32),
                height: AvailableSpace::Definite(height as f32),
            },
        );

        let commands = self.ui.render_children(&self.tree, Point::ZERO);

        self.hit_areas.clear();
        for cmd in &commands {
            if let RenderCommand::RegisterHitArea { id, rect } = cmd {
                self.hit_areas.push(HitArea {
                    id: *id,
                    rect: *rect,
                });
            }
        }

        {
            let slots = self.slots.as_ref().expect("configured");
            renderer.active_surface(&slots[back].surface);
        }

        renderer.send_command(ClearColor(clear_color));

        for cmd in commands {
            match cmd {
                RenderCommand::DrawQuad {
                    color,
                    border_color,
                    origin,
                    z,
                    size,
                    border_radius,
                    border_thickness,
                } => {
                    renderer.send_command(DrawQuad {
                        color,
                        border_color,
                        origin,
                        z,
                        size,
                        border_radius,
                        border_thickness,
                    });
                }
                RenderCommand::DrawText {
                    font,
                    text,
                    origin,
                    z,
                    color,
                } => {
                    let texture_id = renderer.get_texture_id(font.atlas_id);
                    renderer.send_command(DrawText {
                        font,
                        texture_id,
                        text,
                        origin,
                        z,
                        color,
                    });
                }
                RenderCommand::DrawMonochromeSprite {
                    atlas_id,
                    region,
                    origin,
                    z,
                    size,
                    color,
                } => {
                    let texture_id = renderer.get_texture_id(atlas_id);
                    renderer.send_command(DrawMonochromeSprite {
                        texture_id,
                        region: Rect::new(region.x, region.y, region.w, region.h),
                        origin,
                        z,
                        size: RSize::new(size.width(), size.height()),
                        color,
                    });
                }
                _ => {}
            }
        }

        renderer.process_command_queue::<ClearColor>();
        renderer.process_command_queue::<DrawRect>();
        renderer.process_command_queue::<DrawQuad>();
        renderer.process_command_queue::<DrawMonochromeSprite>();
        renderer.process_command_queue::<DrawText>();
        renderer.finish();

        let surface = self.surface.as_ref().expect("configured");
        let slots = self.slots.as_mut().expect("configured");
        let next_frame = surface.frame();
        surface.attach(Some(&slots[back].buffer), 0, 0);
        surface.damage(0, 0, width as i32, height as i32);
        surface.commit();
        slots[back].released = false;
        self.back ^= 1;
        next_frame
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

    fn surface(&self) -> &Handle<WlSurface> {
        self.surface.as_ref().expect("surface initialized")
    }

    fn on_pointer_event(&mut self, ev: &WlPointerEvent) {
        self.interactivity.call_before_frame();
        self.interactivity.process_pointer(ev);
        self.ui.on_event(&self.interactivity, &mut self.tree);
    }

    fn on_keyboard_event(&mut self, ev: &WlKeyboardEvent) {
        self.interactivity.call_before_frame();
        self.interactivity.process_keyboard(ev);
        self.ui.on_event(&self.interactivity, &mut self.tree);
    }

    fn on_touch_event(&mut self, ev: &WlTouchEvent) {
        self.interactivity.call_before_frame();
        self.interactivity.process_touch(ev);
        self.ui.on_event(&self.interactivity, &mut self.tree);
    }

    fn wants_input(&self) -> bool {
        self.ui.wants_input()
    }
}
