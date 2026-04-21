use std::collections::VecDeque;

use glow::{HasContext, NativeBuffer, NativeProgram};

use crate::commands::{Command, CommandQueue, RenderContext};

#[derive(Clone)]
pub struct DrawRect {
    pub color: (f32, f32, f32, f32), // r, g, b, a in f32 from 0.0 to 1.0
    pub origin: (f32, f32, f32),     // x, y in pixels (top-left origin), z unchanged
    pub size: (f32, f32),            // width, height in pixels
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
    queue: VecDeque<DrawRect>,
}

impl CommandQueue<DrawRect> for RectQueue {
    fn init(&mut self, ctx: &RenderContext) {
        unsafe {
            let gl = ctx.gl;
            let program = gl.create_program().expect("glCreateProgram");
            let vs_src = r#"#version 300 es
                layout(location = 0) in vec3 aPos;
                layout(location = 1) in vec4 aColor;
                out vec4 vColor;
                void main() {
                    gl_Position = vec4(aPos, 1.0);
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

            self.vbo = Some(gl.create_buffer().expect("glCreateBuffer"));
            gl.bind_buffer(glow::ARRAY_BUFFER, self.vbo);

            let size = 1024 * 1024;
            gl.buffer_data_size(glow::ARRAY_BUFFER, size, glow::DYNAMIC_DRAW);

            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
    }

    fn enqueue(&mut self, command: DrawRect) {
        self.queue.push_back(command);
    }

    fn process(&mut self, ctx: &RenderContext) {
        if self.queue.is_empty() {
            return;
        }
        if let Some(program) = self.shader_program {
            let gl = ctx.gl;
            let vp_w = ctx.viewport_width as f32;
            let vp_h = ctx.viewport_height as f32;
            let to_ndc_x = |px: f32| (px / vp_w) * 2.0 - 1.0;
            let to_ndc_y = |py: f32| 1.0 - (py / vp_h) * 2.0;

            unsafe {
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                gl.use_program(Some(program));

                let mut vertices = vec![];

                while let Some(cmd) = self.queue.pop_front() {
                    let (px, py, z) = cmd.origin;
                    let (pw, ph) = cmd.size;
                    let x0 = to_ndc_x(px);
                    let y0 = to_ndc_y(py);
                    let x1 = to_ndc_x(px + pw);
                    let y1 = to_ndc_y(py + ph);

                    // Interleaved: [x, y, z, r, g, b, a] per vertex. Coords in NDC.
                    #[rustfmt::skip]
                    vertices.extend([
                        x0, y0, z,  cmd.color.0, cmd.color.1, cmd.color.2, cmd.color.3,
                        x1, y0, z,  cmd.color.0, cmd.color.1, cmd.color.2, cmd.color.3,
                        x1, y1, z,  cmd.color.0, cmd.color.1, cmd.color.2, cmd.color.3,

                        x0, y0, z,  cmd.color.0, cmd.color.1, cmd.color.2, cmd.color.3,
                        x1, y1, z,  cmd.color.0, cmd.color.1, cmd.color.2, cmd.color.3,
                        x0, y1, z,  cmd.color.0, cmd.color.1, cmd.color.2, cmd.color.3,
                    ]);
                }

                gl.bind_buffer(glow::ARRAY_BUFFER, self.vbo);
                gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytemuck::cast_slice(&vertices));
                let stride = 7 * std::mem::size_of::<f32>() as i32;

                gl.enable_vertex_attrib_array(0);
                gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);

                gl.enable_vertex_attrib_array(1);
                gl.vertex_attrib_pointer_f32(
                    1,
                    4,
                    glow::FLOAT,
                    false,
                    stride,
                    3 * size_of::<f32>() as i32,
                );
                let count = (vertices.len() / 7) as i32;
                gl.draw_arrays(glow::TRIANGLES, 0, count);

                gl.use_program(None);
            }
        }
    }
}
