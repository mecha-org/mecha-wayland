use std::os::fd::{FromRawFd, OwnedFd, RawFd};

/// Control-buffer size for `SCM_RIGHTS` ancillary data. Comfortably above the
/// D-Bus per-message fd limit (default 16); `MSG_CTRUNC` guards overflow.
pub(crate) const CMSG_BUF: usize = 512;

/// Aligned control-message buffer
#[repr(C, align(8))]
pub(crate) struct CmsgBuf(pub(crate) [u8; CMSG_BUF]);

impl CmsgBuf {
    pub(crate) fn zeroed() -> Self {
        CmsgBuf([0u8; CMSG_BUF])
    }
}

/// The stable-address FFI trio driving one `RecvMsg`/`SendMsg` direction: the
/// iovec, the aligned control buffer, and the msghdr tying them together. Boxed
/// once per direction in the connection; the SAFETY story is simply "this box
/// never moves while an op is in flight".
pub(crate) struct MsgBuffers {
    pub(crate) iov: libc::iovec,
    pub(crate) cmsg: CmsgBuf,
    pub(crate) hdr: libc::msghdr,
}

impl MsgBuffers {
    pub(crate) fn zeroed() -> Box<Self> {
        // SAFETY: zeroed iovec/msghdr are valid empty descriptors; every field
        // is (re)filled before each submission.
        Box::new(MsgBuffers {
            iov: unsafe { std::mem::zeroed() },
            cmsg: CmsgBuf::zeroed(),
            hdr: unsafe { std::mem::zeroed() },
        })
    }

    /// Point `hdr` at `iov`/`cmsg` for a payload of `len` bytes at `base`, with
    /// `controllen` bytes of control data (0 = none).
    pub(crate) fn wire(&mut self, base: *mut libc::c_void, len: usize, controllen: usize) {
        self.iov.iov_base = base;
        self.iov.iov_len = len;
        self.hdr.msg_name = std::ptr::null_mut();
        self.hdr.msg_namelen = 0;
        self.hdr.msg_iov = &mut self.iov;
        self.hdr.msg_iovlen = 1;
        self.hdr.msg_control = if controllen > 0 {
            self.cmsg.0.as_mut_ptr() as *mut libc::c_void
        } else {
            std::ptr::null_mut()
        };
        self.hdr.msg_controllen = controllen as _;
        self.hdr.msg_flags = 0;
    }
}

pub(crate) const MAX_SEND_FDS: usize = 64;

/// dup(2) a raw fd into an owned copy we control for the duration of a send.
pub(crate) fn dup_owned(raw: RawFd) -> Option<OwnedFd> {
    // SAFETY: dup on a valid fd returns a fresh owned descriptor.
    let d = unsafe { libc::dup(raw) };
    if d < 0 {
        return None;
    }
    // SAFETY: dup succeeded, so `d` is a fresh owned descriptor.
    Some(unsafe { OwnedFd::from_raw_fd(d) })
}

/// Write an `SCM_RIGHTS` control message for `fds` into `buf`; returns the used
/// control length (0 if no fds).
///
/// SAFETY: `buf` must hold at least `CMSG_SPACE(fds.len() * 4)` bytes.
pub(crate) unsafe fn build_scm_rights(buf: &mut [u8], fds: &[RawFd]) -> usize {
    if fds.is_empty() {
        return 0;
    }
    let payload = std::mem::size_of_val(fds);

    unsafe {
        let space = libc::CMSG_SPACE(payload as u32) as usize;
        assert!(space <= buf.len(), "too many fds for cmsg buffer");
        for b in &mut buf[..space] {
            *b = 0;
        }
        let cmsg = buf.as_mut_ptr() as *mut libc::cmsghdr;
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(payload as u32) as _;
        let data = libc::CMSG_DATA(cmsg) as *mut RawFd;
        for (i, fd) in fds.iter().enumerate() {
            std::ptr::write_unaligned(data.add(i), *fd);
        }
        space
    }
}

/// Collect owned fds from any `SCM_RIGHTS` control messages on a completed recv.
///
/// SAFETY: `hdr` must be a msghdr populated by a completed `recvmsg`.
pub(crate) unsafe fn parse_scm_rights(hdr: &libc::msghdr) -> Vec<OwnedFd> {
    let mut fds = Vec::new();
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(hdr as *const libc::msghdr);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let payload = (*cmsg).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                let count = payload / std::mem::size_of::<RawFd>();
                let data = libc::CMSG_DATA(cmsg) as *const RawFd;
                for i in 0..count {
                    let raw = std::ptr::read_unaligned(data.add(i));
                    fds.push(OwnedFd::from_raw_fd(raw));
                }
            }
            cmsg = libc::CMSG_NXTHDR(hdr as *const libc::msghdr, cmsg);
        }
        fds
    }
}

/// Parse the `UNIX_FDS` header field (field code 9) from a complete message
/// frame, returning how many file descriptors the message carries (0 if none).
///
/// The header fields are the `a(yv)` array beginning at byte 16; each entry is a
/// (code: y, value: v) pair, 8-byte aligned. Only the value types that appear in
/// D-Bus header fields are handled: `u` (u32), `s`/`o` (u32-length string), and
/// `g` (byte-length signature).
pub(crate) fn dbus_unix_fd_count(frame: &[u8]) -> u32 {
    if frame.len() < 16 {
        return 0;
    }
    let le = frame[0] == b'l';
    let rd_u32 = |o: usize| -> u32 {
        let b = [frame[o], frame[o + 1], frame[o + 2], frame[o + 3]];
        if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    let fields_len = rd_u32(12) as usize;
    let start = 16usize;
    let end = start + fields_len;
    if end > frame.len() {
        return 0;
    }
    let align = |o: usize, n: usize| (o + n - 1) & !(n - 1);

    let mut cur = start;
    while cur < end {
        cur = align(cur, 8); // struct array elements are 8-aligned
        if cur >= end {
            break;
        }
        let code = frame[cur];
        cur += 1;
        // variant signature: 1-byte length, `len` chars, trailing nul
        if cur >= end {
            break;
        }
        let sig_len = frame[cur] as usize;
        cur += 1;
        if cur + sig_len + 1 > end {
            break;
        }
        let sig0 = frame[cur];
        cur += sig_len + 1; // skip signature chars + nul
        match sig0 {
            b'u' => {
                cur = align(cur, 4);
                if cur + 4 > end {
                    break;
                }
                let val = rd_u32(cur);
                cur += 4;
                if code == 9 {
                    return val; // UNIX_FDS
                }
            }
            b's' | b'o' => {
                cur = align(cur, 4);
                if cur + 4 > end {
                    break;
                }
                let slen = rd_u32(cur) as usize;
                cur += 4 + slen + 1;
            }
            b'g' => {
                if cur >= end {
                    break;
                }
                let glen = frame[cur] as usize;
                cur += 1 + glen + 1;
            }
            _ => break, // unexpected header-field type
        }
    }
    0
}
