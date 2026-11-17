//! One in-flight operation: the buffers, the `iovec`, the `msghdr` and
//! the control buffer, boxed so the pointers the kernel holds stay put.
//! The only unsafe in the workspace lives here.
#![allow(unsafe_code)]

use std::io;
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::ptr;
use std::time::Duration;

use io_uring::{opcode, squeue, types};

use crate::Completed;

pub(crate) enum Kind {
    Recv,
    Read,
    Send,
    Timeout,
    /// Readiness interests belong only to poll operations.
    Poll {
        events: u32,
    },
    /// target is the userdata of the call we want to cancel
    AsyncCancel {
        target: u64,
    },
}

pub(crate) struct Op {
    kind: Kind,
    buf: Vec<u8>,
    fds: Vec<OwnedFd>,
    raw_fds: Vec<RawFd>,
    iov: libc::iovec,
    msg: libc::msghdr,
    cmsg: Vec<u8>,
    /// A timeout's duration. The kernel reads it after submission; the
    /// box keeps it put until the completion.
    ts: types::Timespec,
    pub(crate) result: Option<i32>,
}

fn cmsg_space(fds: usize) -> usize {
    // SAFETY: a pure size computation.
    unsafe { libc::CMSG_SPACE((fds * mem::size_of::<RawFd>()) as u32) as usize }
}

impl Op {
    /// A receive into `buf`'s whole capacity, with control room for
    /// `max_fds` passed fds (none if zero).
    pub(crate) fn recv(mut buf: Vec<u8>, max_fds: usize) -> Box<Op> {
        buf.clear();
        let cmsg = if max_fds == 0 {
            Vec::new()
        } else {
            // Must stay zeroed: `take_fds` relies on a zero `cmsg_len`
            // terminating the walk. If control buffers are ever pooled
            // and reused, zero them here rather than handing one back
            // with a previous completion's headers still in it.
            vec![0u8; cmsg_space(max_fds)]
        };
        // SAFETY: iovec and msghdr are plain C structs; all-zero is valid.
        let mut op = Box::new(Op {
            kind: Kind::Recv,
            buf,
            fds: Vec::new(),
            raw_fds: Vec::new(),
            iov: unsafe { mem::zeroed() },
            msg: unsafe { mem::zeroed() },
            cmsg,
            ts: types::Timespec::new(),
            result: None,
        });
        op.iov.iov_base = op.buf.as_mut_ptr().cast();
        op.iov.iov_len = op.buf.capacity();
        op.msg.msg_iov = &raw mut op.iov;
        op.msg.msg_iovlen = 1;
        if !op.cmsg.is_empty() {
            op.msg.msg_control = op.cmsg.as_mut_ptr().cast();
            op.msg.msg_controllen = op.cmsg.len() as _;
        }
        op
    }

    /// A read into `buf`'s whole capacity, for fds that are not sockets.
    pub(crate) fn read(mut buf: Vec<u8>) -> Box<Op> {
        buf.clear();
        // SAFETY: as in `recv`.
        Box::new(Op {
            kind: Kind::Read,
            buf,
            fds: Vec::new(),
            raw_fds: Vec::new(),
            iov: unsafe { mem::zeroed() },
            msg: unsafe { mem::zeroed() },
            cmsg: Vec::new(),
            ts: types::Timespec::new(),
            result: None,
        })
    }

    /// A send of `buf` with `fds` as `SCM_RIGHTS`.
    pub(crate) fn send(buf: Vec<u8>, fds: Vec<OwnedFd>) -> Box<Op> {
        let raw_fds: Vec<RawFd> = fds.iter().map(|f| f.as_raw_fd()).collect();
        let cmsg = if raw_fds.is_empty() {
            Vec::new()
        } else {
            vec![0u8; cmsg_space(raw_fds.len())]
        };
        // SAFETY: as in `recv`.
        let mut op = Box::new(Op {
            kind: Kind::Send,
            buf,
            fds,
            raw_fds,
            iov: unsafe { mem::zeroed() },
            msg: unsafe { mem::zeroed() },
            cmsg,
            ts: types::Timespec::new(),
            result: None,
        });
        op.iov.iov_base = op.buf.as_mut_ptr().cast();
        op.iov.iov_len = op.buf.len();
        op.msg.msg_iov = &raw mut op.iov;
        op.msg.msg_iovlen = 1;
        if !op.cmsg.is_empty() {
            op.msg.msg_control = op.cmsg.as_mut_ptr().cast();
            op.msg.msg_controllen = op.cmsg.len() as _;
            let bytes = (op.raw_fds.len() * mem::size_of::<RawFd>()) as u32;
            // SAFETY: `cmsg` was sized by `CMSG_SPACE` for exactly these fds,
            // and `msg_control` points into it.
            unsafe {
                let c = libc::CMSG_FIRSTHDR(&op.msg);
                (*c).cmsg_level = libc::SOL_SOCKET;
                (*c).cmsg_type = libc::SCM_RIGHTS;
                (*c).cmsg_len = libc::CMSG_LEN(bytes) as _;
                ptr::copy_nonoverlapping(
                    op.raw_fds.as_ptr(),
                    libc::CMSG_DATA(c).cast::<RawFd>(),
                    op.raw_fds.len(),
                );
            }
        }
        op
    }

