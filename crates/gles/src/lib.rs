#![deny(unsafe_code)]
//! The GLES 3.0 backend: one device, the atlas resident on it, dmabuf
//! render targets, and the draw of a `render::Queue`.
//!
//! # Model
//!
//! - [`Device::open`] takes the first render node, a GBM device, an EGL
//!   display over it and a GLES 3.0 context, current on the calling
//!   thread for the device's life. No GPU, or no 3.0, is a panic.
//! - The atlas lives in two array textures allocated once from a
//!   [`Budget`] and never rebound: glyph and icon pages as layers of an
//!   `R8` array on unit 0, image pages as layers of an `RGBA8` array on
//!   unit 1, five levels each. A page table maps an `AtlasId` to its
//!   class and layer in a uniform. [`Device::upload`] gives new pages a
//!   layer and sends dirty cells as row runs straight from page memory;
//!   most frames upload nothing.
//! - A [`Target`] is a dmabuf: a GBM buffer laid out by the first of the
//!   compositor's modifiers the GPU accepts, linear otherwise, wrapped as
//!   an EGLImage into a colour renderbuffer with a 16-bit depth
//!   renderbuffer and one FBO. Its planes go to `zwp_linux_dmabuf_v1`.
//! - [`Device::draw`] uploads both passes' commands as the bytes they
//!   are, clears each damage rect, then draws the opaque pass with depth
//!   write and no blending and the translucent pass with depth test and
//!   premultiplied blending. Each kind of command has its own small
//!   program (fill, box, sprite, image) carrying only that kind's work;
//!   the opaque pass is grouped by program, cheapest first, and the
//!   translucent pass keeps its order. Each run of one program is one
//!   instanced draw per damage rect. No texture is bound inside a frame. The vertex shader
//!   clamps each command's corners to its `clip` and shades from where
//!   they land, so a clipped command draws only its kept part, unchanged.
//! - Everything is device pixels, y included: GL row 0 is the buffer's
//!   first row, which is the row the compositor reads as the top, so
//!   nothing is flipped anywhere — not in the vertex shader, not in the
//!   scissor, not in the readback. `z` maps to depth so higher is nearer.
//!
//! # Quick start
//!
//! ```no_run
//! use geometry::{Color, Rect, Size};
//! use gles::prelude::*;
//! use render::{Pass, Queue};
//!
//! let mut device = Device::open(Budget::default());
//! let target = device.target(320, 200, &[]);
//! let queue = Queue {
//!     size: Size::new(320.0, 200.0),
//!     scale: 1.0,
//!     clear: Color::rgb(0.1, 0.2, 0.3),
//!     depth: 2.0,
//!     scissor: vec![Rect::new(0.0, 0.0, 320.0, 200.0)],
//!     opaque: Pass::default(),
//!     translucent: Pass::default(),
//! };
//! device.draw(&target, &queue);
//! let rgba = device.read(&target);
//! assert_eq!(&rgba[..3], &[26, 51, 77]);
//! device.destroy(target);
//! ```

mod copy;
mod draw;
mod egl;
mod program;
mod target;
mod textures;

pub use target::{Plane, Target, XRGB8888};

/// `Error` and `Plane` are left out: `atlas` has its own of each, and a
/// facade folding both preludes together would make neither name usable.
/// Take them by path, as `gles::Error` and `gles::Plane`.
pub mod prelude {
    pub use crate::{Budget, Device, Target, XRGB8888};
}

/// How many atlas pages of each kind the device allocates room for, once,
/// at open. Pages are 1024 square; the mono array costs about 1.3 MB a
/// layer with its mips, the color array about 5.6 MB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Glyph and icon pages together. Default 4.
    pub mono_pages: u32,
    /// Image pages. Default 2.
    pub color_pages: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            mono_pages: 4,
            color_pages: 2,
        }
    }
}

/// Why the device did not open.
#[derive(Debug)]
pub enum Error {
    /// No `/dev/dri/renderD*` could be opened.
    NoDevice,
    /// A GBM, EGL or GL step failed; the string names it.
    Setup(String),
}

/// The GPU: one render node, one GBM device, one EGL display and one
/// GLES 3.0 context, current on the thread that opened it for the
/// device's life. Not `Send`.
///
/// `gpu` is declared last so it drops last: [`Drop for Device`](#impl-Drop-for-Device)
/// frees the atlas textures and the programs' GL objects first, while
/// the context is still current.
pub struct Device {
    budget: Budget,
    programs: program::Programs,
    /// The opaque pass in program order, reused by every draw.
    sorted: Vec<render::Command>,
    textures: textures::Textures,
    gpu: egl::Gpu,
}

