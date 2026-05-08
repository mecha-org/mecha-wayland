use io_uring::{IoUring, opcode, types};
use std::{any::Any, collections::HashMap, io};
use tracing::{event, trace};

use crate::channel::{Receiver, unbounded};

type OpTag = u16;
// ── Token encoding ────────────────────────────────────────────────────────────

/// Encode/decode the 64-bit `user_data` field on every SQE.
///
/// Layout: `[tag: u16][context: u48]`
///
/// - `tag`     — the `OpTag` discriminant identifying the operation type.
/// - `context` — operation-specific data (fd cast to u64, timer id, etc.).
// #[repr(u16)]
// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// pub enum OpTag {
//     Read = 0x0001,
//     Write = 0x0002,
//     Accept = 0x0003,
//     WlSend = 0x0004,
//     WlRecv = 0x0005,
//     Timer = 0x0007,
//     Wakeup = 0x00FF,
// }

// impl OpTag {
//     #[inline]
//     pub fn encode(self, id: u64) -> u64 {
//         ((self as u64) << 48) | (id & 0x0000_FFFF_FFFF_FFFF)
//     }

//     #[inline]
//     pub fn decode(token: u64) -> (u16, u64) {
//         ((token >> 48) as u16, token & 0x0000_FFFF_FFFF_FFFF)
//     }
// }

// impl From<u16> for OpTag {
//     fn from(value: u16) -> Self {
//         match value {
//             x if x == OpTag::Read as u16 => OpTag::Read,
//             x if x == OpTag::Write as u16 => OpTag::Write,
//             x if x == OpTag::Accept as u16 => OpTag::Accept,
//             x if x == OpTag::WlSend as u16 => OpTag::WlSend,
//             x if x == OpTag::WlRecv as u16 => OpTag::WlRecv,
//             x if x == OpTag::Timer as u16 => OpTag::Timer,
//             x if x == OpTag::Wakeup as u16 => OpTag::Wakeup,
//             _ => panic!("invalid tag value: {value:#06x}"),
//         }
//     }
// }

#[inline]
pub fn encode_io_op(tag: u16, id: u64) -> u64 {
    ((tag as u64) << 48) | (id & 0x0000_FFFF_FFFF_FFFF)
}

#[inline]
pub fn decode_io_op(token: u64) -> (u16, u64) {
    ((token >> 48) as u16, token & 0x0000_FFFF_FFFF_FFFF)
}

// The dispatcher consumer or produces below events on the event manager
// #[derive(Clone)]
// pub struct IoRequest(pub IoSubmit);

// #[derive(Clone)]
// pub struct IoResponse(pub IoRes);

#[derive(Clone)]
pub enum IoSubmit {
    Send {
        tag: u16,
        fd: i32,
        buf: *const u8,
        len: usize,
    },
    SendMsg {
        tag: u16,
        fd: i32,
        msg: *const libc::msghdr,
    },
    Recv {
        tag: u16,
        fd: i32,
        buf: *mut u8,
        len: usize,
    },
}

pub type Token = u64;
pub type CompletionResult = i32;

pub struct IoSubscription {
    receiver: Receiver<(Token, CompletionResult)>,
}

impl IoSubscription {
    pub fn try_recv(&self) -> Option<(Token, CompletionResult)> {
        self.receiver.try_recv().ok()
    }
}

type BoxedSender = Box<dyn Fn((Token, CompletionResult))>;

pub struct Ring {
    ring: IoUring,
    next_token: u64,
    senders: HashMap<OpTag, Vec<BoxedSender>>,
}

impl Ring {
    pub fn new() -> io::Result<Self> {
        let ring = IoUring::builder().setup_sqpoll(1000).build(256)?;
        let dispatcher = Self {
            ring,
            next_token: 1,
            senders: HashMap::new(),
        };
        Ok(dispatcher)
    }

