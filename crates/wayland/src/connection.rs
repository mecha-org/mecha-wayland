use io_runtime::ring::{CompletionResult, IoSubmit, IoSubscription, Ring, Token, decode_io_op};
use std::{
    collections::{HashMap, VecDeque},
    env,
    io::{self, ErrorKind},
    mem,
    ops::Sub,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::net::UnixStream,
    },
    path::{Path, PathBuf},
};
use tracing::{debug, info, trace};

type ObjectId = u32;
type OpCode = u16;
type Size = u16;

const WL_HEADER_SIZE: usize = 8;
// 16 bytes of payload (that remains fixed) and max 24 fds (assumption)
const CMSG_BUF_SIZE: usize = mem::size_of::<libc::cmsghdr>() + 24 * mem::size_of::<RawFd>();
const RECV_BUF_SIZE: usize = 4096;
const WL_SEND_OP_TAG: u16 = 0x0004;
const WL_RECV_OP_TAG: u16 = 0x0005;

#[derive(Debug)]
pub struct WlEvent {
    pub object_id: ObjectId,
    pub opcode: OpCode,
    pub args: Vec<u8>,
}

struct CmsgBlock {
    iov: Box<libc::iovec>,
    hdr: Box<libc::msghdr>,
    cmsg_buf: Box<Vec<u8>>,
}

struct SubmitEntry {
    /// Encoded Wayland message bytes.
    data: Vec<u8>,
    /// Optional pre-built ancillary data for sendmsg (None → use send).
    cmsg: Option<CmsgBlock>,
}

pub struct Connection {
    stream: UnixStream,
    // send state
    send_queue: HashMap<u64, SubmitEntry>,
    sends_pending: usize,
    send_sub: IoSubscription,

    // recv state
    recv_inflight_buf: Box<[u8; RECV_BUF_SIZE]>,
    recv_armed: bool,
    recv_sub: IoSubscription,
    recv_buf: Vec<u8>,

    /// File descriptors received
    pub pending_fds: VecDeque<OwnedFd>,

    // id counter
    next_id: u32,
}

impl Connection {
    pub fn connect(io: &mut Ring) -> io::Result<Self> {
        let path = resolve_socket_path()?;
        info!(path = ?path, "connecting to Wayland socket");
        Self::connect_to(&path, io)
    }

    pub fn connect_to(path: &Path, io: &mut Ring) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        let io_recv_sub = io.subscribe(WL_RECV_OP_TAG); // subscribe to IO completion for recv
        let io_send_sub = io.subscribe(WL_SEND_OP_TAG); // subscribe to IO completion for send
        let mut conn = Self {
            stream,
            send_queue: HashMap::new(),
            sends_pending: 0,
            send_sub: io_send_sub,
            recv_inflight_buf: Box::new([0u8; RECV_BUF_SIZE]),
            recv_armed: false,
            recv_buf: Vec::new(),
            recv_sub: io_recv_sub,
            pending_fds: VecDeque::new(),
            next_id: 2, // Wayland object IDs start at 1; we reserve 1 for the display.
        };

