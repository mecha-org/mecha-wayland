use event_manager::{Event, EventHandler, EventManagerContext, HasSchedule, ScheduleLabel};
use glow::HasContext;
use renderer::commands::{ClearColor, DrawMonochromeSprite, DrawQuad, DrawRect, DrawText};
use renderer::Renderer;

// ── Schedule labels ───────────────────────────────────────────────────────────

pub struct Init;
pub struct BeginFrame;
pub struct Update;
pub struct EndFrame;

impl ScheduleLabel for Init {}
impl ScheduleLabel for BeginFrame {}
impl ScheduleLabel for Update {}
impl ScheduleLabel for EndFrame {}

// ── Lifecycle payload types ───────────────────────────────────────────────────

#[derive(Clone)]
pub struct InitPayload;

#[derive(Clone)]
pub struct BeginFramePayload {
    pub fbo:    glow::Framebuffer,
    pub width:  u32,
    pub height: u32,
}

#[derive(Clone)]
pub struct EndFramePayload;

// ── RendererCommand<T> ────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RendererCommand<T: Clone + 'static>(pub T);

impl<T: Clone + 'static> Event for RendererCommand<T> {}

// ── HasSchedule impls ─────────────────────────────────────────────────────────

impl HasSchedule for RendererCommand<InitPayload> {
    type Label = Init;
}
impl HasSchedule for RendererCommand<BeginFramePayload> {
    type Label = BeginFrame;
}
impl HasSchedule for RendererCommand<EndFramePayload> {
    type Label = EndFrame;
}
impl HasSchedule for RendererCommand<ClearColor> {
    type Label = Update;
}
impl HasSchedule for RendererCommand<DrawRect> {
    type Label = Update;
}
impl HasSchedule for RendererCommand<DrawQuad> {
    type Label = Update;
}
impl HasSchedule for RendererCommand<DrawMonochromeSprite> {
    type Label = Update;
}
impl HasSchedule for RendererCommand<DrawText> {
    type Label = Update;
}

// ── RendererComponent ─────────────────────────────────────────────────────────

pub struct RendererComponent(pub Renderer);

impl EventHandler<RendererCommand<InitPayload>> for RendererComponent {
    fn handle(&mut self, _: RendererCommand<InitPayload>, _: &mut EventManagerContext) {
        self.0.init_command_queue::<ClearColor>();
        self.0.init_command_queue::<DrawRect>();
        self.0.init_command_queue::<DrawQuad>();
        self.0.init_command_queue::<DrawMonochromeSprite>();
        self.0.init_command_queue::<DrawText>();
    }
}

impl EventHandler<RendererCommand<BeginFramePayload>> for RendererComponent {
    fn handle(&mut self, event: RendererCommand<BeginFramePayload>, _: &mut EventManagerContext) {
        let bf = event.0;
        unsafe {
            self.0.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(bf.fbo));
            self.0.gl.viewport(0, 0, bf.width as i32, bf.height as i32);
        }
        self.0.set_width(bf.width);
        self.0.set_height(bf.height);
    }
}

impl EventHandler<RendererCommand<ClearColor>> for RendererComponent {
    fn handle(&mut self, event: RendererCommand<ClearColor>, _: &mut EventManagerContext) {
        self.0.send_command(event.0);
    }
}

impl EventHandler<RendererCommand<DrawRect>> for RendererComponent {
    fn handle(&mut self, event: RendererCommand<DrawRect>, _: &mut EventManagerContext) {
        self.0.send_command(event.0);
    }
}

impl EventHandler<RendererCommand<DrawQuad>> for RendererComponent {
    fn handle(&mut self, event: RendererCommand<DrawQuad>, _: &mut EventManagerContext) {
        self.0.send_command(event.0);
    }
}

impl EventHandler<RendererCommand<DrawMonochromeSprite>> for RendererComponent {
    fn handle(&mut self, event: RendererCommand<DrawMonochromeSprite>, _: &mut EventManagerContext) {
        self.0.send_command(event.0);
    }
}

impl EventHandler<RendererCommand<DrawText>> for RendererComponent {
    fn handle(&mut self, event: RendererCommand<DrawText>, _: &mut EventManagerContext) {
        self.0.send_command(event.0);
    }
}

impl EventHandler<RendererCommand<EndFramePayload>> for RendererComponent {
    fn handle(&mut self, _: RendererCommand<EndFramePayload>, _: &mut EventManagerContext) {
        self.0.process_command_queue::<ClearColor>();
        self.0.process_command_queue::<DrawRect>();
        self.0.process_command_queue::<DrawQuad>();
        self.0.process_command_queue::<DrawMonochromeSprite>();
        unsafe { self.0.gl.finish(); }
    }
}