    pub fn submit(&mut self, cmd: IoSubmit) -> io::Result<u64> {
        // make the opcodes dynamic based on operation type
        let token = match cmd {
            IoSubmit::Send { tag, .. } => self.alloc_token(tag),
            IoSubmit::SendMsg { tag, .. } => self.alloc_token(tag),
            IoSubmit::Recv { tag, .. } => self.alloc_token(tag),
        };

        let sqe = match cmd {
            IoSubmit::Send { fd, buf, len, .. } => {
                // SAFETY:
                // - `buf_ptr` points into `buf`, which is owned by the caller and
                //   must remain valid until the completion with `token` is returned from `wait`.
                // - `self` (and therefore the SQE) outlives the caller's `buf` because the
                //   caller must ensure the buffer remains valid until completion.
                opcode::Send::new(types::Fd(fd), buf, len as u32)
                    .flags(libc::MSG_NOSIGNAL as _)
                    .build()
                    .user_data(token)
            }
            IoSubmit::SendMsg { fd, msg, .. } => {
                // SAFETY:
                // - `msg` is a pointer to a valid `msghdr` that remains valid until the completion with `token` is returned from `wait`.
                // - `self` (and therefore the SQE) outlives the caller's `msg` because the
                //   caller must ensure the buffer remains valid until completion.
                opcode::SendMsg::new(types::Fd(fd), msg)
                    .flags(libc::MSG_NOSIGNAL as _)
                    .build()
                    .user_data(token)
            }
            IoSubmit::Recv { fd, buf, len, .. } => {
                // SAFETY:
                // - `buf_ptr` points into `buf`, which is owned by the caller and
                //   must remain valid until the completion with `token` is returned from `wait`.
                // - `self` (and therefore the SQE) outlives the caller's `buf` because the
                //   caller must ensure the buffer remains valid until completion.
                opcode::Recv::new(types::Fd(fd), buf, len as u32)
                    .flags(libc::MSG_NOSIGNAL as _)
                    .build()
                    .user_data(token)
            }
        };

        trace!(
            "SQE: submitted: tag={:#06x}, token={:#018x}",
            (token >> 48) as u16,
            token
        );

        // TODO: handle the case where the submission queue is full (currently this returns an error, but we could also consider waiting for space to become available)
        // TODO: What if user wants to push one or more submit entries at a time
        let _ = self.push(sqe)?;

        Ok(token)
    }