        // arm the initial recv
        conn.arm_persistent_recv(io)?;
        Ok(conn)
    }

    #[inline]
    pub fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }

    #[inline]
    pub fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        debug!(id, "allocated object id");
        id
    }

    // ---- Submission handling ---- //

    pub fn send(
        &mut self,
        io: &mut Ring,
        object_id: u32,
        opcode: u16,
        args: &[u8],
    ) -> io::Result<()> {
        let data = encode_wl_message(object_id, opcode, args);
        let sock_fd = self.as_raw_fd();

        // create new entry with Send (no fds)
        let entry = SubmitEntry { data, cmsg: None };

        let token = io.submit(IoSubmit::Send {
            fd: sock_fd,
            buf: entry.data.as_ptr(),
            len: entry.data.len(),
            tag: WL_SEND_OP_TAG,
        })?;

        self.send_queue.insert(token, entry); // store the entry so its buffer lives until completion
        self.sends_pending += 1;
        Ok(())
    }

    pub fn send_with_fds(
        &mut self,
        io: &mut Ring,
        object_id: u32,
        opcode: u16,
        args: &[u8],
        fds: Vec<OwnedFd>,
    ) -> io::Result<()> {
        let data = encode_wl_message(object_id, opcode, args);
        let raw_fds: Vec<RawFd> = fds.iter().map(|f| f.as_raw_fd()).collect();
        let cmsg = build_cmsg(&data, &raw_fds);
        let sock_fd = self.as_raw_fd();

        // create new entry with SendMsg (with fds)
        let entry = SubmitEntry {
            data,
            cmsg: Some(cmsg),
        };

        // push to queue, and read from queue
        let hdr = entry.cmsg.as_ref().unwrap().hdr.as_ref();
        let token = io.submit(IoSubmit::SendMsg {
            fd: sock_fd,
            msg: hdr,
            tag: WL_SEND_OP_TAG,
        })?;

        self.send_queue.insert(token, entry); // store the entry so its buffers live until completion
        self.sends_pending += 1;
        Ok(())
    }

    // ---- Completion Receive handling ---- //

    // add a persistent recv entry to the ring; we re-arm it after every completion
    pub fn arm_persistent_recv(&mut self, io: &mut Ring) -> io::Result<()> {
        if self.recv_armed {
            return Ok(());
        }
        let fd = self.as_raw_fd();
        let buf = self.recv_inflight_buf.as_mut_ptr();
        // SAFETY: recv_inflight_buf is Box-allocated; its address is stable.
        // We clear recv_inflight in handle_recv_completion before re-arming.
        let recv_entry = IoSubmit::Recv {
            fd,
            buf,
            len: RECV_BUF_SIZE as usize,
            tag: WL_RECV_OP_TAG,
        };
        let _ = io.submit(recv_entry)?;
        self.recv_armed = true;
        Ok(())
    }

    /// Receives (drains) completions from the ring and processes them, returning any Wayland events that were received.
    pub fn drain(&mut self, io: &mut Ring, len: usize) -> io::Result<()> {
        let mut idx = 0;

        while idx < len {
            // 1. Process send completions first, which frees up send buffers.
            let send_msg = self.send_sub.try_recv();
            if send_msg.is_some() {
                self.drain_send_completion(send_msg.unwrap())?;

                idx += 1;
            }

            // 2. Process recv message
            let recv_msg = self.recv_sub.try_recv();
            if recv_msg.is_some() {
                self.drain_recv_completion(recv_msg.unwrap())?;

                self.arm_persistent_recv(io)?;
                // Re-arm for the next chunk of data.

                idx += 1;
            }
        }

        Ok(())
    }

    fn drain_send_completion(&mut self, send_msg: (Token, CompletionResult)) -> io::Result<()> {
        let (token, result) = send_msg;
        if result < 0 {
            return Err(io::Error::from_raw_os_error(-result));
        }

        trace!("IO: send_msg: token={:#018x}, result={}", token, result);

        // Send completion means the buffer for this entry is now safe to drop.
        self.send_queue.remove(&token);
        self.sends_pending -= 1;

        Ok(())
    }

    fn drain_recv_completion(&mut self, recv_msg: (Token, CompletionResult)) -> io::Result<()> {
        let (token, result) = recv_msg;
        if result < 0 {
            return Err(io::Error::from_raw_os_error(-result));
        }

        trace!("IO: recv_msg: token={:#018x}, result={}", token, result);

        self.recv_armed = false; // we'll re-arm after processing this batch

        // move the result from in_flight_buf to recv_buf, and drain any ancillary fds with recvmsg
        self.process_recv_msg(result as usize)?;

        // TODO: decode Wayland events from recv_buf and publish them to the event manager
        self.decode_wl_events()?;
        Ok(())
    }

    fn process_recv_msg(&mut self, result_bytes: usize) -> io::Result<()> {
        // The bytes io_uring read are already in recv_stage[..n_uring_bytes].
        // Append them to recv_buf.
        self.recv_buf
            .extend_from_slice(&self.recv_inflight_buf[..result_bytes]);

        // Now drain any fds (and more data) that arrived with SCM_RIGHTS.
        // In practice fds are rare; this loop usually runs 0 times.
        let sock_fd = self.as_raw_fd();
        loop {
            let mut data = [0u8; RECV_BUF_SIZE];
            let mut cmsg_buf = [0u8; CMSG_BUF_SIZE];
            match recvmsg_nonblocking(sock_fd, &mut data, &mut cmsg_buf) {
                Ok((0, _)) => break, // no more data right now
                Ok((n, fds)) => {
                    self.recv_buf.extend_from_slice(&data[..n]);
                    for raw in fds {
                        // SAFETY: kernel allocated this fd for us via SCM_RIGHTS.
                        self.pending_fds
                            .push_back(unsafe { OwnedFd::from_raw_fd(raw) });
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn decode_wl_events(&mut self) -> io::Result<Vec<WlEvent>> {
        let mut out = Vec::new();
        loop {
            if self.recv_buf.len() < WL_HEADER_SIZE {
                break;
            }
            let (object_id, opcode, size) = {
                let b: [u8; 8] = self.recv_buf[..8].try_into().unwrap();
                decode_wl_header(&b)
            };
            let total = size as usize;
            if total < WL_HEADER_SIZE || self.recv_buf.len() < total {
                break;
            }

            let payload = self.recv_buf[WL_HEADER_SIZE..total].to_vec();
            self.recv_buf.drain(..total);

            out.push(WlEvent {
                object_id,
                opcode,
                args: payload,
            });

            debug!(
                "WL: event: object_id={}, opcode={}, args={:?}",
                object_id,
                opcode,
                out.last().unwrap().args
            );

            // let ev = decode_event(hdr.object_id, hdr.opcode, &payload, |id| {
            //     self.objs.get(id).map(|i| i.name().to_string())
            // });

            // if let Some(o) = self.handle_wl_event(ev)? {
            //     out.push(o);
            // }
        }
        Ok(out)
    }

    pub fn pop_fd(&mut self) -> io::Result<OwnedFd> {
        self.pending_fds
            .pop_front()
            .ok_or_else(|| io::Error::other("Error"))
    }

    // pub fn flush(&mut self, io: &mut Ring) -> io::Result<()> {
    //     while self.sends_pending > 0 {
    //         let completions = io.wait(1)?;
    //         for c in completions {
    //             let (tag, _) = decode_io_op(c.token);
    //             if tag == WL_SEND_OP_TAG {
    //                 if c.is_error() {
    //                     return Err(c.err());
    //                 }

    //                 self.sends_pending -= 1;

    //                 // The oldest entry's buffer is now safe to drop.
    //                 if !self.send_queue.is_empty() {
    //                     self.send_queue.remove(0);
    //                 }
    //             }
    //         }
    //     }
    //     Ok(())
    // }

    // fn blocking_roundtrip(&mut self, io: &mut Ring) -> io::Result<Vec<WlEvent>> {
    //     let completions = io.wait(1)?;
    //     let mut wl_events = Vec::new();
    //     for c in completions {
    //         let (tag, _) = decode_io_op(c.token);
    //         match tag {
    //             IO_OP_WL_RECV_TAG => {
    //                 let n = c.check()?;
    //                 self.handle_recv_completion(io, n)?;
    //                 let wl_events = self.drain_protocol_events()?;

    //                 // let outcomes = self.drain_protocol_events()?;
    //                 // for o in outcomes {
    //                 //     if let Outcome::SyncDone = o {
    //                 //         sync_done = true;
    //                 //     }
    //                 // }
    //             }
    //             WL_SEND_OP_TAG => {
    //                 // nothing to do here
    //                 c.check()?;
    //             }
    //             _ => {}
    //         }
    //     }
    //     Ok(wl_events)
    // }

    // pub fn handle_recv_completion(&mut self, io: &mut Ring, n_bytes: usize) -> io::Result<()> {
    //     self.recv_armed = false;

    //     if n_bytes == 0 {
    //         return Err(io::Error::new(
    //             ErrorKind::BrokenPipe,
    //             "compositor closed connection",
    //         ));
    //     }

    //     // IORING_OP_RECV does not deliver ancillary data (cmsg). To receive
    //     // SCM_RIGHTS fds we need to drain the socket with recvmsg after the
    //     // RECV op signals the fd is readable. We use MSG_DONTWAIT so it never
    //     // blocks; io_uring already confirmed data is present.
    //     self.drain_with_recvmsg(n_bytes)?;

    //     // Re-arm for the next chunk of data.
    //     self.arm_persistent_recv(io)?;
    //     Ok(())
    // }
}

fn build_cmsg(data: &[u8], fds: &[RawFd]) -> CmsgBlock {
    let fd_bytes = unsafe {
        std::slice::from_raw_parts(fds.as_ptr() as *const u8, fds.len() * size_of::<RawFd>())
    };
    let cmsg_space = unsafe { libc::CMSG_SPACE(fd_bytes.len() as u32) as usize };
    let mut cmsg_buf: Box<Vec<u8>> = Box::new(vec![0u8; cmsg_space]);

    // iov points at `data`; caller must keep data alive alongside this block.
    let mut iov: Box<libc::iovec> = Box::new(libc::iovec {
        iov_base: data.as_ptr() as *mut _,
        iov_len: data.len(),
    });

    let hdr: Box<libc::msghdr> = Box::new(libc::msghdr {
        msg_name: std::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: iov.as_mut() as *mut _,
        msg_iovlen: 1,
        msg_control: cmsg_buf.as_mut_ptr() as *mut _,
        msg_controllen: cmsg_space,
        msg_flags: 0,
    });

    // Fill in the cmsg header and fd payload.
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(hdr.as_ref());
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(fd_bytes.len() as u32) as _;
        std::ptr::copy_nonoverlapping(fd_bytes.as_ptr(), libc::CMSG_DATA(cmsg), fd_bytes.len());
    }

    CmsgBlock { iov, cmsg_buf, hdr }
}

fn recvmsg_nonblocking(
    fd: RawFd,
    buf: &mut [u8],
    cmsg_buf: &mut [u8],
) -> io::Result<(usize, Vec<RawFd>)> {
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr() as *mut _,
        iov_len: buf.len(),
    };
    let mut hdr = libc::msghdr {
        msg_name: std::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: &mut iov,
        msg_iovlen: 1,
        msg_control: cmsg_buf.as_mut_ptr() as *mut _,
        msg_controllen: cmsg_buf.len(),
        msg_flags: 0,
    };

    let n = unsafe { libc::recvmsg(fd, &mut hdr, libc::MSG_DONTWAIT) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }

    let mut fds = Vec::new();
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&hdr);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let payload = libc::CMSG_DATA(cmsg);
                let len = (*cmsg).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                for i in 0..(len / size_of::<RawFd>()) {
                    fds.push((payload as *const RawFd).add(i).read_unaligned());
                }
            }
            cmsg = libc::CMSG_NXTHDR(&hdr, cmsg);
        }
    }

    Ok((n as usize, fds))
}

