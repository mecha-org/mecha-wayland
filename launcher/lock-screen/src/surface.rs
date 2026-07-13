use std::collections::HashMap;
use std::os::fd::AsRawFd;

use renderer::{DmaBuf, RenderableSurface, Renderer};
use wayland::Wayland;

const DRM_FORMAT_ARGB8888: u32 = 0x34325241;

pub struct Surface {
    pub wl_surface_id: u32,
    pub size: (i32, i32),
    pub dmabuf: [Option<RenderableSurface<DmaBuf>>; 2],
    pub wl_buf_ids: [u32; 2],
    pub buf_in_flight: [bool; 2],
    pub frame_callback_pending: bool,
    pub dirty: bool,
}

impl Surface {
    pub fn new(wl_surface_id: u32) -> Self {
        Self {
            wl_surface_id,
            size: (0, 0),
            dmabuf: [None, None],
            wl_buf_ids: [0, 0],
            buf_in_flight: [false, false],
            frame_callback_pending: false,
            dirty: false,
        }
    }

    /// Allocate two DMA-buf back-buffers and register them with the compositor.
    /// Cleans up any previously allocated surfaces first to prevent resource leaks.
    pub fn alloc_buffers(
        &mut self,
        renderer: &mut Renderer,
        wayland: &mut Wayland,
        w: i32,
        h: i32,
    ) {
        self.size = (w, h);

        let s0 = renderer
            .create_surface::<DmaBuf>(w as u32, h as u32)
            .expect("dmabuf 0");
        let s1 = renderer
            .create_surface::<DmaBuf>(w as u32, h as u32)
            .expect("dmabuf 1");

        let id0 = create_wl_buffer(wayland, &s0, w, h);
        let id1 = create_wl_buffer(wayland, &s1, w, h);
        wayland.wl_buffer.register(id0);
        wayland.wl_buffer.register(id1);

        self.dmabuf = [Some(s0), Some(s1)];
        self.wl_buf_ids = [id0, id1];
        self.buf_in_flight = [false, false];
    }

    /// Pick a free buffer index, or `None` if both are in-flight.
    fn free_buf_idx(&self) -> Option<usize> {
        if !self.buf_in_flight[0] {
            Some(0)
        } else if !self.buf_in_flight[1] {
            Some(1)
        } else {
            None
        }
    }

    /// Submit a new frame immediately.
    pub fn present(
        &mut self,
        renderer: &mut Renderer,
        wayland: &mut Wayland,
        callback_map: &mut HashMap<u32, u32>,
        draw: impl FnOnce(&mut Renderer),
    ) -> bool {
        let Some(idx) = self.free_buf_idx() else {
            return false;
        };

        renderer.active_surface(self.dmabuf[idx].as_ref().unwrap());
        draw(renderer);

        let (w, h) = self.size;
        wayland
            .surface
            .attach(self.wl_surface_id, self.wl_buf_ids[idx], 0, 0);
        wayland.surface.damage(self.wl_surface_id, 0, 0, w, h);

        let cb_id = wayland.surface.frame(self.wl_surface_id);
        wayland.callback.register_frame(cb_id);
        callback_map.insert(cb_id, self.wl_surface_id);

        wayland.surface.commit(self.wl_surface_id);
        self.buf_in_flight[idx] = true;
        wayland.flush();

        true
    }

    /// Request a redraw: submit immediately if possible, otherwise mark dirty.
    pub fn request_redraw(
        &mut self,
        renderer: &mut Renderer,
        wayland: &mut Wayland,
        callback_map: &mut HashMap<u32, u32>,
        draw: impl FnOnce(&mut Renderer),
    ) {
        if self.frame_callback_pending {
            self.dirty = true;
        } else if self.present(renderer, wayland, callback_map, draw) {
            self.frame_callback_pending = true;
            self.dirty = false;
        }
    }

    /// Call when `WlCallbackEvent::Done` fires for this surface.
    ///
    /// Clears the pending flag and re-presents if content is dirty.
    pub fn on_frame_done(
        &mut self,
        renderer: &mut Renderer,
        wayland: &mut Wayland,
        callback_map: &mut HashMap<u32, u32>,
        draw: impl FnOnce(&mut Renderer),
    ) {
        self.frame_callback_pending = false;
        if self.dirty {
            if self.present(renderer, wayland, callback_map, draw) {
                self.frame_callback_pending = true;
                self.dirty = false;
            }
        }
    }

    /// Mark a buffer as no longer in-flight.
    ///
    /// Returns `true` if the released buffer belongs to this surface.
    pub fn on_buffer_release(&mut self, buf_id: u32) -> bool {
        for i in 0..2 {
            if self.wl_buf_ids[i] == buf_id {
                self.buf_in_flight[i] = false;
                return true;
            }
        }
        false
    }

    /// Release GPU resources. The Wayland surface object itself is left for
    /// the compositor to clean up when the session lock is destroyed.
    pub fn destroy(self, renderer: &mut Renderer) {
        for dmabuf in self.dmabuf.into_iter().flatten() {
            renderer.destroy_surface(dmabuf);
        }
    }
}

/// Import a DMA-buf into the compositor and return the `wl_buffer` id.
fn create_wl_buffer(
    wayland: &mut Wayland,
    surface: &RenderableSurface<DmaBuf>,
    width: i32,
    height: i32,
) -> u32 {
    let modifier = surface.backend.modifier;
    let modifier_hi = (modifier >> 32) as u32;
    let modifier_lo = (modifier & 0xffff_ffff) as u32;

    let fd = unsafe { libc::dup(surface.backend.prime_fd.as_raw_fd()) };
    if fd < 0 {
        panic!(
            "failed to dup prime fd: {}",
            std::io::Error::last_os_error()
        );
    }

    let params_id = wayland.dmabuf.create_params();
    wayland.buf_params.register(params_id);
    wayland.buf_params.add(
        params_id,
        fd,
        0,
        0,
        surface.backend.stride,
        modifier_hi,
        modifier_lo,
    );

    let buf_id = wayland
        .buf_params
        .create_immed(params_id, width, height, DRM_FORMAT_ARGB8888, 0);
    wayland.buf_params.destroy(params_id);
    buf_id
}
