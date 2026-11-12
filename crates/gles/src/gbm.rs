//! GBM, loaded from `libgbm.so.1` when the device opens instead of linked,
//! the way EGL is. Building needs no `libgbm.so`, no GBM headers and no
//! `-devel` package; running needs the `libgbm.so.1` every Mesa install
//! ships.
//!
//! Only the entry points this crate calls are bound. A buffer holds its
//! device, so the device outlives every buffer however they are dropped,
//! and the library outlives both.
#![allow(unsafe_code)]

use std::ffi::{c_int, c_void};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::ptr::NonNull;
use std::rc::Rc;

/// `GBM_BO_USE_RENDERING`.
pub(crate) const USE_RENDERING: u32 = 1 << 2;
/// `GBM_BO_USE_LINEAR`.
pub(crate) const USE_LINEAR: u32 = 1 << 4;
/// `GBM_BO_TRANSFER_READ`.
const TRANSFER_READ: u32 = 1 << 0;

#[repr(C)]
struct RawDevice {
    _opaque: [u8; 0],
}

#[repr(C)]
struct RawBo {
    _opaque: [u8; 0],
}

/// The entry points, resolved once. `_lib` keeps them mapped.
struct Lib {
    create_device: unsafe extern "C" fn(fd: c_int) -> *mut RawDevice,
    device_destroy: unsafe extern "C" fn(gbm: *mut RawDevice),
    bo_create: unsafe extern "C" fn(
        gbm: *mut RawDevice,
        width: u32,
        height: u32,
        format: u32,
        flags: u32,
    ) -> *mut RawBo,
    bo_create_with_modifiers2: unsafe extern "C" fn(
        gbm: *mut RawDevice,
        width: u32,
        height: u32,
        format: u32,
        modifiers: *const u64,
        count: u32,
        flags: u32,
    ) -> *mut RawBo,
    bo_get_modifier: unsafe extern "C" fn(bo: *mut RawBo) -> u64,
    bo_get_plane_count: unsafe extern "C" fn(bo: *mut RawBo) -> c_int,
    bo_get_fd_for_plane: unsafe extern "C" fn(bo: *mut RawBo, plane: c_int) -> c_int,
    bo_get_offset: unsafe extern "C" fn(bo: *mut RawBo, plane: c_int) -> u32,
    bo_get_stride_for_plane: unsafe extern "C" fn(bo: *mut RawBo, plane: c_int) -> u32,
    bo_map: unsafe extern "C" fn(
        bo: *mut RawBo,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        flags: u32,
        stride: *mut u32,
        map_data: *mut *mut c_void,
    ) -> *mut c_void,
    bo_unmap: unsafe extern "C" fn(bo: *mut RawBo, map_data: *mut c_void),
    bo_destroy: unsafe extern "C" fn(bo: *mut RawBo),
    _lib: libloading::Library,
}

impl Lib {
    fn load() -> Result<Lib, String> {
        // SAFETY: libgbm's initialisers only set up Mesa's backend
        // loader; nothing runs that this process could race with.
        let lib = unsafe { libloading::Library::new("libgbm.so.1") }
            .map_err(|e| format!("load libgbm.so.1: {e}"))?;

        macro_rules! sym {
            ($name:literal) => {
                // SAFETY: the symbol is gbm.h's function of that name, and
                // the field it lands in has exactly its C signature.
                *unsafe { lib.get(concat!($name, "\0").as_bytes()) }
                    .map_err(|e| format!("libgbm.so.1 has no {}: {e}", $name))?
            };
        }

        Ok(Lib {
            create_device: sym!("gbm_create_device"),
            device_destroy: sym!("gbm_device_destroy"),
            bo_create: sym!("gbm_bo_create"),
            bo_create_with_modifiers2: sym!("gbm_bo_create_with_modifiers2"),
            bo_get_modifier: sym!("gbm_bo_get_modifier"),
            bo_get_plane_count: sym!("gbm_bo_get_plane_count"),
            bo_get_fd_for_plane: sym!("gbm_bo_get_fd_for_plane"),
            bo_get_offset: sym!("gbm_bo_get_offset"),
            bo_get_stride_for_plane: sym!("gbm_bo_get_stride_for_plane"),
            bo_map: sym!("gbm_bo_map"),
            bo_unmap: sym!("gbm_bo_unmap"),
            bo_destroy: sym!("gbm_bo_destroy"),
            _lib: lib,
        })
    }
}

struct DeviceInner {
    raw: NonNull<RawDevice>,
    lib: Lib,
    /// The render node. GBM borrows the fd, so it closes after
    /// `gbm_device_destroy`: fields drop after `Drop::drop`.
    _node: File,
}

impl Drop for DeviceInner {
    fn drop(&mut self) {
        // SAFETY: made by gbm_create_device; every buffer holds an `Rc`
        // to this, so none is left.
        unsafe { (self.lib.device_destroy)(self.raw.as_ptr()) }
    }
}

/// A `gbm_device` over a render node.
pub(crate) struct Device(Rc<DeviceInner>);

