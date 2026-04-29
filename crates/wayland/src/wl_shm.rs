use std::io;
use std::os::unix::io::OwnedFd;

use crate::object::Object;
use crate::{WlShm, WlShmFormatEvent, WlShmHandler};

pub struct ShmHandler {
    pub inner: WlShm,
}

impl Object for ShmHandler {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl WlShmHandler for ShmHandler {
    fn on_format(&mut self, event: WlShmFormatEvent) {
        tracing::debug!(format = event.format, "wl_shm::format");
    }
}

pub fn alloc_shm_file(size: usize, pixel: [u8; 4]) -> io::Result<OwnedFd> {
    use std::io::Write;
    let path = format!("/tmp/wl-shm-{}", std::process::id());
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;
    std::fs::remove_file(&path)?;
    file.set_len(size as u64)?;
    // Fill with repeated pixel
    let mut buf = Vec::with_capacity(size);
    while buf.len() < size {
        buf.extend_from_slice(&pixel);
    }
    buf.truncate(size);
    file.write_all(&buf)?;
    Ok(file.into())
}
