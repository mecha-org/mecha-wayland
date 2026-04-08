pub mod atlas;
pub mod gpu_image;
pub mod primitives;
pub mod scene;
pub mod surface;
pub mod text;

use std::any::TypeId;
use std::collections::HashMap;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::os::unix::io::{AsFd, AsRawFd, OwnedFd};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use gbm::{AsRaw, BufferObjectFlags, Device as GbmDevice, Format as GbmFormat};
use glow::HasContext;
use khronos_egl as egl;

pub use gpu_image::{GpuImage, GpuImageProcessor, GpuTextureId};
pub use primitives::{AttribDesc, Image, MonoSprite, Quad, Rect, RenderablePrimitive};
pub use scene::{PrimitiveId, Scene};
pub use surface::{DmaBufSurface, RenderableSurface};
pub use text::{TextMetrics, TextSystem};

// ── EGL extension types ────────────────────────────────────────────────────

const EGL_PLATFORM_GBM_MESA: u32 = 0x31D7;
const EGL_LINUX_DMA_BUF_EXT: u32 = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: i32 = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: i32 = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: i32 = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: i32 = 0x3274;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: i32 = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: i32 = 0x3444;
const DRM_FORMAT_ARGB8888: u32 = 0x34325241;

type PfnGetPlatformDisplay =
    unsafe extern "C" fn(u32, *const c_void, *const i32) -> *const c_void;
type PfnCreateImageKHR = unsafe extern "C" fn(
    *const c_void,
    *const c_void,
    u32,
    *const c_void,
    *const i32,
) -> *mut c_void;
type PfnDestroyImageKHR = unsafe extern "C" fn(*const c_void, *mut c_void) -> u32;
type PfnImageTargetRenderbufferStorageOES = unsafe extern "C" fn(u32, *mut c_void);

// ── DmaBufFrame (presented to Wayland) ────────────────────────────────────

pub struct DmaBufFrame {
    pub fd:       OwnedFd,
    pub stride:   u32,
    pub offset:   u32,
    pub format:   u32,
    pub modifier: u64,
}

// ── Erased pipeline trait ──────────────────────────────────────────────────

trait ErasedPipeline {
    fn draw_erased(
        &self,
        gl:       &glow::Context,
        instances: &[u8],
        count:    usize,
        texture:  Option<glow::Texture>,
        viewport: (u32, u32),
    );
}

// ── Typed pipeline ─────────────────────────────────────────────────────────

struct Pipeline<P: RenderablePrimitive> {
    program:    glow::Program,
    vao:        glow::VertexArray,
    vbo:        glow::Buffer,
    u_viewport: Option<glow::UniformLocation>,
    u_atlas:    Option<glow::UniformLocation>,
    _phantom:   PhantomData<fn() -> P>,
}

impl<P: RenderablePrimitive> Pipeline<P> {
    fn new(gl: &glow::Context) -> Result<Self> {
        Self::new_with_src(gl, P::vert_src(), P::frag_src())
    }

    fn new_with_src(gl: &glow::Context, vert_src: &str, frag_src: &str) -> Result<Self> {
        let program = compile_program(gl, vert_src, frag_src)?;
        let u_viewport = unsafe { gl.get_uniform_location(program, "u_viewport") };
        let u_atlas    = unsafe { gl.get_uniform_location(program, "u_atlas") };

        let (vao, vbo) = unsafe {
            let vao = gl.create_vertex_array().map_err(|e| anyhow::anyhow!("{e}"))?;
            let vbo = gl.create_buffer().map_err(|e| anyhow::anyhow!("{e}"))?;

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));

            // Pre-allocate for 256 instances; grows with DYNAMIC_DRAW on each frame.
            let init = 256 * std::mem::size_of::<P::InstanceData>() as i32;
            gl.buffer_data_size(glow::ARRAY_BUFFER, init, glow::DYNAMIC_DRAW);