impl Device {
    /// Loads libgbm and opens a device on `node`, which it keeps open.
    pub(crate) fn new(node: File) -> Result<Device, String> {
        let lib = Lib::load()?;
        // SAFETY: a valid open fd, kept open for the device's life.
        let raw = unsafe { (lib.create_device)(node.as_raw_fd()) };
        let raw = NonNull::new(raw)
            .ok_or_else(|| format!("gbm_create_device: {}", io::Error::last_os_error()))?;
        Ok(Device(Rc::new(DeviceInner {
            raw,
            lib,
            _node: node,
        })))
    }

    /// The `gbm_device *`, for `eglGetPlatformDisplay`.
    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.0.raw.as_ptr().cast()
    }

    /// `gbm_bo_create`: a buffer laid out by `flags`, not by a modifier.
    pub(crate) fn create_buffer(
        &self,
        width: u32,
        height: u32,
        fourcc: u32,
        flags: u32,
    ) -> io::Result<BufferObject> {
        let d = &self.0;
        // SAFETY: a live device.
        let raw = unsafe { (d.lib.bo_create)(d.raw.as_ptr(), width, height, fourcc, flags) };
        self.wrap(raw)
    }

    /// `gbm_bo_create_with_modifiers2`: laid out by the first of
    /// `modifiers` the driver can produce.
    pub(crate) fn create_buffer_with_modifiers(
        &self,
        width: u32,
        height: u32,
        fourcc: u32,
        modifiers: &[u64],
        flags: u32,
    ) -> io::Result<BufferObject> {
        let d = &self.0;
        // SAFETY: a live device; the pointer and count describe `modifiers`.
        let raw = unsafe {
            (d.lib.bo_create_with_modifiers2)(
                d.raw.as_ptr(),
                width,
                height,
                fourcc,
                modifiers.as_ptr(),
                modifiers.len() as u32,
                flags,
            )
        };
        self.wrap(raw)
    }

    fn wrap(&self, raw: *mut RawBo) -> io::Result<BufferObject> {
        let raw = NonNull::new(raw).ok_or_else(io::Error::last_os_error)?;
        Ok(BufferObject {
            raw,
            device: Rc::clone(&self.0),
        })
    }
}

/// A `gbm_bo`. Destroyed on drop.
pub(crate) struct BufferObject {
    raw: NonNull<RawBo>,
    device: Rc<DeviceInner>,
}

impl BufferObject {
    fn lib(&self) -> &Lib {
        &self.device.lib
    }

    /// The `gbm_bo *`, for `eglCreateImage`.
    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.raw.as_ptr().cast()
    }

    pub(crate) fn modifier(&self) -> u64 {
        // SAFETY: a live buffer.
        unsafe { (self.lib().bo_get_modifier)(self.raw.as_ptr()) }
    }

    pub(crate) fn plane_count(&self) -> i32 {
        // SAFETY: a live buffer.
        unsafe { (self.lib().bo_get_plane_count)(self.raw.as_ptr()) }
    }

    /// A new fd for `plane`'s dmabuf, owned by the caller.
    pub(crate) fn fd_for_plane(&self, plane: i32) -> io::Result<OwnedFd> {
        // SAFETY: a live buffer.
        let fd = unsafe { (self.lib().bo_get_fd_for_plane)(self.raw.as_ptr(), plane) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: GBM dup'd this fd for the caller; nothing else owns it.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    pub(crate) fn offset(&self, plane: i32) -> u32 {
        // SAFETY: a live buffer.
        unsafe { (self.lib().bo_get_offset)(self.raw.as_ptr(), plane) }
    }

    pub(crate) fn stride_for_plane(&self, plane: i32) -> u32 {
        // SAFETY: a live buffer.
        unsafe { (self.lib().bo_get_stride_for_plane)(self.raw.as_ptr(), plane) }
    }

    /// Maps a `width`×`height` region at (`x`, `y`) for reading and hands
    /// `f` its bytes: `height` rows of `stride` bytes, the last cut to
    /// `width * 4`. Unmapped before returning.
    pub(crate) fn read_map<T>(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        f: impl FnOnce(&[u8]) -> T,
    ) -> io::Result<T> {
        assert!(width > 0 && height > 0, "gbm: an empty map");
        let lib = self.lib();
        let mut stride = 0u32;
        let mut map_data = std::ptr::null_mut();
        // SAFETY: a live buffer; the out-pointers are valid locals.
        let ptr = unsafe {
            (lib.bo_map)(
                self.raw.as_ptr(),
                x,
                y,
                width,
                height,
                TRANSFER_READ,
                &mut stride,
                &mut map_data,
            )
        };
        if ptr.is_null() {
            return Err(io::Error::last_os_error());
        }
        let len = stride as usize * (height as usize - 1) + width as usize * 4;
        // SAFETY: GBM mapped `height` rows of `stride` bytes at `ptr`, and
        // the last row holds at least `width * 4`; unmapped only below.
        let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) };
        let out = f(bytes);
        // SAFETY: the map_data this map returned; `bytes` is dead.
        unsafe { (lib.bo_unmap)(self.raw.as_ptr(), map_data) };
        Ok(out)
    }
}

impl Drop for BufferObject {
    fn drop(&mut self) {
        // SAFETY: made by this device, which `self.device` keeps alive.
        unsafe { (self.lib().bo_destroy)(self.raw.as_ptr()) }
    }
}
