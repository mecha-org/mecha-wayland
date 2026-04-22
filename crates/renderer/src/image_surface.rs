use anyhow::{Result, bail};
use glow::HasContext;

use crate::{Renderer, SurfaceBackend};

/// Offscreen render target backed by a plain GL RGBA texture.
///
/// After rendering, bind `texture` as a `sampler2D` to use the result in
/// another draw pass. Create via `Renderer::create_surface::<ImageSurface>()`.
pub struct ImageSurface {
    pub texture: glow::Texture,
    pub depth_rbo: glow::Renderbuffer,
    pub fbo: glow::Framebuffer,
}

impl SurfaceBackend for ImageSurface {
    fn allocate(renderer: &Renderer, width: u32, height: u32) -> Result<Self> {
        let gl = &renderer.gl;

        let texture = unsafe { gl.create_texture() }
            .map_err(|e| anyhow::anyhow!("glGenTextures: {e}"))?;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        }

        let depth_rbo = unsafe { gl.create_renderbuffer() }
            .map_err(|e| anyhow::anyhow!("glGenRenderbuffers (depth): {e}"))?;
        unsafe {
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth_rbo));
            gl.renderbuffer_storage(
                glow::RENDERBUFFER,
                glow::DEPTH_COMPONENT24,
                width as i32,
                height as i32,
            );
        }

        let fbo = unsafe { gl.create_framebuffer() }
            .map_err(|e| anyhow::anyhow!("glGenFramebuffers: {e}"))?;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(depth_rbo),
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            if status != glow::FRAMEBUFFER_COMPLETE {
                bail!("ImageSurface FBO incomplete: 0x{:x}", status);
            }
        }
        tracing::info!(width, height, "ImageSurface FBO complete");

        Ok(Self { texture, depth_rbo, fbo })
    }

    fn fbo(&self) -> glow::Framebuffer {
        self.fbo
    }

    fn destroy(self, renderer: &Renderer) {
        let gl = &renderer.gl;
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_renderbuffer(self.depth_rbo);
            gl.delete_texture(self.texture);
        }
    }
}
