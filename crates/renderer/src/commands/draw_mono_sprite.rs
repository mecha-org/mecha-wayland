use std::mem::size_of;

use glow::{HasContext, NativeBuffer, NativeProgram, NativeUniformLocation};
use utils::{Color, Point, Rect, Size};

use crate::{
    commands::{Command, CommandQueue, RenderContext},
    texture::TextureId,
};

#[derive(Clone)]
pub struct DrawMonochromeSprite {
    pub texture_id: TextureId,
    pub region: Rect,
    pub origin: Point,
    pub z: f32,
    pub size: Size,
    pub color: Color,
}

impl Command for DrawMonochromeSprite {
    fn get_queue_from_registry(
        registry: &mut super::CommandQueueRegistry,
    ) -> &mut impl CommandQueue<Self> {
        &mut registry.draw_mono_sprite_queue
    }
}

#[derive(Default)]
pub(crate) struct MonoSpriteQueue {
    shader_program: Option<NativeProgram>,
    vbo: Option<NativeBuffer>,
    u_viewport_inv_res_loc: Option<NativeUniformLocation>,
    u_tex_inv_size_loc: Option<NativeUniformLocation>,
    u_texture_loc: Option<NativeUniformLocation>,
    sprites: Vec<DrawMonochromeSprite>,
}

// Interleaved vertex layout: aPos(2) aColor(4) aOrigin(3) aSize(2) aRegion(4) = 15 floats/vertex
fn build_sprite_verts(sprites: &[DrawMonochromeSprite]) -> Vec<f32> {
    #[rustfmt::skip]
    const CORNERS: [(f32, f32); 6] = [
        (-0.5,  0.5), ( 0.5,  0.5), (-0.5, -0.5),
        ( 0.5,  0.5), ( 0.5, -0.5), (-0.5, -0.5),
    ];
    let mut v = Vec::with_capacity(sprites.len() * 6 * 15);
    for s in sprites {
        let (ox, oy) = s.origin.as_tuple();
        let oz = s.z;
        let (sw, sh) = s.size.as_tuple();
        let rx = s.region.x();
        let ry = s.region.y();
        let rw = s.region.width();
        let rh = s.region.height();
        for (px, py) in CORNERS {
            v.extend_from_slice(&[
                px, py, s.color.r, s.color.g, s.color.b, s.color.a, ox, oy, oz, sw, sh, rx, ry, rw,
                rh,
            ]);
        }
    }
    v
}