            for a in P::attrib_layout() {
                gl.enable_vertex_attrib_array(a.location);
                gl.vertex_attrib_pointer_f32(
                    a.location, a.size, glow::FLOAT, false, a.stride, a.offset,
                );
                gl.vertex_attrib_divisor(a.location, 1);
            }

            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            (vao, vbo)
        };

        Ok(Self { program, vao, vbo, u_viewport, u_atlas, _phantom: PhantomData })
    }
}

impl<P: RenderablePrimitive> ErasedPipeline for Pipeline<P> {
    fn draw_erased(
        &self,
        gl:       &glow::Context,
        instances: &[u8],
        count:    usize,
        texture:  Option<glow::Texture>,
        viewport: (u32, u32),
    ) {
        if count == 0 { return; }

        unsafe {
            // Upload instance data — orphans old buffer, avoiding pipeline stalls.
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, instances, glow::DYNAMIC_DRAW);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);

            gl.use_program(Some(self.program));

            if let Some(u) = &self.u_viewport {
                gl.uniform_2_f32(Some(u), viewport.0 as f32, viewport.1 as f32);
            }

            if let (Some(tex), Some(u)) = (texture, &self.u_atlas) {
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.uniform_1_i32(Some(u), 0);
            }

            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, count as i32);
            gl.bind_vertex_array(None);

            if texture.is_some() {
                gl.bind_texture(glow::TEXTURE_2D, None);
            }
            gl.use_program(None);
        }
    }
}

// ── Renderer ───────────────────────────────────────────────────────────────

pub struct Renderer {
    pub gl:       glow::Context,
    egl_lib:      egl::DynamicInstance<egl::EGL1_4>,
    display:      egl::Display,
    context:      egl::Context,
    rbo:          glow::Renderbuffer,
    fbo:          glow::Framebuffer,
    egl_image:    *mut c_void,
    frame_stride: u32,
    fn_destroy_image: PfnDestroyImageKHR,
    width:        u32,
    height:       u32,
    bo_prime_fd:  OwnedFd,
    #[allow(dead_code)]
    gbm_bo:       gbm::BufferObject<()>,
    _gbm_device:  GbmDevice<std::fs::File>,
    pipelines:    HashMap<TypeId, Box<dyn ErasedPipeline>>,
    deletion_queue: Arc<Mutex<Vec<GpuTextureId>>>,
}

