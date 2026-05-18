use app::event::Event;
use io_uring::{IoUring, squeue::Entry};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;

#[derive(Clone)]
pub struct SharedRingProxy(Rc<RefCell<RingProxy>>);

impl SharedRingProxy {
    pub fn push(&self, sqe: Entry) -> u64 {
        self.0.borrow_mut().push(sqe)
    }

    pub fn set_no_wait(&self, no_wait: bool) {
        self.0.borrow_mut().set_no_wait(no_wait);
    }
}

#[derive(Clone, Copy, Debug)]
pub enum IoEvent {
    Completed { token: u64, result: i32 },
}

impl Event for IoEvent {}

pub struct RingProxy {
    pending: VecDeque<Entry>,
    next_token: u64,
    no_wait: bool,
}

impl RingProxy {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            next_token: 1,
            no_wait: false,
        }
    }

    pub fn push(&mut self, sqe: Entry) -> u64 {
        let token = self.next_token;
        self.next_token += 1;
        self.pending.push_back(sqe.user_data(token));
        token
    }

    /// Prevent the ring from blocking the thread on the next poll.
    pub fn set_no_wait(&mut self, no_wait: bool) {
        self.no_wait = no_wait;
    }

    pub fn no_wait(&self) -> bool {
        self.no_wait
    }
}

pub struct Ring {
    ring: IoUring,
    ready: VecDeque<IoEvent>,
    proxy: SharedRingProxy,
}

impl Default for Ring {
    fn default() -> Self {
        Self {
            ring: IoUring::new(256).expect("failed to create io_uring"),
            ready: VecDeque::new(),
            proxy: SharedRingProxy(Rc::new(RefCell::new(RingProxy::new()))),
        }
    }
}

impl Ring {
    pub fn get_proxy(&self) -> SharedRingProxy {
        SharedRingProxy(Rc::clone(&self.proxy.0))
    }

    pub fn poll_one(&mut self, min_wait: usize) -> Option<IoEvent> {
        let has_pending = !self.proxy.0.borrow().pending.is_empty();
        if !has_pending && !self.ready.is_empty() {
            return self.ready.pop_front();
        }
        if let Err(e) = self.flush_and_fill(min_wait) {
            eprintln!("Ring flush error: {}", e);
            return None;
        }
        self.ready.pop_front()
    }

    fn flush_and_fill(&mut self, min_wait: usize) -> io::Result<()> {
        let mut proxy = self.proxy.0.borrow_mut();

        if !proxy.pending.is_empty() {
            let mut sq = self.ring.submission();
            for sqe in proxy.pending.drain(..) {
                unsafe {
                    sq.push(&sqe).map_err(|_| {
                        io::Error::new(io::ErrorKind::WouldBlock, "submission queue full")
                    })?;
                }
            }
        }

        let no_wait = proxy.no_wait;
        drop(proxy);

        let wait_for = if self.ready.is_empty() && !no_wait {
            min_wait
        } else {
            0
        };
        self.ring.submit_and_wait(wait_for)?;

        let mut cq = self.ring.completion();
        cq.sync();
        while let Some(cqe) = cq.next() {
            self.ready.push_back(IoEvent::Completed {
                token: cqe.user_data(),
                result: cqe.result(),
            });
        }

        Ok(())
    }
}

#[macro_export]
macro_rules! register_ring {
    ($min_wait:expr) => {
        app::module::Module::<crate::ring::Ring>::new()
            .processor(move |ring: &mut crate::ring::Ring, _: &app::Poll| ring.poll_one($min_wait))
    };
}
