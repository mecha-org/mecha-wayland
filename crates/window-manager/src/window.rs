use std::marker::PhantomData;
use wayland::{Handle, ObjectId, WlBuffer, WlSurface, XdgSurface, XdgToplevel, ZwlrLayerSurfaceV1};
pub use wayland::{ZwlrLayerShellV1Layer, ZwlrLayerSurfaceV1Anchor};

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub struct WindowId(pub(crate) ObjectId);

pub struct WindowSettings {
    pub width: u32,
    pub height: u32,
    pub color: u32,
    pub kind: WindowKind,
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

pub struct Window<T> {
    pub(crate) surface: Handle<WlSurface>,
    pub(crate) buffer: Option<Handle<WlBuffer>>,
    pub(crate) color: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) kind: WindowKindHandles,
    pub(crate) _phantom: PhantomData<T>,
}
