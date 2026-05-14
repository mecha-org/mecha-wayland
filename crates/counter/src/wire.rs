use std::os::fd::RawFd;

pub const HEADER_SIZE: usize = 8;

// ============================================================
// MESSAGE HEADER
// ============================================================

#[derive(Debug, Clone, Copy)]
pub struct MessageHeader {
    pub sender_id: u32,
    pub opcode: u16,
    pub size: u16, // Total message size in bytes, including the 8-byte header
}

impl MessageHeader {
    /// Attempts to parse an 8-byte header from the start of the buffer.
    /// Returns None if the buffer doesn't have enough bytes.
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < HEADER_SIZE {
            return None;
        }

        let sender_id = u32::from_ne_bytes(buf[0..4].try_into().unwrap());
        let word2 = u32::from_ne_bytes(buf[4..8].try_into().unwrap());

        Some(Self {
            sender_id,
            opcode: (word2 & 0xFFFF) as u16,
            size: (word2 >> 16) as u16,
        })
    }
}

// ============================================================
// BUILDER (SERIALIZATION)
// ============================================================

pub struct MessageBuilder<'a> {
    buf: &'a mut Vec<u8>,
    fds: &'a mut Vec<RawFd>,

    header_start: usize,
    opcode: u16,
}

impl<'a> MessageBuilder<'a> {
    /// Reserves 8 bytes for the header which will be backfilled in `build()`.
    pub fn new(buf: &'a mut Vec<u8>, fds: &'a mut Vec<RawFd>, sender_id: u32, opcode: u16) -> Self {
        let header_start = buf.len();

        // Write sender ID and a placeholder for size/opcode
        buf.extend_from_slice(&sender_id.to_ne_bytes());
        buf.extend_from_slice(&[0u8; 4]);

        Self {
            buf,
            fds,
            header_start,
            opcode,
        }
    }

    pub fn write_u32(self, v: u32) -> Self {
        self.buf.extend_from_slice(&v.to_ne_bytes());
        self
    }

    pub fn write_i32(self, v: i32) -> Self {
        self.buf.extend_from_slice(&v.to_ne_bytes());
        self
    }

    /// Converts an f64 to Wayland's 24.8 signed fixed-point format
    pub fn write_fixed(self, v: f64) -> Self {
        self.write_i32((v * 256.0) as i32)
    }

    /// Length-prefixed, null-terminated, 4-byte-aligned UTF-8 string.
    pub fn write_string(self, s: &str) -> Self {
        let len = s.len() + 1; // +1 for the null terminator
        let pad = align4(len) - len;

        self.buf.extend_from_slice(&(len as u32).to_ne_bytes());
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0); // Null terminator

        // Pad to 32-bit boundary
        self.buf.extend(std::iter::repeat(0).take(pad));
        self
    }

    /// Length-prefixed raw byte array, 4-byte-aligned.
    pub fn write_array(self, a: &[u8]) -> Self {
        let len = a.len();
        let pad = align4(len) - len;

        self.buf.extend_from_slice(&(len as u32).to_ne_bytes());
        self.buf.extend_from_slice(a);

        // Pad to 32-bit boundary
        self.buf.extend(std::iter::repeat(0).take(pad));
        self
    }

    pub fn write_fd(self, fd: RawFd) -> Self {
        self.fds.push(fd);
        self
    }

    /// Finalizes the message. Backfills the 8-byte header with the calculated total size.
    pub fn build(self) {
        let total_size = self.buf.len() - self.header_start;
        let size_op = ((total_size as u32) << 16) | (self.opcode as u32);

        self.buf[self.header_start + 4..self.header_start + 8]
            .copy_from_slice(&size_op.to_ne_bytes());
    }
}

// ============================================================================
// READER (DESERIALIZATION)
// ============================================================================

/// Reads primitives and borrows strings/arrays directly from the receive buffer.
pub struct MessageReader<'a> {
    body: &'a [u8],
    offset: usize,
    fds: &'a mut Vec<RawFd>,
}

impl<'a> MessageReader<'a> {
    /// Creates a new reader from the message payload (excluding the 8-byte header).
    pub fn new(body: &'a [u8], fds: &'a mut Vec<RawFd>) -> Self {
        Self {
            body,
            offset: 0,
            fds,
        }
    }

    pub fn read_u32(&mut self) -> Option<u32> {
        if self.body.len() < self.offset + 4 {
            return None;
        }
        let val = u32::from_ne_bytes(self.body[self.offset..self.offset + 4].try_into().unwrap());
        self.offset += 4;
        Some(val)
    }

    pub fn read_i32(&mut self) -> Option<i32> {
        if self.body.len() < self.offset + 4 {
            return None;
        }
        let val = i32::from_ne_bytes(self.body[self.offset..self.offset + 4].try_into().unwrap());
        self.offset += 4;
        Some(val)
    }

    /// Converts Wayland's 24.8 signed fixed-point format back to an f64
    pub fn read_fixed(&mut self) -> Option<f64> {
        self.read_i32().map(|v| v as f64 / 256.0)
    }

    /// Returns a string slice borrowed directly from the buffer.
    pub fn read_string(&mut self) -> Option<&'a str> {
        let len = self.read_u32()? as usize;

        if len == 0 || self.body.len() < self.offset + len {
            return None;
        }

        // The length includes the null terminator.
        // We slice up to `len - 1` to exclude the null byte from the Rust string slice.
        let s_bytes = &self.body[self.offset..self.offset + len - 1];
        let s = std::str::from_utf8(s_bytes).ok()?;

        self.offset += align4(len); // Advance past string and padding
        Some(s)
    }

    /// Returns an array slice borrowed directly from the buffer.
    pub fn read_array(&mut self) -> Option<&'a [u8]> {
        let len = self.read_u32()? as usize;

        if self.body.len() < self.offset + len {
            return None;
        }

        let a = &self.body[self.offset..self.offset + len];
        self.offset += align4(len); // Advance past array and padding
        Some(a)
    }

    /// Safely pops the next File Descriptor off the queue.
    pub fn read_fd(&mut self) -> Option<RawFd> {
        if self.fds.is_empty() {
            None
        } else {
            Some(self.fds.remove(0))
        }
    }
}

// ============================================================
// UTILS
// ============================================================

/// Rounds up a length to the nearest multiple of 4 bytes.
#[inline(always)]
pub fn align4(n: usize) -> usize {
    (n + 3) & !3
}
