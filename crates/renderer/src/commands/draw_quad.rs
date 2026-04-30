use glow::{HasContext, NativeBuffer, NativeProgram, NativeUniformLocation};

use crate::commands::{Command, CommandQueue, CommandQueueRegistry, DrawRect, RenderContext};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrawQuad {
    pub color: (f32, f32, f32, f32),        // r, g, b, a  — offset  0
    pub border_color: (f32, f32, f32, f32), // r, g, b, a  — offset 16
    pub origin: (f32, f32, f32),            // x, y in pixels, z for depth — offset 32
    pub size: (f32, f32),                   // width, height in pixels — offset 44
    pub border_radius: f32,                 // corner radius in pixels — offset 52
    pub border_thickness: f32,              // border width in pixels  — offset 56
}
unsafe impl bytemuck::Pod for DrawQuad {}
unsafe impl bytemuck::Zeroable for DrawQuad {
    fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

impl Command for DrawQuad {
    fn get_queue_from_registry(
        registry: &mut CommandQueueRegistry,
    ) -> &mut impl CommandQueue<Self> {
        &mut registry.draw_quad_queue
    }

    fn on_enqueue(registry: &mut CommandQueueRegistry, command: &DrawQuad) {
        if command.color.3 >= 1.0 {
            let r = command.border_radius;
            let inner_w = command.size.0 - 2.0 * r;
            let inner_h = command.size.1 - 2.0 * r;
            if inner_w > 0.0 && inner_h > 0.0 {
                const Z_EPSILON: f32 = 1e-5;
                DrawRect::get_queue_from_registry(registry).enqueue(DrawRect {
                    color: command.color,
                    origin: (command.origin.0 + r, command.origin.1 + r, command.origin.2 + Z_EPSILON),
                    size: (inner_w, inner_h),
                });
            }
        }
    }
}

// Builds an interleaved vertex buffer (17 floats/vertex, 6 vertices/quad) for a batch of quads.
// Layout per vertex: aPos(2) aColor(4) aBorderColor(4) aOrigin(3) aSize(2) aBorderRadius(1) aBorderThickness(1)
fn build_quad_verts(quads: &[DrawQuad]) -> Vec<f32> {
    #[rustfmt::skip]
    const CORNERS: [(f32, f32); 6] = [
        (-0.5,  0.5), ( 0.5,  0.5), (-0.5, -0.5),  // triangle 1
        ( 0.5,  0.5), ( 0.5, -0.5), (-0.5, -0.5),  // triangle 2
    ];
    let mut v = Vec::with_capacity(quads.len() * 6 * 17);
    for q in quads {
        let (cr, cg, cb, ca) = q.color;
        let (br, bg, bb, ba) = q.border_color;
        let (ox, oy, oz) = q.origin;
        let (sw, sh) = q.size;
        for (px, py) in CORNERS {
            v.extend_from_slice(&[
                px, py,
                cr, cg, cb, ca,
                br, bg, bb, ba,
                ox, oy, oz,
                sw, sh,
                q.border_radius,
                q.border_thickness,
            ]);
        }
    }
    v
}

#[derive(Default)]
pub(crate) struct QuadQueue {
    shader_program: Option<NativeProgram>,
    vbo: Option<NativeBuffer>,

    u_viewport_inv_res_loc: Option<NativeUniformLocation>,

