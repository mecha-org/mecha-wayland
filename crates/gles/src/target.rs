//! One dmabuf a frame is drawn into: a GBM buffer with a negotiated
//! layout, its EGLImage, a colour renderbuffer over it, a depth
//! renderbuffer, one FBO. Made and freed through the device.
#![allow(unsafe_code)]

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use glow::HasContext;
use khronos_egl as egl;

use crate::egl::Gpu;
use crate::gbm::{BufferObject, USE_LINEAR, USE_RENDERING};

/// `DRM_FORMAT_XRGB8888`: an opaque target's format.
pub const XRGB8888: u32 = 0x3432_5258;

/// `DRM_FORMAT_ARGB8888`: the format of a target with alpha, for a window
/// that clears to transparent.
pub const ARGB8888: u32 = 0x3432_5241;

/// `EGL_NATIVE_PIXMAP_KHR`: how a `gbm_bo` is handed to `eglCreateImage`.
const NATIVE_PIXMAP: egl::Enum = 0x30B0;

/// One plane of a target's dmabuf, what `zwp_linux_buffer_params_v1.add`
/// takes.
#[derive(Debug, Clone, Copy)]
pub struct Plane<'a> {
    pub fd: BorrowedFd<'a>,
    pub offset: u32,
    pub stride: u32,
}

struct PlaneData {
    fd: OwnedFd,
    offset: u32,
    stride: u32,
}

/// A render target. There is no `Drop`: free it with [`crate::Device::destroy`].
pub struct Target {
    width: u32,
    height: u32,
    fourcc: u32,
    modifier: u64,
    planes: Vec<PlaneData>,
    pub(crate) fbo: glow::Framebuffer,
    color: glow::Renderbuffer,
    depth: glow::Renderbuffer,
    image: egl::Image,
    /// Must outlive `image`; dropped last by declaration order. Mapped
    /// on the CPU by [`first_row`].
    bo: BufferObject,
}

impl Target {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    /// [`XRGB8888`], or [`ARGB8888`] for a target with alpha.
    pub fn fourcc(&self) -> u32 {
        self.fourcc
    }
    /// The layout GBM chose, `0` for linear — which is what the fallback
    /// buffer reports, since it is created linear.
    pub fn modifier(&self) -> u64 {
        self.modifier
    }
    /// One to four planes, in plane order.
    pub fn planes(&self) -> impl Iterator<Item = Plane<'_>> {
        self.planes.iter().map(|p| Plane {
            fd: p.fd.as_fd(),
            offset: p.offset,
            stride: p.stride,
        })
    }
}

/// `DRM_FORMAT_MOD_LINEAR`.
const LINEAR: u64 = 0;

/// `DRM_FORMAT_MOD_INVALID`, what `gbm_bo_get_modifier` reports for a
/// buffer the legacy `gbm_bo_create` made: that entry point takes flags,
/// not modifiers, so Mesa records none.
const INVALID: u64 = 0x00ff_ffff_ffff_ffff;

/// The buffer and the modifier to report for it.
///
/// The modifier list is the compositor's, in the order it advertised
/// them, and GBM picks the first it can produce. It may refuse the lot:
/// the compositor advertises what it can *import and scan out*, which on
/// a split-GPU or multi-driver machine is not what this render node can
/// *render into*. Falling back is safe rather than fatal because linear
/// is the one layout every driver renders and every compositor imports —
/// slower on a tiler (see the crate docs), but correct everywhere. The
/// reason GBM gave is dropped: there is nothing to do with it but take
/// the fallback, and the target reports the layout it ended up with, so
/// a caller that cares can compare.
fn buffer(
    gpu: &Gpu,
    width: u32,
    height: u32,
    fourcc: u32,
    modifiers: &[u64],
) -> (BufferObject, u64) {
    let linear = || {
        let bo = gpu
            .gbm
            .create_buffer(width, height, fourcc, USE_RENDERING | USE_LINEAR)
            .unwrap_or_else(|e| panic!("gles: gbm_bo_create({width}x{height}, linear): {e}"));
        // `GBM_BO_USE_LINEAR` is a linear layout, but this entry point
        // records no modifier and the buffer reports `INVALID`, which is
        // not a value `zwp_linux_buffer_params_v1.add` accepts. Name the
        // layout we asked for.
        (bo, LINEAR)
    };
    if modifiers.is_empty() {
        return linear();
    }
    match gpu
        .gbm
        .create_buffer_with_modifiers(width, height, fourcc, modifiers, USE_RENDERING)
    {
        Ok(bo) => {
            let m = bo.modifier();
            let m = if m == INVALID { LINEAR } else { m };
            (bo, m)
        }
        Err(_) => linear(),
    }
}

