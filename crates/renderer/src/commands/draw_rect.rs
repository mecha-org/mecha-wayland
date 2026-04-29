use glow::{HasContext, NativeBuffer, NativeProgram, NativeUniformLocation};

use crate::commands::{Command, CommandQueue, RenderContext};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrawRect {
    pub color: (f32, f32, f32, f32), // r, g, b, a in f32 from 0.0 to 1.0
    pub origin: (f32, f32, f32),     // x, y in pixels, z for depth
    pub size: (f32, f32),            // width, height in pixels
}
unsafe impl bytemuck::Pod for DrawRect {}
unsafe impl bytemuck::Zeroable for DrawRect {
    fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

impl Command for DrawRect {
    fn get_queue_from_registry(
        registry: &mut super::CommandQueueRegistry,
    ) -> &mut impl super::CommandQueue<Self> {
        &mut registry.draw_rect_queue
    }
}

#[derive(Default)]
pub(crate) struct RectQueue {
    shader_program: Option<NativeProgram>,
    vbo: Option<NativeBuffer>,

    u_viewport_inv_res_loc: Option<NativeUniformLocation>,

    opaque: Vec<DrawRect>,
    translucent: Vec<DrawRect>,
}

// Builds an interleaved vertex buffer (11 floats/vertex, 6 vertices/rect) for a batch of rects.
// Layout per vertex: aPos(2) aColor(4) aOrigin(3) aSize(2)
fn build_rect_verts(rects: &[DrawRect]) -> Vec<f32> {
    #[rustfmt::skip]
    const CORNERS: [(f32, f32); 6] = [
        (-0.5,  0.5), ( 0.5,  0.5), (-0.5, -0.5),  // triangle 1
        ( 0.5,  0.5), ( 0.5, -0.5), (-0.5, -0.5),  // triangle 2
    ];
    let mut v = Vec::with_capacity(rects.len() * 6 * 11);
    for r in rects {
        let (cr, cg, cb, ca) = r.color;
        let (ox, oy, oz) = r.origin;
        let (sw, sh) = r.size;
        for (px, py) in CORNERS {
            v.extend_from_slice(&[px, py, cr, cg, cb, ca, ox, oy, oz, sw, sh]);
        }
    }
    v
}

impl CommandQueue<DrawRect> for RectQueue {
    fn init(&mut self, ctx: &RenderContext) {
        unsafe {
            let gl = ctx.gl;
            let program = gl.create_program().expect("glCreateProgram");

            // GLSL ES 1.00 — device is GLES 2.0 (Vivante GC7000).
            // Per-rect data is packed as vertex attributes; all rects draw in one call.
            let vs_src = r#"#version 100
                attribute vec2 aPos;
                attribute vec4 aColor;
                attribute vec3 aOrigin;
                attribute vec2 aSize;
                varying vec4 vColor;

                uniform vec2 uViewportInvRes;

                void main() {
                  vec2 center = aOrigin.xy + aSize * 0.5;
                  vec2 pixelPos = vec2(aPos.x, -aPos.y) * aSize + center;
                  vec2 ndc = pixelPos * uViewportInvRes - 1.0;
                  ndc.y = -ndc.y;
                  gl_Position = vec4(ndc, aOrigin.z, 1.0);
                  vColor = aColor;
                }
            "#;

            let vs = gl
                .create_shader(glow::VERTEX_SHADER)
                .expect("glCreateShader(VERTEX_SHADER)");
            gl.shader_source(vs, vs_src);
            gl.compile_shader(vs);
            if !gl.get_shader_compile_status(vs) {
                panic!(
                    "vertex shader compile error: {}",
                    gl.get_shader_info_log(vs)
                );
            }

            let fs_src = r#"#version 100
                precision mediump float;
                varying vec4 vColor;
                void main() {
                    gl_FragColor = vColor;
                }
            "#;
            let fs = gl
                .create_shader(glow::FRAGMENT_SHADER)
                .expect("glCreateShader(FRAGMENT_SHADER)");
            gl.shader_source(fs, fs_src);
            gl.compile_shader(fs);
            if !gl.get_shader_compile_status(fs) {
                panic!(
                    "fragment shader compile error: {}",
                    gl.get_shader_info_log(fs)
                );
            }

            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            // Bind attribute locations before linking (no layout qualifiers in GLSL ES 1.00).
            gl.bind_attrib_location(program, 0, "aPos");
            gl.bind_attrib_location(program, 1, "aColor");
            gl.bind_attrib_location(program, 2, "aOrigin");
            gl.bind_attrib_location(program, 3, "aSize");
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                panic!("shader link error: {}", gl.get_program_info_log(program));
            }
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            self.shader_program = Some(program);

            self.vbo = Some(gl.create_buffer().expect("glCreateBuffer"));

            self.u_viewport_inv_res_loc = gl.get_uniform_location(program, "uViewportInvRes");
        }
    }