    /// A timeout that completes after `after` with `-ETIME`.
    pub(crate) fn timeout(after: Duration) -> Box<Op> {
        // SAFETY: as in `recv`.
        Box::new(Op {
            kind: Kind::Timeout,
            buf: Vec::new(),
            fds: Vec::new(),
            raw_fds: Vec::new(),
            iov: unsafe { mem::zeroed() },
            msg: unsafe { mem::zeroed() },
            cmsg: Vec::new(),
            ts: types::Timespec::new()
                .sec(after.as_secs())
                .nsec(after.subsec_nanos()),
            result: None,
        })
    }

    /// A one-shot readiness poll. Successful completions carry readiness flags.
    pub(crate) fn poll(events: u32) -> Box<Op> {
        // SAFETY: all-zero is valid for iovec and msghdr.
        // Neither structure is used by this operation.
        Box::new(Op {
            kind: Kind::Poll { events },
            buf: Vec::new(),
            fds: Vec::new(),
            raw_fds: Vec::new(),
            iov: unsafe { mem::zeroed() },
            msg: unsafe { mem::zeroed() },
            cmsg: Vec::new(),
            ts: types::Timespec::new(),
            result: None,
        })
    }

    pub(crate) fn cancel(target: u64) -> Box<Op> {
        Box::new(Op {
            kind: Kind::AsyncCancel { target },
            buf: Vec::new(),
            fds: Vec::new(),
            raw_fds: Vec::new(),
            iov: unsafe { mem::zeroed() },
            msg: unsafe { mem::zeroed() },
            cmsg: Vec::new(),
            ts: types::Timespec::new(),
            result: None,
        })
    }

    /// The submission entry. Valid while this box lives, which is until
    /// `complete`.
    ///
    /// A receive asks for `MSG_CMSG_CLOEXEC` so passed fds are not
    /// inherited across an exec; a send asks for `MSG_NOSIGNAL` so a
    /// hung-up peer is an `EPIPE` result rather than a `SIGPIPE` that
    /// kills the process.
    pub(crate) fn entry(&mut self, fd: RawFd) -> squeue::Entry {
        match self.kind {
            Kind::Recv => opcode::RecvMsg::new(types::Fd(fd), &mut self.msg)
                .flags(libc::MSG_CMSG_CLOEXEC as u32)
                .build(),
            // `u64::MAX` reads from the current position, not at an offset.
            Kind::Read => {
                let len = self.buf.capacity().min(u32::MAX as usize) as u32;
                opcode::Read::new(types::Fd(fd), self.buf.as_mut_ptr(), len)
                    .offset(u64::MAX)
                    .build()
            }
            Kind::Send => opcode::SendMsg::new(types::Fd(fd), &self.msg)
                .flags(libc::MSG_NOSIGNAL as u32)
                .build(),
            Kind::Timeout => opcode::Timeout::new(&self.ts).build(),
            Kind::Poll { events } => opcode::PollAdd::new(types::Fd(fd), events).build(),
            Kind::AsyncCancel { target } => opcode::AsyncCancel::new(target).build(),
        }
    }

