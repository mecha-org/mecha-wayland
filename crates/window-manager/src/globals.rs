use wayland::{Handle, WlCompositor, WlOutput, WlShm, XdgWmBase, ZwlrLayerShellV1};

#[derive(Default)]
pub struct WaylandGlobals {
    pub compositor: Option<Handle<WlCompositor>>,
    pub shm: Option<Handle<WlShm>>,
    pub output: Option<Handle<WlOutput>>,
    pub layer_shell: Option<Handle<ZwlrLayerShellV1>>,
    pub xdg_wm_base: Option<Handle<XdgWmBase>>,
}
