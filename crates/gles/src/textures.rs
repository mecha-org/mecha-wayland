//! The atlas on the GPU: two array textures allocated once at open and
//! never rebound, the page table that maps an `AtlasId` to its array and
//! layer, and the upload of dirty cells in row runs.
#![allow(unsafe_code)]

use atlas::{Atlas, AtlasId, CELL, Class, PAGE};
use glow::HasContext;

use crate::Budget;
use crate::egl::Gpu;
use crate::program::Programs;

/// Levels every layer has; glyph layers fill only the first.
const LEVELS: i32 = 5;
/// Ids the uniform table holds: `ivec4 u_page[64]`.
const TABLE: usize = 256;

#[derive(Clone, Copy)]
enum Array {
    Mono,
    Color,
}

pub(crate) struct Textures {
    mono: glow::Texture,
    color: glow::Texture,
    budget: Budget,
    /// The `Atlas::generation` the layers below were assigned under.
    generation: u64,
    mono_used: u32,
    color_used: u32,
    /// By `AtlasId`: the class and layer of a page that has a layer.
    pages: Vec<Option<(Class, u32)>>,
    /// `class << 8 | layer` per id, mirrored into `u_page`.
    table: [i32; TABLE],
    /// Sub-image uploads issued so far. For tests.
    pub(crate) calls: u64,
}

/// A run of contiguous dirty cells in one row of one page.
///
/// `drain_dirty` hands the closure `&Page` under a higher-ranked bound
/// (`for<'r> FnMut(&'r Page, Cell)`, since the signature elides the
/// lifetime): the reference is only valid for that one call, so it
/// cannot sit in a variable that survives to the next call. `page` is
/// kept by `AtlasId` instead, a plain value with no lifetime; `flush`
/// looks the page back up through `atlas.page()`, a second shared
/// borrow alongside `drain_dirty`'s own.
struct Run {
    page: AtlasId,
    layer: u32,
    row: u8,
    col: u8,
    count: u8,
}

fn array_of(class: Class) -> Array {
    match class {
        Class::Glyph | Class::Icon => Array::Mono,
        Class::Image => Array::Color,
        Class::External => unreachable!("externals have no page"),
    }
}

