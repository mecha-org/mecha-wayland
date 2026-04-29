use crate::object::Object;
use crate::{
    ZwlrLayerShellV1, ZwlrLayerShellV1Handler, ZwlrLayerSurfaceV1,
    ZwlrLayerSurfaceV1ConfigureEvent, ZwlrLayerSurfaceV1Handler,
};

pub struct LayerShell {
    pub inner: ZwlrLayerShellV1,
}

impl LayerShell {
    pub fn new(inner: ZwlrLayerShellV1) -> Self {
        LayerShell { inner }
    }
}

impl Object for LayerShell {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl ZwlrLayerShellV1Handler for LayerShell {}

pub struct LayerSurface {
    pub inner: ZwlrLayerSurfaceV1,
    pub pending_ack: Option<u32>,
    pub configured: bool,
    pub closed: bool,
    pub width: u32,
    pub height: u32,
}

impl LayerSurface {
    pub fn new(inner: ZwlrLayerSurfaceV1) -> Self {
        LayerSurface {
            inner,
            pending_ack: None,
            configured: false,
            closed: false,
            width: 0,
            height: 0,
        }
    }
}

impl Object for LayerSurface {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl ZwlrLayerSurfaceV1Handler for LayerSurface {
    fn on_configure(&mut self, event: ZwlrLayerSurfaceV1ConfigureEvent) {
        tracing::debug!(
            serial = event.serial,
            width = event.width,
            height = event.height,
            "zwlr_layer_surface_v1::configure"
        );
        self.pending_ack = Some(event.serial);
        self.configured = true;
        self.width = event.width;
        self.height = event.height;
    }

    fn on_closed(&mut self) {
        tracing::info!("zwlr_layer_surface_v1::closed");
        self.closed = true;
    }
}