// SAFETY: EGLImageKHR pointer is only accessed from one thread.
unsafe impl Send for Renderer {}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let drm_file = open_drm_render_node()?;
        let gbm_device =
            GbmDevice::new(drm_file).map_err(|e| anyhow::anyhow!("gbm_create_device: {}", e))?;

        let egl_lib = unsafe {
            egl::DynamicInstance::<egl::EGL1_4>::load_required().context("load EGL")?
        };

        let fn_get_platform_display: PfnGetPlatformDisplay = unsafe {
            std::mem::transmute(
                egl_lib.get_proc_address("eglGetPlatformDisplayEXT")
                    .context("eglGetPlatformDisplayEXT")?,
            )
        };
        let fn_create_image: PfnCreateImageKHR = unsafe {
            std::mem::transmute(
                egl_lib.get_proc_address("eglCreateImageKHR")
                    .context("eglCreateImageKHR")?,
            )
        };
        let fn_destroy_image: PfnDestroyImageKHR = unsafe {
            std::mem::transmute(
                egl_lib.get_proc_address("eglDestroyImageKHR")
                    .context("eglDestroyImageKHR")?,
            )
        };
        let fn_image_target_rbo: PfnImageTargetRenderbufferStorageOES = unsafe {
            std::mem::transmute(
                egl_lib.get_proc_address("glEGLImageTargetRenderbufferStorageOES")
                    .context("glEGLImageTargetRenderbufferStorageOES")?,
            )
        };

        let raw_dpy = unsafe {
            fn_get_platform_display(
                EGL_PLATFORM_GBM_MESA,
                gbm_device.as_raw() as *const c_void,
                std::ptr::null(),
            )
        };
        if raw_dpy.is_null() {
            bail!("eglGetPlatformDisplayEXT(GBM) returned EGL_NO_DISPLAY");
        }
        let display = unsafe { egl::Display::from_ptr(raw_dpy as *mut c_void) };

        egl_lib.initialize(display).context("eglInitialize")?;
        egl_lib.bind_api(egl::OPENGL_ES_API).context("bind GLES API")?;

        let config_attribs = [
            egl::RENDERABLE_TYPE, egl::OPENGL_ES2_BIT,
            egl::RED_SIZE,   8,
            egl::GREEN_SIZE, 8,
            egl::BLUE_SIZE,  8,
            egl::ALPHA_SIZE, 8,
            egl::NONE,
        ];
        let config = egl_lib
            .choose_first_config(display, &config_attribs)
            .context("eglChooseConfig")?
            .context("no suitable EGL config")?;

        // GLES3 context for instanced rendering, R8 textures, gl_VertexID.
        let ctx_attribs = [egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE];
        let context = egl_lib
            .create_context(display, config, None, &ctx_attribs)
            .context("create EGL context")?;

        egl_lib
            .make_current(display, None, None, Some(context))
            .context("eglMakeCurrent (surfaceless)")?;

        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                let sym = std::ffi::CString::new(s).unwrap();
                egl_lib
                    .get_proc_address(sym.to_str().unwrap())
                    .map(|f| f as *const _)
                    .unwrap_or(std::ptr::null())
            })
        };

        let gbm_bo = gbm_device
            .create_buffer_object::<()>(
                width, height,
                GbmFormat::Argb8888,
                BufferObjectFlags::RENDERING | BufferObjectFlags::LINEAR,
            )
            .map_err(|e| anyhow::anyhow!("gbm_bo_create: {}", e))?;

        tracing::info!(
            modifier = format!("0x{:016x}", u64::from(gbm_bo.modifier())),
            "gbm bo modifier"
        );

        let bo_fd = gbm_bo.fd().map_err(|e| anyhow::anyhow!("gbm_bo_get_fd: {}", e))?;
        let frame_stride = gbm_bo.stride();

        let raw_display = display.as_ptr() as *const c_void;
        let import_attribs: [i32; 17] = [
            egl::WIDTH,  width as i32,
            egl::HEIGHT, height as i32,
            EGL_LINUX_DRM_FOURCC_EXT, DRM_FORMAT_ARGB8888 as i32,
            EGL_DMA_BUF_PLANE0_FD_EXT,         bo_fd.as_raw_fd(),
            EGL_DMA_BUF_PLANE0_OFFSET_EXT,     0,
            EGL_DMA_BUF_PLANE0_PITCH_EXT,      frame_stride as i32,
            EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT, 0,
            EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT, 0,
            egl::NONE,
        ];
        let egl_image = unsafe {
            (fn_create_image)(
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

        let rbo = unsafe {
            let rbo = gl.create_renderbuffer().map_err(|e| anyhow::anyhow!("{e}"))?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rbo));
            fn_image_target_rbo(glow::RENDERBUFFER, egl_image);
            rbo
        };

        let fbo = unsafe {
            let fbo = gl.create_framebuffer().map_err(|e| anyhow::anyhow!("{e}"))?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(rbo),
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            if status != glow::FRAMEBUFFER_COMPLETE {
                bail!("FBO incomplete: 0x{:x}", status);
            }
            gl.viewport(0, 0, width as i32, height as i32);
            fbo
        };

        unsafe {
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        }

        Ok(Self {
            gl,
            egl_lib,
            display,
            context,
            rbo,
            fbo,
            egl_image,
            frame_stride,
            fn_destroy_image,
            width,
            height,
            bo_prime_fd: bo_fd,
            gbm_bo,
            _gbm_device: gbm_device,
            pipelines: HashMap::new(),
            deletion_queue: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Compile shaders and set up the instanced draw pipeline for primitive type `P`.
    pub fn register<P: RenderablePrimitive>(&mut self) -> Result<()> {
        let pipeline = Pipeline::<P>::new(&self.gl)?;
        self.pipelines.insert(TypeId::of::<P>(), Box::new(pipeline));
        Ok(())
    }

    /// Upload an [`ImageAsset`](utils::image::ImageAsset) to a GPU texture.
    ///
    /// Returns a [`GpuImage`] whose [`id()`](GpuImage::id) can be passed to [`Image`].
    /// The texture is deleted automatically when all `GpuImage` handles are dropped.
    pub fn upload_image(&self, image: &utils::image::ImageAsset) -> Result<GpuImage> {
        upload_image_to_gpu(&self.gl, image, Arc::clone(&self.deletion_queue))
    }

    /// Return a [`GpuImageProcessor`] tied to this renderer's GL context.
    ///
    /// Pass the processor to [`AssetManager::process_pending`] to automatically
    /// upload all pending [`ImageAsset`](utils::image::ImageAsset)s to the GPU:
    ///
    /// ```ignore
    /// asset_manager.process_pending(&mut renderer.image_processor());
    /// let gpu_image = asset_manager.get_processed::<ImageAsset, GpuImage>(&handle).unwrap();
    /// ```
    pub fn image_processor(&self) -> GpuImageProcessor<'_> {
        GpuImageProcessor {
            gl: &self.gl,
            deletion_queue: Arc::clone(&self.deletion_queue),
        }
    }

    /// Like [`register`](Self::register), but uses caller-supplied GLSL source
    /// instead of `P::vert_src()` / `P::frag_src()`.
    ///
    /// Use this with a [`ShaderAsset`](utils::shader::ShaderAsset) loaded via
    /// `AssetManager` to register a primitive with runtime-loaded or file-based shaders:
    ///
    /// ```ignore
    /// let s = assets.get(&shader_handle).unwrap();
    /// renderer.register_with_shader::<MyPrim>(&s.vert_src, &s.frag_src)?;
    /// ```
    pub fn register_with_shader<P: RenderablePrimitive>(
        &mut self,
        vert_src: &str,
        frag_src: &str,
    ) -> Result<()> {
        let pipeline = Pipeline::<P>::new_with_src(&self.gl, vert_src, frag_src)?;
        self.pipelines.insert(TypeId::of::<P>(), Box::new(pipeline));
        Ok(())
    }

    /// Borrow the underlying GL context (needed by TextSystem, GlyphAtlas, …).
    pub fn gl(&self) -> &glow::Context { &self.gl }

    pub fn create_scene(&self) -> Scene { Scene::new() }

    pub fn create_dmabuf_surface(&self) -> DmaBufSurface {
        DmaBufSurface { fbo: self.fbo, width: self.width, height: self.height }
    }

    /// Clear the surface to `bg` and set up for primitive draws.
    pub fn begin_frame(&self, surface: &impl RenderableSurface, bg: (f32, f32, f32)) {
        let (w, h) = surface.size();
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(surface.fbo()));
            self.gl.viewport(0, 0, w as i32, h as i32);
            self.gl.clear_color(bg.0, bg.1, bg.2, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }

    /// Draw all `P`-type primitives from `scene` onto `surface`.
    pub fn render_primitive<P: RenderablePrimitive>(
        &mut self,
        scene:   &Scene,
        surface: &impl RenderableSurface,
    ) -> Result<()> {
        // Drain deferred texture deletions.
        if let Ok(mut q) = self.deletion_queue.lock() {
            for id in q.drain(..) {
                unsafe { self.gl.delete_texture(id.0); }
            }
        }

        let pipeline = self
            .pipelines
            .get(&TypeId::of::<P>())
            .ok_or_else(|| anyhow::anyhow!("pipeline not registered — call register::<P>() first"))?;

        let Some(layer) = scene.get_layer(TypeId::of::<P>()) else { return Ok(()); };
        if layer.count == 0 { return Ok(()); }

        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(surface.fbo()));
        }

        pipeline.draw_erased(&self.gl, &layer.instances, layer.count, layer.texture, surface.size());
        Ok(())
    }

    /// Signal rendering is complete for this frame.
    pub fn end_frame(&self) {
        unsafe { self.gl.finish(); }
    }

    /// Dup the GBM BO fd and return a `DmaBufFrame` for Wayland presentation.
    pub fn present(&mut self) -> Result<DmaBufFrame> {
        let fd = self
            .bo_prime_fd
            .as_fd()
            .try_clone_to_owned()
            .map_err(|e| anyhow::anyhow!("dup(bo_prime_fd): {}", e))?;
        Ok(DmaBufFrame {
            fd,
            stride:   self.frame_stride,
            offset:   0,
            format:   DRM_FORMAT_ARGB8888,
            modifier: 0,
        })
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let raw_display = self.display.as_ptr() as *const c_void;
        unsafe { (self.fn_destroy_image)(raw_display, self.egl_image) };
        unsafe {
            self.gl.delete_framebuffer(self.fbo);
            self.gl.delete_renderbuffer(self.rbo);
        }
        let _ = self.egl_lib.make_current(self.display, None, None, None);
        let _ = self.egl_lib.destroy_context(self.display, self.context);
        let _ = self.egl_lib.terminate(self.display);
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

pub(crate) fn upload_image_to_gpu(
    gl: &glow::Context,
    image: &utils::image::ImageAsset,
    deletion_queue: Arc<Mutex<Vec<GpuTextureId>>>,
) -> Result<GpuImage> {
    let texture = unsafe {
        let tex = gl.create_texture().map_err(|e| anyhow::anyhow!("{e}"))?;
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            image.width as i32,
            image.height as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(&image.pixels)),
        );
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        gl.bind_texture(glow::TEXTURE_2D, None);
        tex
    };
    Ok(GpuImage::new(texture, image.width, image.height, deletion_queue))
}