impl Textures {
    pub(crate) fn new(gpu: &Gpu, budget: Budget) -> Textures {
        let gl = &gpu.gl;
        let make = |unit: u32, internal: u32, layers: u32| {
            // SAFETY: the context is current.
            unsafe {
                let t = gl.create_texture().expect("glGenTextures");
                gl.active_texture(glow::TEXTURE0 + unit);
                gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(t));
                gl.tex_storage_3d(
                    glow::TEXTURE_2D_ARRAY,
                    LEVELS,
                    internal,
                    PAGE as i32,
                    PAGE as i32,
                    layers.max(1) as i32,
                );
                let p = |k, v| gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, k, v);
                p(glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR as i32);
                p(glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                p(glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                p(glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
                p(glow::TEXTURE_MAX_LEVEL, LEVELS - 1);
                t
            }
        };
        let mono = make(0, glow::R8, budget.mono_pages);
        let color = make(1, glow::RGBA8, budget.color_pages);
        Textures {
            mono,
            color,
            budget,
            generation: 0,
            mono_used: 0,
            color_used: 0,
            pages: Vec::new(),
            table: [0; TABLE],
            calls: 0,
        }
    }

    pub(crate) fn has(&self, id: AtlasId) -> bool {
        self.pages.get(id.0 as usize).is_some_and(|p| p.is_some())
    }

    /// New pages get a layer; then every dirty cell goes up in runs. A new
    /// `Atlas::generation` scrapped every page and reused ids, so layers
    /// are assigned afresh: the rebuilt pages are all dirty and go up whole.
    pub(crate) fn upload(&mut self, gpu: &Gpu, programs: &Programs, atlas: &Atlas) {
        if atlas.externals().next().is_some() {
            todo!("gles: Class::External (dmabuf import) is not implemented yet");
        }
        let mut added = false;
        if atlas.generation() != self.generation {
            self.generation = atlas.generation();
            self.pages.clear();
            self.table = [0; TABLE];
            self.mono_used = 0;
            self.color_used = 0;
            added = true;
        }
        let count = |class| atlas.pages().filter(|p| p.class() == class).count();
        for page in atlas.pages() {
            if self.has(page.id()) {
                continue;
            }
            let id = page.id().0 as usize;
            assert!(
                id < TABLE,
                "gles: atlas id {id} is past the page table of {TABLE}"
            );
            let layer = match array_of(page.class()) {
                Array::Mono => {
                    assert!(
                        self.mono_used < self.budget.mono_pages,
                        "gles: the mono atlas budget of {} pages is spent by a {:?} page ({} glyph, {} icon, {} image pages live); raise Budget::mono_pages",
                        self.budget.mono_pages,
                        page.class(),
                        count(Class::Glyph),
                        count(Class::Icon),
                        count(Class::Image)
                    );
                    self.mono_used += 1;
                    self.mono_used - 1
                }
                Array::Color => {
                    assert!(
                        self.color_used < self.budget.color_pages,
                        "gles: the color atlas budget of {} pages is spent by a {:?} page ({} glyph, {} icon, {} image pages live); raise Budget::color_pages",
                        self.budget.color_pages,
                        page.class(),
                        count(Class::Glyph),
                        count(Class::Icon),
                        count(Class::Image)
                    );
                    self.color_used += 1;
                    self.color_used - 1
                }
            };
            if self.pages.len() <= id {
                self.pages.resize(id + 1, None);
            }
            self.pages[id] = Some((page.class(), layer));
            self.table[id] = ((page.class() as i32) << 8) | layer as i32;
            added = true;
        }
        if added {
            // SAFETY: the programs are the device's.
            unsafe {
                for v in programs.all() {
                    gpu.gl.use_program(Some(v.program));
                    gpu.gl.uniform_4_i32_slice(v.u_page.as_ref(), &self.table);
                }
            }
        }

        let mut run: Option<Run> = None;
        atlas.drain_dirty(|page, cell| {
            let layer = self.pages[page.id().0 as usize]
                .expect("every page has a layer")
                .1;
            match &mut run {
                Some(r)
                    if r.page == page.id() && r.row == cell.row && r.col + r.count == cell.col =>
                {
                    r.count += 1;
                }
                _ => {
                    if let Some(r) = run.take() {
                        self.flush(gpu, atlas, r);
                    }
                    run = Some(Run {
                        page: page.id(),
                        layer,
                        row: cell.row,
                        col: cell.col,
                        count: 1,
                    });
                }
            }
        });
        if let Some(r) = run.take() {
            self.flush(gpu, atlas, r);
        }
    }

    /// One `glTexSubImage3D` per level of the run, straight from the
    /// page's memory through unpack row length and skip.
    fn flush(&mut self, gpu: &Gpu, atlas: &Atlas, r: Run) {
        let gl = &gpu.gl;
        let page = atlas.page(r.page).expect("a run names a live page");
        let (unit, format, levels) = match array_of(page.class()) {
            Array::Mono => (
                0,
                glow::RED,
                if page.class() == Class::Glyph {
                    1
                } else {
                    LEVELS
                },
            ),
            Array::Color => (1, glow::RGBA, LEVELS),
        };
        // SAFETY: the array on `unit` is bound for the device's life; the
        // slice is the page's whole level and the skip stays inside it.
        unsafe {
            gl.active_texture(glow::TEXTURE0 + unit);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            for k in 0..levels {
                let side = (CELL >> k) as i32;
                let (x, y) = (r.col as i32 * side, r.row as i32 * side);
                gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, (PAGE >> k) as i32);
                gl.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, x);
                gl.pixel_store_i32(glow::UNPACK_SKIP_ROWS, y);
                gl.tex_sub_image_3d(
                    glow::TEXTURE_2D_ARRAY,
                    k,
                    x,
                    y,
                    r.layer as i32,
                    side * r.count as i32,
                    side,
                    1,
                    format,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(page.level(k as u32))),
                );
                self.calls += 1;
            }
            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            gl.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
            gl.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 0);
        }
    }

    pub(crate) fn drop_with(&self, gpu: &Gpu) {
        // SAFETY: textures this module made.
        unsafe {
            gpu.gl.delete_texture(self.mono);
            gpu.gl.delete_texture(self.color);
        }
    }
}
