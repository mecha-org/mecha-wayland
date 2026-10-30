//! The programs, one per kind of command, their shared attribute table
//! over `render::Command`, the instance buffer and the VAO.
#![allow(unsafe_code)]

use glow::HasContext;

use crate::egl::Gpu;

const VERTEX: &str = r#"#version 300 es
precision highp float;
precision highp int;

layout(location = 0) in vec4 a_rect;
layout(location = 1) in float a_z;
layout(location = 2) in vec4 a_color;
layout(location = 3) in vec4 a_border_color;
layout(location = 4) in vec4 a_background;
layout(location = 5) in vec4 a_radii;
layout(location = 6) in vec4 a_border;
layout(location = 7) in uint a_atlas;
layout(location = 8) in vec4 a_tile;
layout(location = 9) in uint a_flags;
layout(location = 10) in vec4 a_clip;

uniform vec2 u_size;
uniform float u_depth;
uniform float u_inv_page;
uniform ivec4 u_page[64];

flat out vec4 v_rect;
flat out vec4 v_tile;
flat out vec2 v_half;
flat out vec4 v_color;
flat out vec4 v_border_color;
flat out vec4 v_background;
flat out vec4 v_radii;
flat out vec4 v_border;
flat out uint v_flags;
flat out int v_class;
flat out int v_layer;

void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
    // The corner is pulled inside the clip and everything per pixel is
    // derived from where it lands, so a clipped quad rasterises only its
    // kept part and each kept pixel shades as it would unclipped.
    vec2 p = clamp(a_rect.xy + corner * a_rect.zw, a_clip.xy, a_clip.xy + a_clip.zw);
    gl_Position = vec4(
        p.x / u_size.x * 2.0 - 1.0,
        p.y / u_size.y * 2.0 - 1.0,
        1.0 - 2.0 * (a_z + 0.5) / u_depth,
        1.0);
    v_half = a_rect.zw * 0.5;
    v_rect = a_rect;
    v_tile = a_tile * u_inv_page;
    v_color = a_color;
    v_border_color = a_border_color;
    v_background = a_background;
    v_radii = a_radii;
    v_border = a_border;
    v_flags = a_flags;
    int id = int(a_atlas);
    int e = id < 256 ? u_page[id >> 2][id & 3] : 0;
    v_class = e >> 8;
    v_layer = e & 255;
}
"#;

/// One source, four programs: each is compiled with `SHADER` defined to
/// one [`Shader`] and keeps only that kind's work, so a pixel of a plain
/// fill never pays for the rounded-box distance or a texture fetch.
const FRAGMENT: &str = r#"precision highp float;
precision highp int;

flat in vec4 v_rect;
flat in vec4 v_tile;
flat in vec2 v_half;

// Where this pixel sits in the command's rect, from its own window
// coordinate rather than interpolated across the clipped quad: a rect
// moved by whole pixels, or cut by another clip, shades the same.
// The subtraction is in integers, which the compiler cannot reassociate
// with what follows: rects lie on whole pixels.
#define v_rel (vec2(ivec2(gl_FragCoord.xy) - ivec2(v_rect.xy)) + 0.5)
#define v_local (v_rel - v_half)
#define v_uv (v_tile.xy + v_rel / v_rect.zw * v_tile.zw)
flat in vec4 v_color;
flat in vec4 v_border_color;
flat in vec4 v_background;
flat in vec4 v_radii;
flat in vec4 v_border;
flat in uint v_flags;
flat in int v_class;
flat in int v_layer;

uniform bool u_opaque;
uniform highp sampler2DArray u_mono;
uniform highp sampler2DArray u_color;

out vec4 frag;

float sd_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

// radii: top_left, top_right, bottom_right, bottom_left; y grows down.
float corner_radius(vec4 radii, vec2 p) {
    return p.x < 0.0 ? (p.y < 0.0 ? radii.x : radii.w)
                     : (p.y < 0.0 ? radii.y : radii.z);
}

// The coverage of the rect's own square edge: 1 inside, a ramp across
// a fractional edge.
float square_coverage() {
    return 1.0 - smoothstep(-0.5, 0.5, sd_box(v_local, v_half, 0.0));
}

void write(vec4 color, float coverage) {
    float a = color.a * coverage;
    if (u_opaque) {
        // Source-over onto the background, premultiplied: an opaque
        // background gives its own mix and alpha one, a transparent one
        // gives the colour at alpha `a`.
        vec4 bg = vec4(v_background.rgb * v_background.a, v_background.a);
        frag = vec4(mix(bg.rgb, color.rgb, a), mix(bg.a, 1.0, a));
    } else {
        frag = vec4(color.rgb * a, a);
    }
}