    quads: Vec<DrawQuad>,
}

impl CommandQueue<DrawQuad> for QuadQueue {
    fn init(&mut self, ctx: &RenderContext) {
        unsafe {
            let gl = ctx.gl;
            let program = gl.create_program().expect("glCreateProgram");

            // GLSL ES 1.00 — device is GLES 2.0 (Vivante GC7000).
            // Per-quad data is packed as vertex attributes; all quads draw in one call.
            let vs_src = r#"#version 100
                attribute vec2  aPos;
                attribute vec4  aColor;
                attribute vec4  aBorderColor;
                attribute vec3  aOrigin;
                attribute vec2  aSize;
                attribute float aBorderRadius;
                attribute float aBorderThickness;

                varying vec4  vColor;
                varying vec4  vBorderColor;
                varying vec2  vLocalPos;
                varying vec2  vSize;
                varying float vBorderRadius;
                varying float vBorderThickness;

                uniform vec2 uViewportInvRes;

                void main() {
                    vec2 center   = aOrigin.xy + aSize * 0.5;
                    vec2 pixelPos = vec2(aPos.x, -aPos.y) * aSize + center;
                    vec2 ndc      = pixelPos * uViewportInvRes - 1.0;
                    gl_Position   = vec4(ndc, aOrigin.z, 1.0);

                    vColor           = aColor;
                    vBorderColor     = aBorderColor;
                    vLocalPos        = aPos;
                    vSize            = aSize;
                    vBorderRadius    = aBorderRadius;
                    vBorderThickness = aBorderThickness;
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

                varying vec4  vColor;
                varying vec4  vBorderColor;
                varying vec2  vLocalPos;
                varying vec2  vSize;
                varying float vBorderRadius;
                varying float vBorderThickness;

                float roundedRectSDF(vec2 p, vec2 halfSize, float r) {
                    vec2 q = abs(p) - halfSize + r;
                    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
                }

                void main() {
                    vec2  p        = vLocalPos * vSize;
                    vec2  halfSize = vSize * 0.5;
                    float dist     = roundedRectSDF(p, halfSize, vBorderRadius);

                    float alpha    = 1.0 - smoothstep(-0.5, 0.5, dist);
                    float inBorder = smoothstep(-vBorderThickness - 0.5, -vBorderThickness + 0.5, dist);

                    vec4 color = mix(vColor, vBorderColor, inBorder);
                    color.a   *= alpha;
                    gl_FragColor = color;
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
            gl.bind_attrib_location(program, 2, "aBorderColor");
            gl.bind_attrib_location(program, 3, "aOrigin");
            gl.bind_attrib_location(program, 4, "aSize");
            gl.bind_attrib_location(program, 5, "aBorderRadius");
            gl.bind_attrib_location(program, 6, "aBorderThickness");
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

    fn enqueue(&mut self, command: DrawQuad) {
        self.quads.push(command);
    }

    fn process(&mut self, ctx: &RenderContext) {
        if self.quads.is_empty() {
            return;
        }
        if let Some(program) = self.shader_program {
            let gl = ctx.gl;
            let vp_w = ctx.viewport_width as f32;
            let vp_h = ctx.viewport_height as f32;

            // Interleaved layout per vertex: aPos(2) aColor(4) aBorderColor(4) aOrigin(3) aSize(2) aBorderRadius(1) aBorderThickness(1) = 17 floats
            const STRIDE: i32 = 17 * size_of::<f32>() as i32;

            unsafe {
                gl.use_program(Some(program));
                gl.bind_buffer(glow::ARRAY_BUFFER, self.vbo);

                gl.enable_vertex_attrib_array(0);
                gl.enable_vertex_attrib_array(1);
                gl.enable_vertex_attrib_array(2);
                gl.enable_vertex_attrib_array(3);
                gl.enable_vertex_attrib_array(4);
                gl.enable_vertex_attrib_array(5);
                gl.enable_vertex_attrib_array(6);
                gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, STRIDE, 0);
                gl.vertex_attrib_pointer_f32(1, 4, glow::FLOAT, false, STRIDE,  2 * 4);
                gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, STRIDE,  6 * 4);
                gl.vertex_attrib_pointer_f32(3, 3, glow::FLOAT, false, STRIDE, 10 * 4);
                gl.vertex_attrib_pointer_f32(4, 2, glow::FLOAT, false, STRIDE, 13 * 4);
                gl.vertex_attrib_pointer_f32(5, 1, glow::FLOAT, false, STRIDE, 15 * 4);
                gl.vertex_attrib_pointer_f32(6, 1, glow::FLOAT, false, STRIDE, 16 * 4);

                gl.uniform_2_f32(self.u_viewport_inv_res_loc.as_ref(), 2.0 / vp_w, 2.0 / vp_h);

                // All quads back-to-front; DrawRect pre-pass owns depth writes
                let mut sorted: Vec<DrawQuad> = self.quads.drain(..).collect();
                sorted.sort_unstable_by(|a, b| a.origin.2.total_cmp(&b.origin.2));

                gl.enable(glow::DEPTH_TEST);
                gl.depth_func(glow::GEQUAL);
                gl.depth_mask(false);
                gl.enable(glow::BLEND);
                gl.blend_func_separate(
                    glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA,
                    glow::ZERO, glow::ONE,
                );

                let verts = build_quad_verts(&sorted);
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck::cast_slice(&verts), glow::STREAM_DRAW);
                gl.draw_arrays(glow::TRIANGLES, 0, (sorted.len() * 6) as i32);

                // Restore state
                gl.depth_mask(true);
                gl.disable(glow::DEPTH_TEST);
                gl.disable(glow::BLEND);

                for i in 0..=6 {
                    gl.disable_vertex_attrib_array(i);
                }
                gl.bind_buffer(glow::ARRAY_BUFFER, None);
                gl.use_program(None);
            }
        }
    }
}
