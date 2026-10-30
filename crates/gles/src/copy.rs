#![allow(unsafe_code)]
//! Reusing the presented frame: one target copied into another, the
//! scrolled area moved, with `glBlitFramebuffer`.

use geometry::Rect;
use glow::HasContext;

use crate::draw::scissor;
use crate::egl::Gpu;
use crate::target::Target;

/// `from` into `to`: each of `copy` as it is, then `shift.area` moved by
/// `-shift.by`. The strip the move exposed is left as it was in `to`.
pub(crate) fn shift(gpu: &Gpu, from: &Target, to: &Target, shift: render::Shift, copy: &[Rect]) {
    let gl = &gpu.gl;
    let (ax, ay, aw, ah) = scissor(shift.area);
    let (bx, by) = (shift.by.x as i32, shift.by.y as i32);
    // SAFETY: the context is current; both FBOs are live targets'.
    unsafe {
        gl.disable(glow::SCISSOR_TEST);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(from.fbo));
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(to.fbo));
        let blit = |x: i32, y: i32, w: i32, h: i32, dx: i32, dy: i32| {
            if w > 0 && h > 0 {
                gl.blit_framebuffer(
                    x + dx,
                    y + dy,
                    x + dx + w,
                    y + dy + h,
                    x,
                    y,
                    x + w,
                    y + h,
                    glow::COLOR_BUFFER_BIT,
                    glow::NEAREST,
                );
            }
        };
        // Only what the buffer lacks outside the area: each blit costs
        // about as much as its size on the Comet, so copying the rest of
        // the window every frame cost a fifth of the frame.
        for &r in copy {
            let (x, y, w, h) = scissor(r);
            blit(x, y, w, h, 0, 0);
        }
        // What stays in sight: where it lands, read from where it was.
        let x = if bx > 0 { ax } else { ax - bx };
        let y = if by > 0 { ay } else { ay - by };
        blit(x, y, aw - bx.abs(), ah - by.abs(), bx, by);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
    }
}