#if SHADER == 0
// Fill: no corner, no border, no tile.
void main() {
    write(v_color, square_coverage());
}
#elif SHADER == 3
// Box: rounded corners, a border, or both.
void main() {
    float r = corner_radius(v_radii, v_local);
    float coverage = 1.0 - smoothstep(-0.5, 0.5, sd_box(v_local, v_half, r));

    // border: top, right, bottom, left. The inner box is the rect inset
    // per side; its centre shifts by half the difference of opposite
    // sides; its corner radius is r less the larger adjacent side.
    vec2 inner_half = v_half - vec2(v_border.y + v_border.w, v_border.x + v_border.z) * 0.5;
    vec2 inner_center = vec2(v_border.w - v_border.y, v_border.x - v_border.z) * 0.5;
    vec2 pi = v_local - inner_center;
    vec2 sides = vec2(pi.x < 0.0 ? v_border.w : v_border.y,
                      pi.y < 0.0 ? v_border.x : v_border.z);
    float ri = max(r - max(sides.x, sides.y), 0.0);
    float in_border = smoothstep(-0.5, 0.5, sd_box(pi, inner_half, ri));
    write(mix(v_color, v_border_color, in_border), coverage);
}
#elif SHADER == 1
// Sprite: a coverage tile, tinted.
void main() {
    vec3 uvw = vec3(v_uv, float(v_layer));
    float m = v_class == 0 ? textureLod(u_mono, uvw, 0.0).r : texture(u_mono, uvw).r;
    write(vec4(v_color.rgb, v_color.a * m), square_coverage());
}
#else
// Image: a colour tile with rounded corners.
void main() {
    float r = corner_radius(v_radii, v_local);
    float coverage = 1.0 - smoothstep(-0.5, 0.5, sd_box(v_local, v_half, r));
    vec4 t = texture(u_color, vec3(v_uv, float(v_layer)));
    // pages are premultiplied: unpremultiply after filtering
    vec3 rgb = t.a > 0.0 ? t.rgb / t.a : vec3(0.0);
    if ((v_flags & 4u) != 0u) {
        rgb = vec3(dot(rgb, vec3(0.2126, 0.7152, 0.0722)));
    }
    float opacity = float((v_flags >> 8) & 255u) / 255.0;
    write(vec4(rgb * v_color.rgb, t.a * v_color.a * opacity), coverage);
}
#endif
"#;

/// Which program draws a command; the discriminant is its `SHADER`. The
/// order is the opaque pass's: cheap programs first, so their depth is
/// down before the box program runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Shader {
    Fill = 0,
    Sprite = 1,
    Image = 2,
    Box = 3,
}

impl Shader {
    pub(crate) const ALL: [Shader; 4] = [Shader::Fill, Shader::Sprite, Shader::Image, Shader::Box];

    pub(crate) fn of(c: &render::Command) -> Shader {
        match c.kind() {
            render::Command::SPRITE => Shader::Sprite,
            render::Command::IMAGE => Shader::Image,
            _ => {
                let r = c.radii;
                let b = c.border;
                let square = r.top_left == 0.0
                    && r.top_right == 0.0
                    && r.bottom_right == 0.0
                    && r.bottom_left == 0.0;
                let borderless = b.top == 0.0 && b.right == 0.0 && b.bottom == 0.0 && b.left == 0.0;
                if square && borderless {
                    Shader::Fill
                } else {
                    Shader::Box
                }
            }
        }
    }
}

/// One linked program and the uniforms the draw sets on it.
pub(crate) struct Variant {
    pub(crate) program: glow::Program,
    pub(crate) u_size: Option<glow::UniformLocation>,
    pub(crate) u_depth: Option<glow::UniformLocation>,
    pub(crate) u_opaque: Option<glow::UniformLocation>,
    /// Written by `textures::Textures::upload`, once the atlas has pages
    /// to describe; zeroed here so an id with no entry maps to class 0,
    /// layer 0.
    pub(crate) u_page: Option<glow::UniformLocation>,
}

/// The four programs, one per [`Shader`], over one VAO and one VBO.
/// Nothing here is rebound after `new`.
pub(crate) struct Programs {
    pub(crate) vao: glow::VertexArray,
    pub(crate) vbo: glow::Buffer,
    by: [Variant; 4],
}

fn compile(gl: &glow::Context, kind: u32, src: &str, name: &str) -> glow::Shader {
    // SAFETY: the context is current.
    unsafe {
        let s = gl.create_shader(kind).expect("glCreateShader");
        gl.shader_source(s, src);
        gl.compile_shader(s);
        assert!(
            gl.get_shader_compile_status(s),
            "gles: the {name} shader does not compile:\n{}",
            gl.get_shader_info_log(s)
        );
        s
    }
}

