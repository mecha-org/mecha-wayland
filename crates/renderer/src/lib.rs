use anyhow::{Context as _, Result, bail};
use gbm::{AsRaw, Device as GbmDevice};
use glow::HasContext;
use khronos_egl as egl;
use std::{collections::HashMap, ffi::c_void};

pub mod dmabuf;
pub use dmabuf::DmaBuf;

pub mod image_surface;
pub use image_surface::ImageSurface;

pub mod texture;
use texture::GpuTexture;
pub use texture::{TextureFormat, TextureId};

use crate::commands::{Command, CommandQueueRegistry, RenderContext};
pub mod commands;

// ── EGL platform extension ─────────────────────────────────────────────────

// EGL_MESA_platform_gbm — not in the core EGL headers, must be hardcoded.
const EGL_PLATFORM_GBM_MESA: egl::Enum = 0x31D7;

// ── Extension function pointer types ──────────────────────────────────────

type EglGetPlatformDisplayEXT = unsafe extern "system" fn(
    platform: egl::Enum,
    native_display: *mut c_void,
    attrib_list: *const egl::Int,
) -> *mut c_void;

pub(crate) type PfnCreateImageKHR = unsafe extern "C" fn(
    *const c_void,
    *const c_void,
    u32,
    *const c_void,
    *const i32,
) -> *mut c_void;

pub(crate) type PfnDestroyImageKHR = unsafe extern "C" fn(*const c_void, *mut c_void) -> u32;

pub(crate) type PfnRboImageOES = unsafe extern "C" fn(target: u32, image: *mut c_void);

// ── SurfaceBackend trait ───────────────────────────────────────────────────

/// A backing store for a `RenderableSurface`. Implement this to add new render
/// target types (DMA-BUF for Wayland, GL textures for offscreen, etc.).
///
/// Surfaces are created via `Renderer::create_surface::<B>()` and destroyed via
/// `Renderer::destroy_surface()`. There is no `Drop` — cleanup requires access
/// to the live `Renderer`.
pub trait SurfaceBackend: Sized {
    /// Allocate the backing storage using the renderer's shared context.
    fn allocate(renderer: &Renderer, width: u32, height: u32) -> Result<Self>;

    /// Release all resources owned by this backend.
    /// Called by `Renderer::destroy_surface` — do not call directly.
    fn destroy(self, renderer: &Renderer);

    /// The framebuffer object to bind when drawing into this surface.
    fn fbo(&self) -> glow::Framebuffer;

    /// Read all RGBA pixels from this surface into a `Vec<u8>`.
    ///
    /// The buffer is `width * height * 4` bytes, RGBA order, row 0 at the bottom
    /// (OpenGL convention). The surface's FBO is bound temporarily; call
    /// `active_surface` again before the next draw. This default implementation
    /// works for all backends — override only if a more efficient path exists.
    fn read_pixels(&self, renderer: &Renderer, width: u32, height: u32) -> Vec<u8> {
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        unsafe {
            renderer
                .gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo()));
            renderer.gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
        }
        pixels
    }
}

// ── RenderableSurface<B> ───────────────────────────────────────────────────

/// A GPU render target backed by any `SurfaceBackend`.
///
/// Create with `Renderer::create_surface::<B>(width, height)`.
/// Destroy with `Renderer::destroy_surface(surface)`.
///
/// Do not drop without calling `destroy_surface` — backend resources will leak.
pub struct RenderableSurface<B: SurfaceBackend> {
    pub width: u32,
    pub height: u32,
    pub fbo: glow::Framebuffer,
    pub backend: B,
}

impl<B: SurfaceBackend> RenderableSurface<B> {
    /// Read all RGBA pixels from this surface into a `Vec<u8>`.
    /// Forwards to [`SurfaceBackend::read_pixels`] with the surface dimensions.
    pub fn read_pixels(&self, renderer: &Renderer) -> Vec<u8> {
        self.backend.read_pixels(renderer, self.width, self.height)
    }
}

// ── Renderer ───────────────────────────────────────────────────────────────

/// GPU device. Owns the EGL display, GL context, and GBM allocator.
///
/// Does not track surfaces or manage swap chains — that is the caller's
/// responsibility. Use `create_surface` / `destroy_surface` to manage render
/// targets. All surfaces must be destroyed before dropping the `Renderer`.
pub struct Renderer {
    pub _gbm_device: GbmDevice<std::fs::File>,
    egl: egl::DynamicInstance<egl::EGL1_4>,
    pub display: egl::Display,
    context: egl::Context,
    pub gl: glow::Context,
    pub(crate) command_queue_registry: CommandQueueRegistry,
    pub(crate) fn_create_image: PfnCreateImageKHR,
    pub(crate) fn_destroy_image: PfnDestroyImageKHR,
    pub(crate) fn_rbo_image: PfnRboImageOES,
    textures: HashMap<TextureId, GpuTexture>,
    next_texture_id: u32,
    viewport_width: u32,
    viewport_height: u32,
}