    fn enqueue(&mut self, command: DrawRect) {
        if command.color.3 >= 1.0 {
            self.opaque.push(command);
        } else {
            self.translucent.push(command);
        }
    }

    fn process(&mut self, ctx: &RenderContext) {
        if self.opaque.is_empty() && self.translucent.is_empty() {
            return;
        }
        if let Some(program) = self.shader_program {
            let gl = ctx.gl;
            let vp_w = ctx.viewport_width as f32;
            let vp_h = ctx.viewport_height as f32;

            // Interleaved layout per vertex: aPos(2) aColor(4) aOrigin(3) aSize(2) = 11 floats
            const STRIDE: i32 = 11 * size_of::<f32>() as i32;

            unsafe {
                gl.use_program(Some(program));
                gl.bind_buffer(glow::ARRAY_BUFFER, self.vbo);

                gl.enable_vertex_attrib_array(0);
                gl.enable_vertex_attrib_array(1);
                gl.enable_vertex_attrib_array(2);
                gl.enable_vertex_attrib_array(3);
                gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, STRIDE, 0);
                gl.vertex_attrib_pointer_f32(1, 4, glow::FLOAT, false, STRIDE, 2 * 4);
                gl.vertex_attrib_pointer_f32(2, 3, glow::FLOAT, false, STRIDE, 6 * 4);
                gl.vertex_attrib_pointer_f32(3, 2, glow::FLOAT, false, STRIDE, 9 * 4);

                gl.uniform_2_f32(self.u_viewport_inv_res_loc.as_ref(), 2.0 / vp_w, 2.0 / vp_h);

                gl.enable(glow::DEPTH_TEST);
                gl.depth_func(glow::GEQUAL);

                // Pass 1: opaque, front-to-back, depth write on, blending off
                if !self.opaque.is_empty() {
                    let mut sorted: Vec<DrawRect> = self.opaque.drain(..).collect();
                    sorted.sort_unstable_by(|a, b| b.origin.2.total_cmp(&a.origin.2));
                    gl.depth_mask(true);
                    gl.disable(glow::BLEND);

                    let verts = build_rect_verts(&sorted);
                    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck::cast_slice(&verts), glow::STREAM_DRAW);
                    gl.draw_arrays(glow::TRIANGLES, 0, (sorted.len() * 6) as i32);
                }

                // Pass 2: translucent, back-to-front, depth write off, blending on
                if !self.translucent.is_empty() {
                    let mut sorted: Vec<DrawRect> = self.translucent.drain(..).collect();
                    sorted.sort_unstable_by(|a, b| a.origin.2.total_cmp(&b.origin.2));
                    gl.depth_mask(false);
                    gl.enable(glow::BLEND);
                    gl.blend_func_separate(
                        glow::SRC_ALPHA,
                        glow::ONE_MINUS_SRC_ALPHA,
                        glow::ZERO,
                        glow::ONE,
                    );

                    let verts = build_rect_verts(&sorted);
                    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck::cast_slice(&verts), glow::STREAM_DRAW);
                    gl.draw_arrays(glow::TRIANGLES, 0, (sorted.len() * 6) as i32);
                }

                // Restore state
                gl.depth_mask(true);
                gl.disable(glow::DEPTH_TEST);
                gl.disable(glow::BLEND);

                gl.disable_vertex_attrib_array(0);
                gl.disable_vertex_attrib_array(1);
                gl.disable_vertex_attrib_array(2);
                gl.disable_vertex_attrib_array(3);
                gl.bind_buffer(glow::ARRAY_BUFFER, None);
                gl.use_program(None);
            }
        }
    }
}
