use anyhow::{Result, bail};
use gbm::{BufferObjectFlags, Format as GbmFormat};
use glow::HasContext;
use khronos_egl as egl;
use std::ffi::c_void;
use std::os::unix::io::{AsRawFd, OwnedFd};

use crate::{Renderer, SurfaceBackend};

// ── EGL_EXT_image_dma_buf_import constants ────────────────────────────────

const EGL_LINUX_DMA_BUF_EXT: u32 = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: i32 = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: i32 = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: i32 = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: i32 = 0x3274;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: i32 = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: i32 = 0x3444;
const DRM_FORMAT_ARGB8888: u32 = 0x34325241;

// ── DmaBuf ────────────────────────────────────────────────────────────────

/// Wayland-presentable render target: GBM buffer object imported as EGLImageKHR,
/// backed by an RBO and bound to an FBO for GL rendering.
///
/// To present: `dup` the `prime_fd` and attach it to a `wl_buffer` via
/// `zwp_linux_dmabuf_v1`. Keep one `RenderableSurface<DmaBuf>` per swap slot;
/// the caller manages buffer lifecycle and swap chain rotation.
pub struct DmaBuf {
    _gbm_bo: gbm::BufferObject<()>, // must outlive egl_image
    pub prime_fd: OwnedFd,          // DMA-BUF fd to hand to Wayland (dup before sending)
    pub egl_image: *mut c_void,     // EGLImageKHR wrapping the DMA-BUF memory
    pub stride: u32,                // bytes per row
    pub modifier: u64,              // DRM format modifier (for Wayland params)
    pub rbo: glow::Renderbuffer,    // GL renderbuffer backed by egl_image
    pub depth_rbo: glow::Renderbuffer, // depth renderbuffer at DEPTH_ATTACHMENT
    pub fbo: glow::Framebuffer,     // GL framebuffer with rbo at COLOR_ATTACHMENT0
}

// SAFETY: EGLImageKHR pointer and GL handles are only accessed from one thread.
unsafe impl Send for DmaBuf {}

impl SurfaceBackend for DmaBuf {
    fn allocate(renderer: &Renderer, width: u32, height: u32) -> Result<Self> {
        // ── Session 1-3: GBM BO → prime fd → EGLImageKHR ─────────────────

        let gbm_bo = renderer
            ._gbm_device
            .create_buffer_object::<()>(
                width,
                height,
                GbmFormat::Argb8888,
                BufferObjectFlags::RENDERING | BufferObjectFlags::LINEAR,
            )
            .map_err(|e| anyhow::anyhow!("gbm_bo_create: {e}"))?;

        let modifier = u64::from(gbm_bo.modifier());
        tracing::info!(modifier = format!("0x{modifier:016x}"), "gbm bo modifier");

        let prime_fd = gbm_bo
            .fd()
            .map_err(|e| anyhow::anyhow!("gbm_bo_get_fd: {e}"))?;
        let stride = gbm_bo.stride();
        let raw_display = renderer.display.as_ptr() as *const c_void;

        let import_attribs: [i32; 17] = [
            egl::WIDTH,
            width as i32,
            egl::HEIGHT,
            height as i32,
            EGL_LINUX_DRM_FOURCC_EXT,
            DRM_FORMAT_ARGB8888 as i32,
            EGL_DMA_BUF_PLANE0_FD_EXT,
            prime_fd.as_raw_fd(),
            EGL_DMA_BUF_PLANE0_OFFSET_EXT,
            0,
            EGL_DMA_BUF_PLANE0_PITCH_EXT,
            stride as i32,
            EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT,
            (modifier & 0xffff_ffff) as i32,
            EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT,
            (modifier >> 32) as i32,
            egl::NONE,
        ];

        let egl_image = unsafe {
            (renderer.fn_create_image)(
                raw_display,
                std::ptr::null(),
                EGL_LINUX_DMA_BUF_EXT,
                std::ptr::null(),
                import_attribs.as_ptr(),
            )
        };
        if egl_image.is_null() {
            bail!("eglCreateImageKHR(EGL_LINUX_DMA_BUF_EXT) failed");
        }
        tracing::info!(width, height, "DmaBuf EGLImageKHR created");

        // ── Session 4: RBO backed by the EGLImage ─────────────────────────

        let rbo = unsafe { renderer.gl.create_renderbuffer() }
            .map_err(|e| anyhow::anyhow!("glGenRenderbuffers: {e}"))?;
        unsafe {
            renderer.gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rbo));
            // Re-backs the RBO's storage with the EGLImage memory (writes go into the GBM BO).
            (renderer.fn_rbo_image)(glow::RENDERBUFFER, egl_image);
        }

        // ── Session 4: FBO with RBO at COLOR_ATTACHMENT0 ──────────────────

        let fbo = unsafe { renderer.gl.create_framebuffer() }
            .map_err(|e| anyhow::anyhow!("glGenFramebuffers: {e}"))?;
        let depth_rbo = unsafe { renderer.gl.create_renderbuffer() }
            .map_err(|e| anyhow::anyhow!("glGenRenderbuffers (depth): {e}"))?;
        unsafe {
            renderer.gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth_rbo));
            renderer.gl.renderbuffer_storage(
                glow::RENDERBUFFER,
                glow::DEPTH_COMPONENT24,
                width as i32,
                height as i32,
            );
        }

        unsafe {
            renderer.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            renderer.gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::RENDERBUFFER,
                Some(rbo),
            );
            renderer.gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(depth_rbo),
            );
            let status = renderer.gl.check_framebuffer_status(glow::FRAMEBUFFER);
            if status != glow::FRAMEBUFFER_COMPLETE {
                bail!("FBO incomplete: 0x{:x}", status);
            }
        }
        tracing::info!(width, height, "DmaBuf FBO complete");

        Ok(Self {
            _gbm_bo: gbm_bo,
            prime_fd,
            egl_image,
            stride,
            modifier,
            rbo,
            depth_rbo,
            fbo,
        })
    }

    fn fbo(&self) -> glow::Framebuffer {
        self.fbo
    }

    fn destroy(self, renderer: &Renderer) {
        unsafe {
            renderer.gl.delete_framebuffer(self.fbo);
            renderer.gl.delete_renderbuffer(self.depth_rbo);
            renderer.gl.delete_renderbuffer(self.rbo);
            (renderer.fn_destroy_image)(renderer.display.as_ptr() as *const c_void, self.egl_image);
        }
        // prime_fd (OwnedFd) and _gbm_bo drop naturally here
    }
}