fn compile_program(
    gl:       &glow::Context,
    vert_src: &str,
    frag_src: &str,
) -> Result<glow::Program> {
    unsafe {
        let vert = gl.create_shader(glow::VERTEX_SHADER).map_err(|e| anyhow::anyhow!("{e}"))?;
        gl.shader_source(vert, vert_src);
        gl.compile_shader(vert);
        if !gl.get_shader_compile_status(vert) {
            bail!("vert shader: {}", gl.get_shader_info_log(vert));
        }

        let frag = gl.create_shader(glow::FRAGMENT_SHADER).map_err(|e| anyhow::anyhow!("{e}"))?;
        gl.shader_source(frag, frag_src);
        gl.compile_shader(frag);
        if !gl.get_shader_compile_status(frag) {
            bail!("frag shader: {}", gl.get_shader_info_log(frag));
        }

        let prog = gl.create_program().map_err(|e| anyhow::anyhow!("{e}"))?;
        gl.attach_shader(prog, vert);
        gl.attach_shader(prog, frag);
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            bail!("link: {}", gl.get_program_info_log(prog));
        }
        gl.detach_shader(prog, vert);
        gl.detach_shader(prog, frag);
        gl.delete_shader(vert);
        gl.delete_shader(frag);
        Ok(prog)
    }
}

fn open_drm_render_node() -> Result<std::fs::File> {
    for i in 128..=255 {
        let path = format!("/dev/dri/renderD{}", i);
        if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
            return Ok(f);
        }
    }
    bail!("no DRM render node found in /dev/dri/renderD128..255")
}