impl CommandQueue<DrawMonochromeSprite> for MonoSpriteQueue {
    fn init(&mut self, ctx: &RenderContext) {
        unsafe {
            let gl = ctx.gl;
            let program = gl.create_program().expect("glCreateProgram");

            // UV mapping: aPos is in [-0.5, 0.5]. We map it to [0, 1] in both axes,
            // accounting for the Y-flip between screen space (Y down) and texture space (Y down).
            // uv_frac.x = aPos.x + 0.5  (left→0, right→1)
            // uv_frac.y = 0.5 - aPos.y  (top→0, bottom→1)
            let vs_src = r#"#version 100
                attribute vec2 aPos;
                attribute vec4 aColor;
                attribute vec3 aOrigin;
                attribute vec2 aSize;
                attribute vec4 aRegion;

                varying vec4 vColor;
                varying vec2 vUV;

                uniform vec2 uViewportInvRes;
                uniform vec2 uTexInvSize;

                void main() {
                    vec2 center   = aOrigin.xy + aSize * 0.5;
                    vec2 pixelPos = vec2(aPos.x, -aPos.y) * aSize + center;
                    vec2 ndc      = pixelPos * uViewportInvRes - 1.0;
                    gl_Position   = vec4(ndc, aOrigin.z, 1.0);

                    vColor = aColor;

                    vec2 uv_frac = vec2(aPos.x + 0.5, 0.5 - aPos.y);
                    vUV = (aRegion.xy + uv_frac * aRegion.zw) * uTexInvSize;
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

            // Texture is R8 (uploaded as GL_LUMINANCE on GLES 2.0).
            // The red channel acts as an alpha mask multiplied with the tint color's alpha.
            let fs_src = r#"#version 100
                precision mediump float;
                varying vec4 vColor;
                varying vec2 vUV;
                uniform sampler2D uTexture;

                void main() {
                    float mask   = texture2D(uTexture, vUV).r;
                    gl_FragColor = vec4(vColor.rgb, vColor.a * mask);
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
            gl.bind_attrib_location(program, 0, "aPos");
            gl.bind_attrib_location(program, 1, "aColor");
            gl.bind_attrib_location(program, 2, "aOrigin");
            gl.bind_attrib_location(program, 3, "aSize");
            gl.bind_attrib_location(program, 4, "aRegion");
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                panic!("shader link error: {}", gl.get_program_info_log(program));
            }
            gl.delete_shader(vs);
            gl.delete_shader(fs);

            self.u_viewport_inv_res_loc = gl.get_uniform_location(program, "uViewportInvRes");
            self.u_tex_inv_size_loc = gl.get_uniform_location(program, "uTexInvSize");
            self.u_texture_loc = gl.get_uniform_location(program, "uTexture");
            self.shader_program = Some(program);
            self.vbo = Some(gl.create_buffer().expect("glCreateBuffer"));
        }
    }

    fn enqueue(&mut self, command: DrawMonochromeSprite) {
        self.sprites.push(command);
    }

    fn process(&mut self, ctx: &RenderContext) {
        let mut sprites = std::mem::take(&mut self.sprites);
        if sprites.is_empty() {
            return;
        }

        let Some(program) = self.shader_program else {
            return;
        };

        // Back-to-front sort: farthest first, closest last — so closer
        // sprites at higher z overwrite farther ones for correct blending.
        sprites.sort_unstable_by(|a, b| a.z.total_cmp(&b.z));

        let gl = ctx.gl;
        let vp_w = ctx.viewport_width as f32;
        let vp_h = ctx.viewport_height as f32;

        // aPos(2) aColor(4) aOrigin(3) aSize(2) aRegion(4) = 15 floats/vertex
        const STRIDE: i32 = 15 * size_of::<f32>() as i32;

        unsafe {
            gl.use_program(Some(program));
            gl.bind_buffer(glow::ARRAY_BUFFER, self.vbo);

            gl.enable_vertex_attrib_array(0);
            gl.enable_vertex_attrib_array(1);
            gl.enable_vertex_attrib_array(2);
            gl.enable_vertex_attrib_array(3);
            gl.enable_vertex_attrib_array(4);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, STRIDE, 0);
            gl.vertex_attrib_pointer_f32(1, 4, glow::FLOAT, false, STRIDE, 2 * 4);
            gl.vertex_attrib_pointer_f32(2, 3, glow::FLOAT, false, STRIDE, 6 * 4);
            gl.vertex_attrib_pointer_f32(3, 2, glow::FLOAT, false, STRIDE, 9 * 4);
            gl.vertex_attrib_pointer_f32(4, 4, glow::FLOAT, false, STRIDE, 11 * 4);

            gl.uniform_2_f32(self.u_viewport_inv_res_loc.as_ref(), 2.0 / vp_w, 2.0 / vp_h);

            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::GEQUAL);
            gl.depth_mask(false);
            gl.enable(glow::BLEND);
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );

            gl.active_texture(glow::TEXTURE0);
            gl.uniform_1_i32(self.u_texture_loc.as_ref(), 0);

            let mut i = 0usize;
            while i < sprites.len() {
                let tex_id = sprites[i].texture_id;
                let end = {
                    let mut e = i + 1;
                    while e < sprites.len() && sprites[e].texture_id == tex_id {
                        e += 1;
                    }
                    e
                };

                let Some(gpu_tex) = ctx.textures.get(&tex_id) else {
                    i = end;
                    continue;
                };

                gl.bind_texture(glow::TEXTURE_2D, Some(gpu_tex.handle));
                gl.uniform_2_f32(
                    self.u_tex_inv_size_loc.as_ref(),
                    1.0 / gpu_tex.width as f32,
                    1.0 / gpu_tex.height as f32,
                );

                let batch = &sprites[i..end];
                let verts = build_sprite_verts(batch);
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    bytemuck::cast_slice(&verts),
                    glow::STREAM_DRAW,
                );
                gl.draw_arrays(glow::TRIANGLES, 0, (batch.len() * 6) as i32);
                i = end;
            }

            gl.depth_mask(true);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::BLEND);
            gl.bind_texture(glow::TEXTURE_2D, None);

            gl.disable_vertex_attrib_array(0);
            gl.disable_vertex_attrib_array(1);
            gl.disable_vertex_attrib_array(2);
            gl.disable_vertex_attrib_array(3);
            gl.disable_vertex_attrib_array(4);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.use_program(None);
        }
    }
}