fn resolve_socket_path() -> io::Result<PathBuf> {
    let runtime_dir = env::var("XDG_RUNTIME_DIR")
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR not set"))?;
    let display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());

    if display.starts_with('/') {
        Ok(PathBuf::from(display))
    } else {
        let mut path = PathBuf::from(runtime_dir);
        path.push(display);
        Ok(path)
    }
}

fn encode_wl_message(object_id: u32, opcode: u16, args: &[u8]) -> Vec<u8> {
    let payload_len = args.len();
    // Total size must be 4-byte aligned
    let total = (WL_HEADER_SIZE + payload_len + 3) & !3;
    let mut buf = Vec::with_capacity(total);

    buf.extend_from_slice(&object_id.to_ne_bytes());
    let size_op = ((total as u32) << 16) | (opcode as u32);
    buf.extend_from_slice(&size_op.to_ne_bytes());
    buf.extend_from_slice(&args);
    // Pad to 4-byte boundary
    while buf.len() < total {
        buf.push(0);
    }
    buf
}

fn decode_wl_header(buf: &[u8; 8]) -> (ObjectId, OpCode, Size) {
    let object_id = u32::from_ne_bytes(buf[0..4].try_into().unwrap());
    let size_op = u32::from_ne_bytes(buf[4..8].try_into().unwrap());

    (object_id, (size_op & 0xffff) as u16, (size_op >> 16) as u16)
}