pub(crate) fn create(gpu: &Gpu, width: u32, height: u32, fourcc: u32, modifiers: &[u64]) -> Target {
    assert!(
        matches!(fourcc, XRGB8888 | ARGB8888),
        "gles: a target is XRGB8888 or ARGB8888"
    );
    let (bo, modifier) = buffer(gpu, width, height, fourcc, modifiers);
    let planes = (0..bo.plane_count())
        .map(|i| PlaneData {
            fd: bo
                .fd_for_plane(i)
                .unwrap_or_else(|e| panic!("gles: gbm_bo_get_fd_for_plane({i}): {e:?}")),
            offset: bo.offset(i),
            stride: bo.stride_for_plane(i),
        })
        .collect();
    // SAFETY: NO_CONTEXT and a live gbm_bo pointer, as EGL_KHR_image_pixmap
    // over the GBM platform expects.
    let image = unsafe {
        gpu.egl.create_image(
            gpu.display,
            egl::Context::from_ptr(egl::NO_CONTEXT),
            NATIVE_PIXMAP,
            egl::ClientBuffer::from_ptr(bo.as_ptr()),
            &[egl::ATTRIB_NONE],
        )
    }
    .unwrap_or_else(|e| panic!("gles: eglCreateImage(gbm_bo): {e:?}"));
    let gl = &gpu.gl;
    // SAFETY: the context is current; the image is valid until destroyed.
    let (fbo, color, depth) = unsafe {
        let color = gl.create_renderbuffer().expect("glGenRenderbuffers");
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(color));
        (gpu.image_target_renderbuffer)(glow::RENDERBUFFER, image.as_ptr());
        let depth = gl.create_renderbuffer().expect("glGenRenderbuffers");
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
        gl.renderbuffer_storage(
            glow::RENDERBUFFER,
            glow::DEPTH_COMPONENT16,
            width as i32,
            height as i32,
        );
        let fbo = gl.create_framebuffer().expect("glGenFramebuffers");
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::RENDERBUFFER,
            Some(color),
        );
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::DEPTH_ATTACHMENT,
            glow::RENDERBUFFER,
            Some(depth),
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        assert!(
            status == glow::FRAMEBUFFER_COMPLETE,
            "gles: the target's framebuffer is incomplete: {status:#x}"
        );
        (fbo, color, depth)
    };
    Target {
        width,
        height,
        fourcc,
        modifier,
        planes,
        fbo,
        color,
        depth,
        image,
        bo,
    }
}

pub(crate) fn destroy(gpu: &Gpu, t: Target) {
    let gl = &gpu.gl;
    // SAFETY: objects this module made on the current context.
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.delete_framebuffer(t.fbo);
        gl.delete_renderbuffer(t.color);
        gl.delete_renderbuffer(t.depth);
    }
    let _ = gpu.egl.destroy_image(gpu.display, t.image);
    // `t.planes` and `t.bo` drop here, fds and buffer last.
}

/// Top-down RGBA8 rows of the whole target: GL row 0 is the buffer's
/// first row, which is the top one, so the rows come back as they lie.
pub(crate) fn read(gpu: &Gpu, t: &Target) -> Vec<u8> {
    let (w, h) = (t.width as usize, t.height as usize);
    let mut out = vec![0u8; w * h * 4];
    let gl = &gpu.gl;
    // SAFETY: the FBO is this target's; the slice is sized for it.
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(t.fbo));
        gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
        gl.read_pixels(
            0,
            0,
            t.width as i32,
            t.height as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut out)),
        );
    }
    out
}

/// The dmabuf's own first row, read from its memory with the GPU
/// flushed: `width * 4` bytes of XRGB8888, which is little-endian, so
/// `B, G, R, X` per pixel.
///
/// This is the one reader that does not go through GL. `read` uses
/// `glReadPixels`, which shares its y with the shader and the scissor,
/// so a flip in all three would be invisible to it; the row here is the
/// row the compositor scans out first. For tests.
pub(crate) fn first_row(gpu: &Gpu, t: &Target) -> Vec<u8> {
    // SAFETY: the context is current; the draw must land in the buffer
    // before the CPU maps it.
    unsafe { gpu.gl.finish() };
    let row = t.width as usize * 4;
    t.bo.read_map(0, 0, t.width, 1, |bytes| bytes[..row].to_vec())
        .unwrap_or_else(|e| panic!("gles: gbm_bo_map({}x1): {e}", t.width))
}
