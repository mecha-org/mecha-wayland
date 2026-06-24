use std::{
    os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
    ptr,
};
use wayland::{Handle, WlBuffer, WlShm, WlShmFormat};

use crate::window::Window;

pub fn render_window(window: &mut Window<()>, shm: &Handle<WlShm>, width: u32, height: u32) {
    let buffer = alloc_shm_buffer(shm, width, height, window.color);
    window.surface.attach(Some(&buffer), 0, 0);
    window.surface.damage(0, 0, width as i32, height as i32);
    window.surface.commit();
    window.buffer = Some(buffer);
}

fn alloc_shm_buffer(shm: &Handle<WlShm>, width: u32, height: u32, color: u32) -> Handle<WlBuffer> {
    let stride = width * 4;
    let size = (stride * height) as usize;

    let fd: OwnedFd = unsafe {
        let raw = libc::memfd_create(c"wm_shm_buf".as_ptr(), libc::MFD_CLOEXEC);
        assert!(raw >= 0, "memfd_create failed");
        assert_eq!(libc::ftruncate(raw, size as i64), 0, "ftruncate failed");
        OwnedFd::from_raw_fd(raw)
    };

    unsafe {
        let p = libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        );
        assert!(p != libc::MAP_FAILED, "mmap failed");
        let pixels = std::slice::from_raw_parts_mut(p as *mut u32, size / 4);
        pixels.fill(color);
        libc::munmap(p, size);
    }

    let pool = shm.create_pool(fd.as_fd(), size as i32);
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        stride as i32,
        WlShmFormat::Xrgb8888,
    );
    pool.destroy();
    buffer
}
