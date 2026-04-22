use glow::{HasContext, NativeBuffer, NativeProgram, NativeUniformLocation, NativeVertexArray};

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

    vao: Option<NativeVertexArray>,
    vbo: Option<NativeBuffer>,
    ibo: Option<NativeBuffer>,

    u_viewport_inv_res_loc: Option<NativeUniformLocation>,

    opaque: Vec<DrawRect>,
    translucent: Vec<DrawRect>,
}

impl CommandQueue<DrawRect> for RectQueue {
    fn init(&mut self, ctx: &RenderContext) {
        unsafe {
            let gl = ctx.gl;
            let program = gl.create_program().expect("glCreateProgram");
            let vs_src = r#"#version 300 es
                layout(location = 0) in vec2 aPos;
                layout(location = 1) in vec4 aColor;
                layout(location = 2) in vec3 aOrigin;
                layout(location = 3) in vec2 aSize;
                out vec4 vColor;

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

            let fs_src = r#"#version 300 es
                precision mediump float;
                in vec4 vColor;
                out vec4 fragColor;
                void main() {
                    fragColor = vColor;
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
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                panic!("shader link error: {}", gl.get_program_info_log(program));
            }
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            self.shader_program = Some(program);

            self.vao = Some(gl.create_vertex_array().expect("glCreateVertexArray"));
            gl.bind_vertex_array(self.vao);

            self.vbo = Some(gl.create_buffer().expect("glCreateBuffer"));
            gl.bind_buffer(glow::ARRAY_BUFFER, self.vbo);
            #[rustfmt::skip]
            let vertices: [f32; 8] = [
                -0.5,  0.5,   // TL
                 0.5,  0.5,   // TR
                -0.5, -0.5,   // BL
                 0.5, -0.5,   // BR
            ];
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&vertices),
                glow::STATIC_DRAW,
            );
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(
                0,
                2,
                glow::FLOAT,
                false,
                (2 * size_of::<f32>()) as i32,
                0,
            );

            self.ibo = Some(gl.create_buffer().expect("glCreateBuffer"));
            gl.bind_buffer(glow::ARRAY_BUFFER, self.ibo);
            let size = 1024 * 1024;
            gl.buffer_data_size(glow::ARRAY_BUFFER, size, glow::DYNAMIC_DRAW);

            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 4, glow::FLOAT, false, size_of::<DrawRect>() as i32, 0);
            gl.vertex_attrib_divisor(1, 1);

            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(
                2,
                3,
                glow::FLOAT,
                false,
                size_of::<DrawRect>() as i32,
                4 * size_of::<f32>() as i32,
            );
            gl.vertex_attrib_divisor(2, 1);

            gl.enable_vertex_attrib_array(3);
            gl.vertex_attrib_pointer_f32(
                3,
                2,
                glow::FLOAT,
                false,
                size_of::<DrawRect>() as i32,
                7 * size_of::<f32>() as i32,
            );
            gl.vertex_attrib_divisor(3, 1);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);

            self.u_viewport_inv_res_loc =
                gl.get_uniform_location(program, "uViewportInvRes");
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

            unsafe {
                gl.bind_vertex_array(self.vao);
                gl.bind_buffer(glow::ARRAY_BUFFER, self.ibo);
                gl.use_program(Some(program));

                gl.uniform_2_f32(self.u_viewport_inv_res_loc.as_ref(), 2.0 / vp_w, 2.0 / vp_h);

                gl.enable(glow::DEPTH_TEST);
                gl.depth_func(glow::GEQUAL);

                // Pass 1: opaque, front-to-back, depth write on, blending off
                if !self.opaque.is_empty() {
                    let mut sorted: Vec<DrawRect> = self.opaque.drain(..).collect();
                    sorted.sort_unstable_by(|a, b| b.origin.2.total_cmp(&a.origin.2));
                    gl.depth_mask(true);
                    gl.disable(glow::BLEND);
                    gl.buffer_sub_data_u8_slice(
                        glow::ARRAY_BUFFER,
                        0,
                        bytemuck::cast_slice(&sorted),
                    );
                    gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, sorted.len() as i32);
                }

                // Pass 2: translucent, back-to-front, depth write off, blending on
                if !self.translucent.is_empty() {
                    let mut sorted: Vec<DrawRect> = self.translucent.drain(..).collect();
                    sorted.sort_unstable_by(|a, b| a.origin.2.total_cmp(&b.origin.2));
                    gl.depth_mask(false);
                    gl.enable(glow::BLEND);
                    gl.blend_func_separate(
                        glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA,
                        glow::ZERO, glow::ONE,
                    );
                    gl.buffer_sub_data_u8_slice(
                        glow::ARRAY_BUFFER,
                        0,
                        bytemuck::cast_slice(&sorted),
                    );
                    gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, sorted.len() as i32);
                }

                // Restore state
                gl.depth_mask(true);
                gl.disable(glow::DEPTH_TEST);
                gl.disable(glow::BLEND);

                gl.use_program(None);
            }
        }
    }
}
