//! Two dmabuf slots per window: a `gles::Target` each, its `wl_buffer`
//! from `zwp_linux_dmabuf_v1`, whether the compositor holds it, and
//! when it was last drawn. Made and torn down through the device.

use gles::{Device, Target};
use wayland::prelude::*;

use crate::BUFFERS;

pub(crate) struct Slot {
    pub(crate) target: Target,
    pub(crate) buffer: WlBuffer,
    /// Attached and not yet released by the compositor.
    pub(crate) held: bool,
    /// The window's frame counter when this slot was last drawn; `None`
    /// until then.
    pub(crate) drawn: Option<u64>,
}

pub(crate) struct Slots {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) slots: [Slot; BUFFERS],
}

fn slot(
    device: &mut Device,
    wl: &mut Wayland,
    dmabuf: ZwpLinuxDmabufV1,
    alpha: bool,
    modifiers: &[u64],
    width: u32,
    height: u32,
) -> Slot {
    let target = if alpha {
        device.target_with_alpha(width, height, modifiers)
    } else {
        device.target(width, height, modifiers)
    };
    let params = dmabuf.create_params(wl);
    let modifier = target.modifier();
    for (i, p) in target.planes().enumerate() {
        params.add(
            wl,
            p.fd,
            i as u32,
            p.offset,
            p.stride,
            (modifier >> 32) as u32,
            modifier as u32,
        );
    }
    let buffer = params.create_immed(
        wl,
        width as i32,
        height as i32,
        target.fourcc(),
        ZwpLinuxBufferParamsV1Flags::empty(),
    );
    params.destroy(wl);
    Slot {
        target,
        buffer,
        held: false,
        drawn: None,
    }
}

impl Slots {
    /// [`BUFFERS`] slots of `width` by `height` device pixels, ARGB8888
    /// with `alpha` and XRGB8888 without, laid out by the first of
    /// `modifiers` the GPU accepts, linear when the list is empty.
    pub(crate) fn create(
        device: &mut Device,
        wl: &mut Wayland,
        dmabuf: ZwpLinuxDmabufV1,
        alpha: bool,
        modifiers: &[u64],
        width: u32,
        height: u32,
    ) -> Slots {
        Slots {
            width,
            height,
            slots: std::array::from_fn(|_| {
                slot(device, wl, dmabuf, alpha, modifiers, width, height)
            }),
        }
    }

    pub(crate) fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| !s.held)
    }

    /// Destroys both buffers on the wire and both targets on the device.
    pub(crate) fn destroy(self, device: &mut Device, wl: &mut Wayland) {
        for s in self.slots {
            s.buffer.destroy(wl);
            device.destroy(s.target);
        }
    }
}

/// The slot holding the frame presented last, `frames - 1`, as a shift's
/// source: `None` before any frame, after the slots were reset, or when
/// that slot is `target` itself.
pub(crate) fn previous(drawn: &[Option<u64>], target: usize, frames: u64) -> Option<usize> {
    let last = frames.checked_sub(1)?;
    drawn
        .iter()
        .position(|d| *d == Some(last))
        .filter(|&j| j != target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_previous_slot_is_the_one_drawn_last_frame() {
        assert_eq!(previous(&[Some(4), Some(5)], 0, 6), Some(1));
    }

    #[test]
    fn no_previous_slot_when_it_is_the_target() {
        assert_eq!(previous(&[Some(4), Some(5)], 1, 6), None);
    }

    #[test]
    fn no_previous_slot_after_a_reset_or_before_any_frame() {
        assert_eq!(previous(&[None, None], 0, 6), None);
        assert_eq!(previous(&[None, None], 0, 0), None);
    }
}
