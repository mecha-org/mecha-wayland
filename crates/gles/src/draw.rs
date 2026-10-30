#![allow(unsafe_code)]
use geometry::Rect;
use glow::HasContext;
use render::Command;

use crate::egl::Gpu;
use crate::program::{Programs, Shader};
use crate::target::Target;

/// A device-pixel rect rounded outward to whole pixels, as GL scissor
/// arguments: `(x, y, w, h)`. GL's y and the device's are the same y, so
/// nothing is flipped.
pub(crate) fn scissor(r: Rect) -> (i32, i32, i32, i32) {
    let x0 = r.x().floor() as i32;
    let y0 = r.y().floor() as i32;
    let x1 = r.right().ceil() as i32;
    let y1 = r.bottom().ceil() as i32;
    (x0, y0, x1 - x0, y1 - y0)
}

pub(crate) fn clear(gpu: &Gpu, t: &Target, q: &render::Queue) {
    let gl = &gpu.gl;
    // SAFETY: the context is current and the FBO is this target's.
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(t.fbo));
        gl.viewport(0, 0, t.width() as i32, t.height() as i32);
        gl.enable(glow::SCISSOR_TEST);
        gl.depth_mask(true);
        gl.clear_color(q.clear.r, q.clear.g, q.clear.b, 1.0);
        gl.clear_depth_f32(1.0);
        for &r in &q.scissor {
            let (x, y, w, h) = scissor(r);
            gl.scissor(x, y, w, h);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }
}

pub(crate) fn finish(gpu: &Gpu) {
    // SAFETY: the context is current.
    unsafe { gpu.gl.flush() }
}

/// The command list as the bytes it is. `Command` is `repr(C)`,
/// 124 bytes, all `f32` and `u32`, no padding; `tests/layout.rs`
/// pins that. The length comes from `size_of::<Command>()` rather than
/// from [`Program::STRIDE`], and a `const _` beside `STRIDE` asserts the
/// two are equal, so the slice can never be longer than the allocation.
fn bytes(cmds: &[Command]) -> &[u8] {
    // SAFETY: a `repr(C)` struct of plain numbers has no padding and
    // no invalid byte patterns; the length is exact.
    unsafe { std::slice::from_raw_parts(cmds.as_ptr() as *const u8, std::mem::size_of_val(cmds)) }
}

/// `sorted` is the device's buffer for the opaque pass in program order;
/// it keeps its allocation from frame to frame.
pub(crate) fn passes(
    gpu: &Gpu,
    p: &Programs,
    t: &Target,
    q: &render::Queue,
    sorted: &mut Vec<Command>,
) {
    let gl = &gpu.gl;
    // The opaque pass is order-free under the depth test, so it is grouped
    // by program; the sort is stable, so each group stays front to back.
    sorted.clear();
    sorted.extend_from_slice(&q.opaque.commands);
    sorted.sort_by_key(Shader::of);
    let opaque = bytes(sorted);
    let translucent = bytes(&q.translucent.commands);
    // SAFETY: the context is current; the programs, VAO and VBO are
    // the device's; every slice is sized by its command count.
    unsafe {
        gl.bind_vertex_array(Some(p.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(p.vbo));
        gl.buffer_data_size(
            glow::ARRAY_BUFFER,
            (opaque.len() + translucent.len()) as i32,
            glow::STREAM_DRAW,
        );
        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, opaque);
        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, opaque.len() as i32, translucent);
        for v in p.all() {
            gl.use_program(Some(v.program));
            gl.uniform_2_f32(v.u_size.as_ref(), t.width() as f32, t.height() as f32);
            gl.uniform_1_f32(v.u_depth.as_ref(), q.depth);
        }
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LESS);

        if !q.opaque.scissor.is_empty() {
            gl.depth_mask(true);
            gl.disable(glow::BLEND);
            runs(gl, p, sorted, 0, &q.opaque.scissor, true);
        }
        if !q.translucent.scissor.is_empty() {
            gl.depth_mask(false);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            runs(
                gl,
                p,
                &q.translucent.commands,
                opaque.len() as i32,
                &q.translucent.scissor,
                false,
            );
        }
    }
}

/// Each maximal run of one program in `cmds`, which start `base` bytes
/// into the VBO: one instanced draw of the run per scissor rect. In order,
/// so a translucent pass composites as listed.
unsafe fn runs(
    gl: &glow::Context,
    p: &Programs,
    cmds: &[Command],
    base: i32,
    scissor: &[Rect],
    opaque: bool,
) {
    let mut start = 0;
    while start < cmds.len() {
        let shader = Shader::of(&cmds[start]);
        let len = cmds[start..]
            .iter()
            .take_while(|c| Shader::of(c) == shader)
            .count();
        let v = p.get(shader);
        // SAFETY: as the caller's.
        unsafe {
            gl.use_program(Some(v.program));
            gl.uniform_1_i32(v.u_opaque.as_ref(), opaque as i32);
            p.point(gl, base + start as i32 * Programs::STRIDE);
            for &r in scissor {
                let (x, y, w, h) = self::scissor(r);
                gl.scissor(x, y, w, h);
                gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, len as i32);
            }
        }
        start += len;
    }
}
