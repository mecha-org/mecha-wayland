//! The render node, GBM, EGL and the context. The one module that loads
//! libraries and calls into them raw; every later module borrows `gl`
//! from here.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::File;

use glow::HasContext;
use khronos_egl as egl;

/// `EGL_PLATFORM_GBM_KHR`, not in the crate's constants.
const PLATFORM_GBM: egl::Enum = 0x31D7;

pub(crate) type Egl = egl::DynamicInstance<egl::EGL1_5>;

/// `glEGLImageTargetRenderbufferStorageOES`, fetched through EGL because
/// glow has no binding for it.
pub(crate) type ImageTargetRenderbuffer =
    unsafe extern "system" fn(target: u32, image: *const c_void);

pub(crate) struct Gpu {
    pub(crate) egl: Egl,
    pub(crate) display: egl::Display,
    pub(crate) context: egl::Context,
    /// Keeps the device fd open for the display's life, and closes it
    /// once the last buffer is gone.
    pub(crate) gbm: crate::gbm::Device,
    pub(crate) gl: glow::Context,
    pub(crate) version: String,
    pub(crate) image_target_renderbuffer: ImageTargetRenderbuffer,
}

fn setup<T>(step: &str, r: Result<T, impl std::fmt::Debug>) -> Result<T, crate::Error> {
    r.map_err(|e| crate::Error::Setup(format!("{step}: {e:?}")))
}

fn render_node() -> Result<File, crate::Error> {
    (128..=255)
        .map(|i| format!("/dev/dri/renderD{i}"))
        .find_map(|p| File::options().read(true).write(true).open(p).ok())
        .ok_or(crate::Error::NoDevice)
}

impl Gpu {
    pub(crate) fn open() -> Result<Gpu, crate::Error> {
        let node = render_node()?;
        let gbm = crate::gbm::Device::new(node).map_err(crate::Error::Setup)?;
        // SAFETY: libEGL.so.1 is the system's EGL; nothing else is loaded.
        let egl = setup("load libEGL.so.1", unsafe { Egl::load_required() })?;
        // SAFETY: the gbm device pointer is live for as long as `gbm` is.
        let display = setup("eglGetPlatformDisplay(GBM)", unsafe {
            egl.get_platform_display(PLATFORM_GBM, gbm.as_ptr(), &[egl::ATTRIB_NONE])
        })?;
        setup("eglInitialize", egl.initialize(display))?;
        setup(
            "eglBindAPI(OPENGL_ES_API)",
            egl.bind_api(egl::OPENGL_ES_API),
        )?;
        #[rustfmt::skip]
        let attribs = [
            egl::RED_SIZE, 8,
            egl::GREEN_SIZE, 8,
            egl::BLUE_SIZE, 8,
            egl::ALPHA_SIZE, 8,
            egl::RENDERABLE_TYPE, egl::OPENGL_ES3_BIT,
            egl::SURFACE_TYPE, 0,
            egl::NONE,
        ];
        let config = setup(
            "eglChooseConfig",
            egl.choose_first_config(display, &attribs),
        )?
        .ok_or_else(|| crate::Error::Setup("no EGL config offers GLES 3.0 with RGBA8".into()))?;
        let context = setup(
            "eglCreateContext(GLES 3)",
            egl.create_context(
                display,
                config,
                None,
                &[egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE],
            ),
        )?;
        setup(
            "eglMakeCurrent (surfaceless)",
            egl.make_current(display, None, None, Some(context)),
        )?;
        // SAFETY: the context is current on this thread; eglGetProcAddress
        // resolves GLES entry points for it.
        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                egl.get_proc_address(s)
                    .map_or(std::ptr::null(), |f| f as *const c_void)
            })
        };
        // SAFETY: a valid current context.
        let version = unsafe { gl.get_parameter_string(glow::VERSION) };
        if !version.starts_with("OpenGL ES 3.") {
            return Err(crate::Error::Setup(format!(
                "the context is {version}, GLES 3.0 is required"
            )));
        }
        let f = egl
            .get_proc_address("glEGLImageTargetRenderbufferStorageOES")
            .ok_or_else(|| {
                crate::Error::Setup("glEGLImageTargetRenderbufferStorageOES is missing".into())
            })?;
        // SAFETY: the OES_EGL_image entry point has exactly this signature.
        let image_target_renderbuffer: ImageTargetRenderbuffer = unsafe { std::mem::transmute(f) };
        Ok(Gpu {
            egl,
            display,
            context,
            gbm,
            gl,
            version,
            image_target_renderbuffer,
        })
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_context(self.display, self.context);
        let _ = self.egl.terminate(self.display);
    }
}