// SAFETY: accessed from one thread only.
unsafe impl Send for Renderer {}

impl Renderer {
    pub fn new() -> Result<Self> {
        let drm_render_node = open_drm_render_node()?;
        let gbm_device = GbmDevice::new(drm_render_node)?;

        let egl_lib =
            unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required().context("load EGL")? };

        let get_platform_display: EglGetPlatformDisplayEXT = {
            let raw = egl_lib
                .get_proc_address("eglGetPlatformDisplayEXT")
                .context(
                    "eglGetPlatformDisplayEXT not found — driver lacks EGL_EXT_platform_base",
                )?;
            // SAFETY: we verified the symbol exists and its signature matches the extension spec.
            unsafe { std::mem::transmute(raw) }
        };

        let raw_display = unsafe {
            get_platform_display(
                EGL_PLATFORM_GBM_MESA,
                gbm_device.as_raw() as *mut c_void,
                std::ptr::null(),
            )
        };

        if raw_display.is_null() {
            bail!("eglGetPlatformDisplayEXT returned EGL_NO_DISPLAY");
        }

        // SAFETY: we checked for null above.
        let display = unsafe { egl::Display::from_ptr(raw_display) };

        let (major, minor) = egl_lib.initialize(display).context("eglInitialize")?;
        tracing::info!("EGL version: {}.{}", major, minor);

        // Pick a config: RGBA8, GLES2-renderable.
        #[rustfmt::skip]
        let config_attribs = [
            egl::RED_SIZE,        8,
            egl::GREEN_SIZE,      8,
            egl::BLUE_SIZE,       8,
            egl::ALPHA_SIZE,      8,
            egl::RENDERABLE_TYPE, egl::OPENGL_ES2_BIT,
            egl::NONE,
        ];

        let config = egl_lib
            .choose_first_config(display, &config_attribs)
            .context("eglChooseConfig")?
            .context("no EGL config matched RGBA8 + GLES2")?;

        egl_lib.bind_api(egl::OPENGL_ES_API).context("eglBindAPI")?;

        let context_attribs = [egl::CONTEXT_CLIENT_VERSION, 2, egl::NONE];

        let context = egl_lib
            .create_context(display, config, None, &context_attribs)
            .context("eglCreateContext")?;

        // Surfaceless context: passing None for both draw and read surfaces
        // relies on EGL_KHR_surfaceless_context. FBO 0 does not exist in this
        // mode — a custom FBO is required for any rendering.
        egl_lib
            .make_current(display, None, None, Some(context))
            .context("eglMakeCurrent (surfaceless)")?;

