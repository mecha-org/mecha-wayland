//! The one program, its attribute table over `render::Command`, the
//! instance buffer and the VAO. Nothing here is rebound after `new`.
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

out vec2 v_local;
out vec2 v_uv;
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
    vec2 f = (p - a_rect.xy) / a_rect.zw;
    gl_Position = vec4(
        p.x / u_size.x * 2.0 - 1.0,
        p.y / u_size.y * 2.0 - 1.0,
        1.0 - 2.0 * (a_z + 0.5) / u_depth,
        1.0);
    v_half = a_rect.zw * 0.5;
    v_local = p - a_rect.xy - v_half;
    v_uv = (a_tile.xy + f * a_tile.zw) * u_inv_page;
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

const FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;

in vec2 v_local;
in vec2 v_uv;
flat in vec2 v_half;
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

void main() {
    float r = corner_radius(v_radii, v_local);
    float d = sd_box(v_local, v_half, r);
    float coverage = 1.0 - smoothstep(-0.5, 0.5, d);

    // border: top, right, bottom, left. The inner box is the rect inset
    // per side; its centre shifts by half the difference of opposite
    // sides; its corner radius is r less the larger adjacent side.
    vec2 inner_half = v_half - vec2(v_border.y + v_border.w, v_border.x + v_border.z) * 0.5;
    vec2 inner_center = vec2(v_border.w - v_border.y, v_border.x - v_border.z) * 0.5;
    vec2 pi = v_local - inner_center;
    vec2 sides = vec2(pi.x < 0.0 ? v_border.w : v_border.y,
                      pi.y < 0.0 ? v_border.x : v_border.z);
    float ri = max(r - max(sides.x, sides.y), 0.0);
    float di = sd_box(pi, inner_half, ri);
    float in_border = smoothstep(-0.5, 0.5, di);
    vec4 color = mix(v_color, v_border_color, in_border);

    uint kind = v_flags & 3u;
    if (kind == 1u) {
        vec3 uvw = vec3(v_uv, float(v_layer));
        float m = v_class == 0 ? textureLod(u_mono, uvw, 0.0).r : texture(u_mono, uvw).r;
        color.a *= m;
    } else if (kind == 2u) {
        vec4 t = texture(u_color, vec3(v_uv, float(v_layer)));
        // pages are premultiplied: unpremultiply after filtering
        vec3 rgb = t.a > 0.0 ? t.rgb / t.a : vec3(0.0);
        if ((v_flags & 4u) != 0u) {
            rgb = vec3(dot(rgb, vec3(0.2126, 0.7152, 0.0722)));
        }
        float opacity = float((v_flags >> 8) & 255u) / 255.0;
        color = vec4(rgb * v_color.rgb, t.a * v_color.a * opacity);
    }

    float a = color.a * coverage;
    if (u_opaque) {
        frag = vec4(mix(v_background.rgb, color.rgb, a), 1.0);
    } else {
        frag = vec4(color.rgb * a, a);
    }
}
"#;

pub(crate) struct Program {
    pub(crate) program: glow::Program,
    pub(crate) vao: glow::VertexArray,
    pub(crate) vbo: glow::Buffer,
    pub(crate) u_size: Option<glow::UniformLocation>,
    pub(crate) u_depth: Option<glow::UniformLocation>,
    pub(crate) u_opaque: Option<glow::UniformLocation>,
    /// Written by `textures::Textures::upload`, once the atlas has pages
    /// to describe; zeroed here so an id with no entry maps to class 0,
    /// layer 0.
    pub(crate) u_page: Option<glow::UniformLocation>,
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

impl Program {
    /// Bytes between two instances: `size_of::<render::Command>()`.
    /// The attribute offsets below are written against this number, so
    /// the `const _` under this `impl` refuses to compile if `Command`
    /// ever grows or shrinks.
    pub(crate) const STRIDE: i32 = 140;

    pub(crate) fn new(gpu: &Gpu) -> Program {
        let gl = &gpu.gl;
        // SAFETY: the context is current; every handle is one made here.
        unsafe {
            let vs = compile(gl, glow::VERTEX_SHADER, VERTEX, "vertex");
            let fs = compile(gl, glow::FRAGMENT_SHADER, FRAGMENT, "fragment");
            let program = gl.create_program().expect("glCreateProgram");
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            assert!(
                gl.get_program_link_status(program),
                "gles: the program does not link:\n{}",
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

            let vao = gl.create_vertex_array().expect("glGenVertexArrays");
            gl.bind_vertex_array(Some(vao));
            let vbo = gl.create_buffer().expect("glGenBuffers");
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            for i in 0..11 {
                gl.enable_vertex_attrib_array(i);
                gl.vertex_attrib_divisor(i, 1);
            }
            let p = Program {
                program,
                vao,
                vbo,
                u_size: loc("u_size"),
                u_depth: loc("u_depth"),
                u_opaque: loc("u_opaque"),
                u_page,
            };
            p.point(gl, 0);
            p
        }
    }

    /// Points the eleven attributes at the instance that starts `base` bytes
    /// into the VBO. GLES 3.0 has no base instance, so each pass calls
    /// this once with its offset.
    pub(crate) fn point(&self, gl: &glow::Context, base: i32) {
        const S: i32 = Program::STRIDE;
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

    /// Deletes the program, its VAO and its VBO.
    pub(crate) fn drop_with(&self, gpu: &Gpu) {
        // SAFETY: objects this module made.
        unsafe {
            gpu.gl.delete_program(self.program);
            gpu.gl.delete_vertex_array(self.vao);
            gpu.gl.delete_buffer(self.vbo);
        }
    }
}

/// `draw::bytes` reinterprets a `&[render::Command]` as bytes and the VAO
/// strides through it by [`Program::STRIDE`]; the two must be the same
/// number, and this fails the build the day they are not.
const _: () = assert!(Program::STRIDE as usize == std::mem::size_of::<render::Command>());
