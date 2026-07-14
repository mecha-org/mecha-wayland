use std::collections::HashMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::ptr::NonNull;

use app::{prelude::*, RegisteredModule, Start};
use wayland::{
    Handle, Interface, ObjectId, WlBuffer, WlBufferRequest, WlShm, WlShmFormat, WlShmPoolRequest,
    WlShmRequest,
};

use crate::protocols::wl_registry::RegisterGlobal;

pub struct ShmPool {
    pub ptr: NonNull<u8>,
    pub size: usize,
    pub fd: OwnedFd,
    pub pending_destroy: bool,
}

unsafe impl Send for ShmPool {}

pub struct ShmBuffer {
    pub pool_id: ObjectId,
    pub offset: i32,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub format: WlShmFormat,
    pub ptr: NonNull<u8>,
    pub handle: Handle<WlBuffer>,
}

unsafe impl Send for ShmBuffer {}

#[derive(State)]
pub struct WlShmState {
    pub pools: HashMap<ObjectId, ShmPool>,
    pub buffers: HashMap<ObjectId, ShmBuffer>,
}

impl WlShmState {
    pub fn new() -> Self {
        Self { pools: HashMap::new(), buffers: HashMap::new() }
    }
}

pub fn module<S>() -> impl RegisteredModule<WlShmState, S> {
    Module::<WlShmState, _, _>::new()
        .on(|_: &mut WlShmState, _: &Start| -> Option<RegisterGlobal> {
            Some(RegisterGlobal { interface: WlShm::NAME, version: WlShm::VERSION })
        })
        .on(|state: &mut WlShmState, ev: &WlShmRequest| {
            match ev {
                WlShmRequest::CreatePool { sender: _, id, fd, size, } => {
                    let size = *size as usize;
                    let ptr = unsafe {
                        libc::mmap(
                            std::ptr::null_mut(),
                            size,
                            libc::PROT_READ,
                            libc::MAP_SHARED,
                            fd.as_raw_fd(),
                            0,
                        )
                    };
                    assert_ne!(ptr, libc::MAP_FAILED, "mmap failed");
                    let ptr = NonNull::new(ptr as *mut u8).expect("non-null");
                    let owned_fd = unsafe { OwnedFd::from_raw_fd(libc::dup(fd.as_raw_fd())) };

                    let pool_id = id.object_id().expect("live pool");
                    state.pools.insert(pool_id, ShmPool { ptr, size, fd: owned_fd, pending_destroy: false });
                }
                WlShmRequest::Release { .. } => {}
            }
            hlist![]
        })
        .on(|state: &mut WlShmState, ev: &WlShmPoolRequest| {
            match ev {
                WlShmPoolRequest::CreateBuffer { sender, id, offset, width, height, stride, format } => {
                    let pool_id = sender.object_id().expect("live pool");
                    let pool = state.pools.get(&pool_id).expect("CreateBuffer on unknown pool");
                    let ptr = unsafe {
                        NonNull::new_unchecked(pool.ptr.as_ptr().add(*offset as usize))
                    };
                    let buf_id = id.object_id().expect("live buffer");
                    state.buffers.insert(buf_id, ShmBuffer {
                        pool_id,
                        offset: *offset,
                        width: *width,
                        height: *height,
                        stride: *stride,
                        format: *format,
                        ptr,
                        handle: id.clone(),
                    });
                }
                WlShmPoolRequest::Destroy { sender } => {
                    let pool_id = sender.object_id().expect("live pool");
                    let has_buffers = state.buffers.values().any(|b| b.pool_id == pool_id);
                    if has_buffers {
                        if let Some(pool) = state.pools.get_mut(&pool_id) {
                            pool.pending_destroy = true;
                        }
                    } else if let Some(pool) = state.pools.remove(&pool_id) {
                        unsafe { libc::munmap(pool.ptr.as_ptr() as *mut _, pool.size) };
                    }
                }
                WlShmPoolRequest::Resize { sender, size } => {
                    let pool_id = sender.object_id().expect("live pool");
                    if let Some(pool) = state.pools.get_mut(&pool_id) {
                        let new_size = *size as usize;
                        let new_ptr = unsafe {
                            libc::mremap(
                                pool.ptr.as_ptr() as *mut _,
                                pool.size,
                                new_size,
                                libc::MREMAP_MAYMOVE,
                            )
                        };
                        assert_ne!(new_ptr, libc::MAP_FAILED, "mremap failed");
                        pool.ptr = NonNull::new(new_ptr as *mut u8).expect("non-null");
                        pool.size = new_size;
                        for buf in state.buffers.values_mut() {
                            if buf.pool_id == pool_id {
                                buf.ptr = unsafe {
                                    NonNull::new_unchecked(pool.ptr.as_ptr().add(buf.offset as usize))
                                };
                            }
                        }
                    }
                }
            }
            hlist![]
        })
        .on(|state: &mut WlShmState, ev: &WlBufferRequest| {
            let WlBufferRequest::Destroy { sender } = ev;
            let buf_id = sender.object_id().expect("live buffer");
            if let Some(buf) = state.buffers.remove(&buf_id) {
                let pool_id = buf.pool_id;
                let should_destroy = state
                    .pools
                    .get(&pool_id)
                    .map_or(false, |p| p.pending_destroy)
                    && !state.buffers.values().any(|b| b.pool_id == pool_id);
                if should_destroy {
                    if let Some(pool) = state.pools.remove(&pool_id) {
                        unsafe { libc::munmap(pool.ptr.as_ptr() as *mut _, pool.size) };
                    }
                }
            }
            hlist![]
        })
}