fn link(gl: &glow::Context, shader: Shader) -> Variant {
    let n = shader as u8;
    let fragment = format!("#version 300 es\n#define SHADER {n}\n{FRAGMENT}");
    // SAFETY: the context is current; every handle is one made here.
    unsafe {
        let vs = compile(gl, glow::VERTEX_SHADER, VERTEX, "vertex");
        let fs = compile(
            gl,
            glow::FRAGMENT_SHADER,
            &fragment,
            &format!("{shader:?} fragment"),
        );
        let program = gl.create_program().expect("glCreateProgram");
        gl.attach_shader(program, vs);
        gl.attach_shader(program, fs);
        gl.link_program(program);
        assert!(
            gl.get_program_link_status(program),
            "gles: the {shader:?} program does not link:\n{}",
            gl.get_program_info_log(program)
        );
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        gl.use_program(Some(program));
        let loc = |n: &str| gl.get_uniform_location(program, n);
        gl.uniform_1_i32(loc("u_mono").as_ref(), 0);
        gl.uniform_1_i32(loc("u_color").as_ref(), 1);
        gl.uniform_1_f32(loc("u_inv_page").as_ref(), 1.0 / atlas::PAGE as f32);
        let u_page = loc("u_page");
        gl.uniform_4_i32_slice(u_page.as_ref(), &[0; 256]);
        Variant {
            program,
            u_size: loc("u_size"),
            u_depth: loc("u_depth"),
            u_opaque: loc("u_opaque"),
            u_page,
        }
    }
}

impl Programs {
    /// Bytes between two instances: `size_of::<render::Command>()`.
    /// The attribute offsets below are written against this number, so
    /// the `const _` under this `impl` refuses to compile if `Command`
    /// ever grows or shrinks.
    pub(crate) const STRIDE: i32 = 140;

    pub(crate) fn new(gpu: &Gpu) -> Programs {
        let gl = &gpu.gl;
        let by = Shader::ALL.map(|s| link(gl, s));
        // SAFETY: the context is current; every handle is one made here.
        unsafe {
            let vao = gl.create_vertex_array().expect("glGenVertexArrays");
            gl.bind_vertex_array(Some(vao));
            let vbo = gl.create_buffer().expect("glGenBuffers");
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            for i in 0..11 {
                gl.enable_vertex_attrib_array(i);
                gl.vertex_attrib_divisor(i, 1);
            }
            let p = Programs { vao, vbo, by };
            p.point(gl, 0);
            p
        }
    }

    /// The program that draws `shader`'s commands.
    pub(crate) fn get(&self, shader: Shader) -> &Variant {
        &self.by[shader as usize]
    }

    /// Every program, for uniforms they all share.
    pub(crate) fn all(&self) -> &[Variant; 4] {
        &self.by
    }

    /// Points the eleven attributes at the instance that starts `base` bytes
    /// into the VBO. GLES 3.0 has no base instance, so each run of one
    /// program calls this once with its offset.
    pub(crate) fn point(&self, gl: &glow::Context, base: i32) {
        const S: i32 = Programs::STRIDE;
        // SAFETY: the VAO and VBO are bound; offsets are inside a command.
        unsafe {
            for (loc, off) in [
                (0, 0),
                (2, 20),
                (3, 36),
                (4, 52),
                (5, 68),
                (6, 84),
                (8, 104),
                (10, 124),
            ] {
                gl.vertex_attrib_pointer_f32(loc, 4, glow::FLOAT, false, S, base + off);
            }
            gl.vertex_attrib_pointer_f32(1, 1, glow::FLOAT, false, S, base + 16);
            gl.vertex_attrib_pointer_i32(7, 1, glow::UNSIGNED_INT, S, base + 100);
            gl.vertex_attrib_pointer_i32(9, 1, glow::UNSIGNED_INT, S, base + 120);
        }
    }

    /// Deletes the programs, the VAO and the VBO.
    pub(crate) fn drop_with(&self, gpu: &Gpu) {
        // SAFETY: objects this module made.
        unsafe {
            for v in &self.by {
                gpu.gl.delete_program(v.program);
            }
            gpu.gl.delete_vertex_array(self.vao);
            gpu.gl.delete_buffer(self.vbo);
        }
    }
}

/// `draw::bytes` reinterprets a `&[render::Command]` as bytes and the VAO
/// strides through it by [`Programs::STRIDE`]; the two must be the same
/// number, and this fails the build the day they are not.
const _: () = assert!(Programs::STRIDE as usize == std::mem::size_of::<render::Command>());