        tracing::info!("EGL context is current (surfaceless)");

        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                egl_lib
                    .get_proc_address(s)
                    .map(|f| f as *const _)
                    .unwrap_or(std::ptr::null())
            })
        };

        let fn_create_image: PfnCreateImageKHR = unsafe {
            std::mem::transmute(
                egl_lib
                    .get_proc_address("eglCreateImageKHR")
                    .context("eglCreateImageKHR not found — driver lacks EGL_KHR_image_base")?,
            )
        };
        let fn_destroy_image: PfnDestroyImageKHR = unsafe {
            std::mem::transmute(
                egl_lib
                    .get_proc_address("eglDestroyImageKHR")
                    .context("eglDestroyImageKHR not found")?,
            )
        };

        let fn_rbo_image: PfnRboImageOES = unsafe {
            std::mem::transmute(
                egl_lib
                    .get_proc_address("glEGLImageTargetRenderbufferStorageOES")
                    .context("glEGLImageTargetRenderbufferStorageOES not found — driver lacks GL_OES_EGL_image")?,
            )
        };

        let command_queue_registry = CommandQueueRegistry::new();
        Ok(Self {
            _gbm_device: gbm_device,
            egl: egl_lib,
            display,
            context,
            gl,
            command_queue_registry,
            fn_create_image,
            fn_destroy_image,
            fn_rbo_image,
            textures: HashMap::new(),
            next_texture_id: 0,
            viewport_width: 0,
            viewport_height: 0,
        })
    }

    /// Allocate a new render target backed by `B`.
    pub fn create_surface<B: SurfaceBackend>(
        &self,
        width: u32,
        height: u32,
    ) -> Result<RenderableSurface<B>> {
        let backend = B::allocate(self, width, height)?;
        let fbo = backend.fbo();
        Ok(RenderableSurface {
            width,
            height,
            fbo,
            backend,
        })
    }

    /// Release all resources owned by `surface`.
    ///
    /// All surfaces must be destroyed before dropping the `Renderer`.
    pub fn destroy_surface<B: SurfaceBackend>(&self, surface: RenderableSurface<B>) {
        let RenderableSurface { backend, .. } = surface;
        backend.destroy(self);
    }

    pub fn set_width(&mut self, width: u32) {
        self.viewport_width = width;
    }

    pub fn set_height(&mut self, height: u32) {
        self.viewport_height = height;
    }

    /// Bind `surface` as the current draw target, set the GL viewport to its
    /// dimensions, and update the renderer's internal viewport so command queues
    /// use the correct pixel sizes. Call once per frame before `send_command` /
    /// `process_command_queue`. Replaces the manual bind + viewport pattern.
    pub fn active_surface<B: SurfaceBackend>(&mut self, surface: &RenderableSurface<B>) {
        unsafe {
            self.gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(surface.fbo));
            self.gl
                .viewport(0, 0, surface.width as i32, surface.height as i32);
        }
        self.viewport_width = surface.width;
        self.viewport_height = surface.height;
    }

    /// Upload pixel data as a GPU texture and return its `TextureId`.
    /// `data` must be `width * height` bytes for `TextureFormat::R8`.
    /// The texture is owned by the renderer for its lifetime.
    pub fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        format: TextureFormat,
        data: &[u8],
    ) -> Result<TextureId> {
        let handle = unsafe {
            let t = self
                .gl
                .create_texture()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            self.gl.bind_texture(glow::TEXTURE_2D, Some(t));
            self.gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);

            // GLES 2.0 lacks GL_R8 / GL_RED; GL_LUMINANCE maps the single channel
            // to RGB (R channel readable as .r in the shader) with alpha = 1.0.
            let (internal_fmt, pixel_fmt) = match format {
                TextureFormat::R8 => (glow::LUMINANCE as i32, glow::LUMINANCE),
            };

            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                internal_fmt,
                width as i32,
                height as i32,
                0,
                pixel_fmt,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(data)),
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.gl.bind_texture(glow::TEXTURE_2D, None);
            t
        };

        let id = TextureId(self.next_texture_id);
        self.next_texture_id += 1;
        self.textures.insert(
            id,
            GpuTexture {
                handle,
                width,
                height,
            },
        );
        Ok(id)
    }

    /// Decode a grayscale PNG atlas and upload it as an R8 texture.
    /// Pass `atlas.png_bytes` from a generated atlas constant.
    pub fn upload_atlas(&mut self, png_bytes: &[u8]) -> Result<TextureId> {
        let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
        let mut reader = decoder.read_info().context("decode atlas PNG header")?;
        let mut buf = vec![
            0u8;
            reader
                .output_buffer_size()
                .context("atlas PNG has unknown size")?
        ];
        let info = reader
            .next_frame(&mut buf)
            .context("decode atlas PNG frame")?;
        self.create_texture(
            info.width,
            info.height,
            TextureFormat::R8,
            &buf[..info.buffer_size()],
        )
    }

    pub fn init_command_queue<C: Command>(&mut self) {
        let ctx = RenderContext {
            gl: &self.gl,
            viewport_width: self.viewport_width,
            viewport_height: self.viewport_height,
            textures: &self.textures,
        };
        self.command_queue_registry.init_queue::<C>(&ctx);
    }

    pub fn send_command<C: Command>(&mut self, command: C) {
        self.command_queue_registry.enqueue(command);
    }

    pub fn process_command_queue<C: Command>(&mut self) {
        let ctx = RenderContext {
            gl: &self.gl,
            viewport_width: self.viewport_width,
            viewport_height: self.viewport_height,
            textures: &self.textures,
        };
        self.command_queue_registry.process::<C>(&ctx);
    }

    /// Block until all pending GPU commands have completed. Call this after
    /// rendering into a DMA-buf surface and before committing it to the
    /// compositor, so the compositor never reads a partially-rendered buffer.
    pub fn finish(&self) {
        unsafe { self.gl.finish() };
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_context(self.display, self.context);
        let _ = self.egl.terminate(self.display);
    }
}

fn open_drm_render_node() -> Result<std::fs::File> {
    for i in 128..=255 {
        let path = format!("/dev/dri/renderD{}", i);
        if let Ok(f) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
        {
            return Ok(f);
        }
    }
    bail!("no DRM render node found in /dev/dri/renderD128..255")
}