impl Device {
    /// Opens the GPU or says why it cannot. Tests call this and skip on
    /// [`Error::NoDevice`].
    pub fn try_open(budget: Budget) -> Result<Device, Error> {
        let gpu = egl::Gpu::open()?;
        let programs = program::Programs::new(&gpu);
        let textures = textures::Textures::new(&gpu, budget);
        Ok(Device {
            gpu,
            budget,
            programs,
            sorted: Vec::new(),
            textures,
        })
    }

    /// [`Device::try_open`] or a panic naming the step that failed.
    pub fn open(budget: Budget) -> Device {
        match Self::try_open(budget) {
            Ok(d) => d,
            Err(Error::NoDevice) => panic!("gles: no render node in /dev/dri; a GPU is required"),
            Err(Error::Setup(step)) => panic!("gles: {step}"),
        }
    }

    /// The `GL_VERSION` string, `OpenGL ES 3.x ...`.
    pub fn version(&self) -> &str {
        &self.gpu.version
    }

    /// The budget this device was opened with.
    pub fn budget(&self) -> Budget {
        self.budget
    }

    /// A render target of `width` by `height` device pixels, XRGB8888,
    /// with the first layout of `modifiers` GBM can produce, or linear
    /// when the list is empty or refused.
    pub fn target(&mut self, width: u32, height: u32, modifiers: &[u64]) -> Target {
        target::create(&self.gpu, width, height, modifiers)
    }

    /// Frees the target's GL objects, EGLImage and buffer.
    pub fn destroy(&mut self, target: Target) {
        target::destroy(&self.gpu, target)
    }

    /// The target's pixels as RGBA8, rows top-down. For tests.
    pub fn read(&mut self, target: &Target) -> Vec<u8> {
        target::read(&self.gpu, target)
    }

    /// The target's buffer's first row, mapped on the CPU after a finish,
    /// as `width * 4` bytes of XRGB8888: little-endian, so `B, G, R, X`
    /// per pixel.
    ///
    /// The only reader that bypasses GL. [`Device::read`] is
    /// `glReadPixels`, which shares its y with the shader and the
    /// scissor, so a flip in all three would be invisible to it; this row
    /// is the one the compositor scans out first. For tests; a buffer the
    /// driver cannot map is a panic.
    pub fn first_row(&mut self, target: &Target) -> Vec<u8> {
        target::first_row(&self.gpu, target)
    }

    /// Uploads what changed in the atlas: new pages get a layer, dirty
    /// cells go up as runs. Takes `&Atlas`, so the drain is not a write.
    pub fn upload(&mut self, atlas: &atlas::Atlas) {
        self.textures.upload(&self.gpu, &self.programs, atlas);
    }

    /// How many sub-image uploads `upload` has issued so far. For tests.
    pub fn upload_calls(&self) -> u64 {
        self.textures.calls
    }

    /// Copies the presented frame in `from` into `to` for a scroll:
    /// outside `shift.area` as it is, inside it moved by `-shift.by`. The
    /// strip that leaves is drawn next with [`render::Scenes::queue_shifted`].
    /// `from` and `to` are different targets of one size.
    pub fn shift(&mut self, from: &Target, to: &Target, shift: render::Shift) {
        debug_assert!(
            (from.width(), from.height()) == (to.width(), to.height()),
            "gles: a shift copies between targets of one size"
        );
        copy::shift(&self.gpu, from, to, shift);
    }

    /// Executes one queue on one target: the clears, the opaque pass,
    /// the translucent pass, a flush. An empty scissor does nothing.
    pub fn draw(&mut self, target: &Target, queue: &render::Queue) {
        if queue.scissor.is_empty() {
            return;
        }
        #[cfg(debug_assertions)]
        for c in q_commands(queue) {
            if c.kind() != render::Command::QUAD {
                debug_assert!(
                    self.textures.has(c.tile.atlas),
                    "gles: atlas page {:?} is sampled before it was uploaded",
                    c.tile.atlas
                );
            }
        }
        draw::clear(&self.gpu, target, queue);
        draw::passes(&self.gpu, &self.programs, target, queue, &mut self.sorted);
        draw::finish(&self.gpu);
    }
}

#[cfg(debug_assertions)]
fn q_commands(q: &render::Queue) -> impl Iterator<Item = &render::Command> {
    q.opaque
        .commands
        .iter()
        .chain(q.translucent.commands.iter())
}

impl Drop for Device {
    fn drop(&mut self) {
        self.textures.drop_with(&self.gpu);
        self.programs.drop_with(&self.gpu);
    }
}
