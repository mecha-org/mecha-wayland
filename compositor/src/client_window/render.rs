use std::os::fd::AsFd;

use renderer::{DmaBuf, Renderer};
use wayland::{Handle, ZwpLinuxBufferParamsV1Flags, ZwpLinuxDmabufV1};

use super::Slot;

const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;

pub fn alloc_slots(
    renderer: &mut Renderer,
    dmabuf: &Handle<ZwpLinuxDmabufV1>,
    width: u32,
    height: u32,
) -> [Slot; 2] {
    std::array::from_fn(|_| alloc_slot(renderer, dmabuf, width, height))
}

fn alloc_slot(
    renderer: &mut Renderer,
    dmabuf: &Handle<ZwpLinuxDmabufV1>,
    width: u32,
    height: u32,
) -> Slot {
    let surface = renderer
        .create_surface::<DmaBuf>(width, height)
        .expect("DmaBuf surface allocation failed");

    let buffer = {
        let fd = surface.backend.prime_fd.as_fd();
        let stride = surface.backend.stride;
        let modifier = surface.backend.modifier;
        let params = dmabuf.create_params();
        params.add(
            fd,
            0,
            0,
            stride,
            (modifier >> 32) as u32,
            (modifier & 0xffff_ffff) as u32,
        );
        params.create_immed(
            width as i32,
            height as i32,
            DRM_FORMAT_ARGB8888,
            ZwpLinuxBufferParamsV1Flags::empty(),
        )
    };

    Slot {
        surface,
        buffer,
        released: true,
    }
}