    /// After the completion: a receive or read is truncated to the bytes received
    /// and a receive's fds decoded; a send gives its buffers back untouched.
    pub(crate) fn complete(mut self: Box<Self>) -> Completed {
        if let Kind::Recv | Kind::Read = self.kind {
            let n = self.result.unwrap_or(0).max(0) as usize;
            // SAFETY: the kernel wrote `n` bytes into the capacity, `n` is
            // at most the capacity `iov_len` offered.
            unsafe { self.buf.set_len(n.min(self.buf.capacity())) };
        }
        if let Kind::Recv = self.kind {
            // SAFETY: `msg_control` and `msg_controllen` are what the kernel
            // filled; each fd found was passed to us and is owned once.
            self.fds = unsafe { take_fds(&self.msg) };
            // The control buffer's fds are spoken for now; zero the length
            // so `Drop` (which runs right after this method returns) does
            // not decode the same raw fd numbers a second time.
            self.msg.msg_controllen = 0;
        }
        Completed {
            buf: mem::take(&mut self.buf),
            fds: mem::take(&mut self.fds),
        }
    }
}

impl Drop for Op {
    /// A completed receive that was never `finish`ed still has its fds
    /// installed in our fd table by the kernel; take them and drop them
    /// here so they are not leaked. `complete` already zeroes
    /// `msg_controllen` once it has taken the real fds, so this only ever
    /// finds something to close when `complete` was never called.
    fn drop(&mut self) {
        if let Kind::Recv = self.kind {
            if self.result.is_some() {
                // SAFETY: `msg_control`/`msg_controllen` are either the
                // kernel's completed values (not yet consumed) or zeroed
                // by `complete`, in which case this finds nothing.
                drop(unsafe { take_fds(&self.msg) });
            }
        }
    }
}

/// Decode the `SCM_RIGHTS` fds in `msg`'s control buffer, taking
/// ownership of each.
///
/// Termination relies on the control buffer being zero-initialised.
/// io_uring never writes `msg_controllen` back, so it still holds the
/// whole buffer's length rather than the bytes the kernel filled, and
/// the walk runs past the last real header into the untouched tail;
/// libc's linux-gnu `CMSG_NXTHDR` returns null on a zero `cmsg_len`, so
/// a buffer that started out zeroed ends the walk there. The data
/// length is clamped to the bytes remaining in the buffer as well, so a
/// nonsense `cmsg_len` cannot read past the end.
unsafe fn take_fds(msg: &libc::msghdr) -> Vec<OwnedFd> {
    let mut out = Vec::new();
    if msg.msg_control.is_null() {
        return out;
    }
    let base = msg.msg_control as usize;
    let end = base + msg.msg_controllen;
    unsafe {
        let mut c = libc::CMSG_FIRSTHDR(msg);
        while !c.is_null() {
            if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                let header = libc::CMSG_LEN(0) as usize;
                let len = ((*c).cmsg_len as usize).min(end.saturating_sub(c as usize));
                let data = len.saturating_sub(header);
                let p = libc::CMSG_DATA(c).cast::<RawFd>();
                for i in 0..data / mem::size_of::<RawFd>() {
                    out.push(OwnedFd::from_raw_fd(*p.add(i)));
                }
            }
            c = libc::CMSG_NXTHDR(msg, c);
        }
    }
    out
}

/// Push one entry, submitting first if the queue is full.
///
/// # Panics
///
/// If the kernel refuses the submission that makes room. Callers that
/// must not panic (the `Drop` path) use [`try_push`] instead.
pub(crate) fn push(uring: &mut io_uring::IoUring, entry: squeue::Entry) {
    try_push(uring, entry).expect("io_uring submit");
}

/// [`push`], fallible. Bounded: one push, one submit to drain the queue,
/// one push. A successful `submit` hands the whole submission queue to
/// the kernel, so the second push has the entire queue to itself and can
/// only fail if the queue has no slots at all.
pub(crate) fn try_push(uring: &mut io_uring::IoUring, entry: squeue::Entry) -> io::Result<()> {
    // SAFETY: the entry's pointers live in a boxed `Op` kept by the
    // ring until its completion is finished.
    if unsafe { uring.submission().push(&entry) }.is_ok() {
        return Ok(());
    }
    uring.submit()?;
    // SAFETY: as above.
    unsafe { uring.submission().push(&entry) }
        .map_err(|_| io::Error::other("io_uring submission queue full after a submit"))
}

/// The submission entry that asks the kernel to cancel the op submitted
/// with `target` as its `user_data`. Unlike `entry`, this holds no
/// pointer into an `Op`, so it needs no unsafe to build.
pub(crate) fn cancel_entry(target: u64) -> squeue::Entry {
    opcode::AsyncCancel::new(target).build().user_data(0)
}
