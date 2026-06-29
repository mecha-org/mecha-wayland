use wayland::{Handle, WlCompositor, WlKeyboard, WlOutput, WlPointer, WlSeat, WlTouch, XdgWmBase, ZwlrLayerShellV1, ZwpLinuxDmabufV1};

#[derive(Default)]
pub struct WaylandGlobals {
    pub compositor: Option<Handle<WlCompositor>>,
    pub output: Option<Handle<WlOutput>>,
    pub layer_shell: Option<Handle<ZwlrLayerShellV1>>,
    pub xdg_wm_base: Option<Handle<XdgWmBase>>,
    pub dmabuf: Option<Handle<ZwpLinuxDmabufV1>>,
    pub seat: Option<Handle<WlSeat>>,
    pub pointer: Option<Handle<WlPointer>>,
    pub keyboard: Option<Handle<WlKeyboard>>,
    pub touch: Option<Handle<WlTouch>>,
}