    fn push(&mut self, sqe: io_uring::squeue::Entry) -> io::Result<()> {
        // SAFETY: caller upholds buffer-lifetime contracts.
        unsafe {
            self.ring
                .submission()
                .push(&sqe)
                .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "submission queue full"))?;
        }
        self.ring.submit()?;
        Ok(())
    }

    /// This waits for at least `min` completions
    /// SAFETY: This can only be called by one caller at a time, and the caller must ensure that the returned completions are processed before calling `wait` again
    // to avoid missing completions or processing the same completion multiple times.
    pub fn wait(&mut self, min: usize) -> io::Result<()> {
        self.ring.submit_and_wait(min)?;
        Ok(())
    }

    pub fn wait_and_dispatch(&mut self, min: usize) -> io::Result<(usize)> {
        self.wait(min)?;
        self.dispatch()
    }

    pub fn dispatch(&mut self) -> io::Result<usize> {
        let mut cq = self.ring.completion();
        cq.sync();

        let cq_cnt = cq.len();

        for cqe in cq {
            let token = cqe.user_data();
            let result = cqe.result(); // negative errno on error, ≥0 on success
            let (tag, _) = decode_io_op(token);

            trace!(
                "CQE: completed: tag={:#06x}, token={:#018x}, result={}",
                tag, token, result
            );

            if let Some(senders) = self.senders.get(&tag) {
                for sender in senders {
                    sender((token, result));
                }
            }
        }
        Ok(cq_cnt)
    }

    /// Register a crossbeam Sender for a specific OpTag.
    /// Returns the paired Receiver for the consumer to hold.
    pub fn subscribe(&mut self, op_tag: OpTag) -> IoSubscription {
        let (tx, rx) = unbounded::<(Token, CompletionResult)>();

        let wrapped: BoxedSender = Box::new(move |event: (Token, CompletionResult)| {
            let _ = tx.send(event);
        });

        self.senders.entry(op_tag).or_default().push(wrapped);

        IoSubscription { receiver: rx }
    }

    /// Allocate a unique token for a read/write SQE.
    ///
    /// Allocate a unique token for a read/write SQE.
    ///
    /// The high 16 bits are the `OpTag`; the low 48 bits are an incrementing
    /// counter so each in-flight operation has a distinct token.
    #[inline]
    fn alloc_token(&mut self, tag: u16) -> u64 {
        let id = self.next_token & 0x0000_FFFF_FFFF_FFFF;
        self.next_token = self.next_token.wrapping_add(1);
        encode_io_op(tag, id)
    }

    // pub fn wait_for_cqe(&mut self) -> Result<()> {
    //     // handle timeout, currently this blocks indefinitely until a CQE is available. We can add a timeout here if needed.
    //     let cqe = self.ring.submit_and_wait(1)?;
    //     Ok(())
    // }

    // pub fn dispatch_cq_entries(&mut self) -> Result<()> {
    //     // print the output of the last read for now
    //     let cqes: Vec<cqueue::Entry> = self.ring.completion().take(256).collect();

    //     for cqe in cqes {
    //         let _ = self.dispatch_cqe(cqe)?;
    //     }

    //     Ok(())
    // }

    // pub fn dispatch_cqe(&mut self, cqe: cqueue::Entry) -> Result<()> {
    //     let token = cqe.user_data();
    //     let (tag, id) = OpTag::decode(token);
    //     let result = cqe.result(); // negative errno on error, ≥0 on success

    //     if result < 0 {
    //         println!(
    //             "CQE: read completed with error: fd={}, errno={}",
    //             id, -result
    //         );
    //     }

    //     let _ = match tag {
    //         t if t == OpTag::Wakeup as u16 => {
    //             // setup read on eventfd again for the next wakeup
    //             self.wakeup_on_event_fd()?;
    //             print!("Ring rewird to wake up on eventfd\n");
    //         }
    //         // t if t == OpTag::Read as u16 => {
    //         //     println!("CQE: read completed: fd={}, bytes={}", id, result);
    //         //     self.ev.publish(IoResponse(IoRes::IoRead {}));
    //         // }
    //         // t if t == OpTag::Write as u16 => {
    //         //     println!("CQE: write completed: fd={}, bytes={}", id, result);
    //         //     self.ev.publish(IoResponse(IoRes::IoWrite {}));
    //         // }
    //         _ => println!("CQE: unknown tag: {tag:#06x}"),
    //     };

    //     Ok(())
    // }

    // pub fn run(&mut self) -> Result<()> {
    //     // setup the initial read on the eventfd to wake up on events
    //     let _ = self.wakeup_on_event_fd()?;

    //     // // create subscription for IoRequestEvent, which is used to make dispatcher perform IO
    //     // let io_sub = self.ev.subscribe::<IoSubmit>();

    //     loop {
    //         // wait for completion
    //         self.wait_for_cqe()?;

    //         // TODO: dispatch the completion message
    //         self.dispatch_cq_entries()?;

    //         // loop {
    //         //     let _ = match io_sub.try_recv() {
    //         //         Some(cmd) => self.submit(cmd),
    //         //         None => break, // no more commands to process
    //         //     };
    //         // }
    //     }
    // }

    // fn wakeup_on_event_fd(&mut self) -> Result<()> {
    //     let token = self.alloc_token(OpTag::Wakeup);
    //     let buf_ptr = self.wakeup_buf.as_mut_ptr();

    //     let sqe = opcode::Read::new(types::Fd(self.event_fd), buf_ptr, 8)
    //         .build()
    //         .user_data(token);

    //     // SAFETY:
    //     // - `buf_ptr` points into `self.wakeup_buf`, which is `Pin<Box<…>>` and
    //     //   therefore never moves for the lifetime of `self`.
    //     // - 8 bytes matches the eventfd read size.
    //     // - `self` (and therefore `wakeup_buf`) outlives the SQE because the
    //     //   SQE is owned by the ring that is also owned by `self`.
    //     unsafe { self.ring.submission().push(&sqe) }.map_err(|_| Error::SubmissionQueueFull)?;
    //     self.ring.submit()?;
    //     Ok(())
    // }
}
