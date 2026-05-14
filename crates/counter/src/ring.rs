use app::event::Event;
use io_uring::{IoUring, squeue::Entry};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;

pub type SharedRingProxy = Rc<RefCell<RingProxy>>;

#[derive(Clone, Copy, Debug)]
pub enum IoEvent {
    Completed { token: u64, result: i32 },
}

impl Event for IoEvent {}

pub struct RingProxy {
    pending: VecDeque<Entry>,
    next_token: u64,
}

impl RingProxy {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            next_token: 1,
        }
    }

    pub fn push(&mut self, sqe: Entry) -> u64 {
        let token = self.next_token;
        self.next_token += 1;
        self.pending.push_back(sqe.user_data(token));
        token
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
            proxy: Rc::new(RefCell::new(RingProxy::new())),
        }
    }
}

impl Ring {
    pub fn get_proxy(&self) -> SharedRingProxy {
        Rc::clone(&self.proxy)
    }

    pub fn poll_one(&mut self, min_wait: usize) -> Option<IoEvent> {
        if let Err(e) = self.flush_and_fill(min_wait) {
            eprintln!("Ring flush error: {}", e);
            return None;
        }
        self.ready.pop_front()
    }

    fn flush_and_fill(&mut self, min_wait: usize) -> io::Result<()> {
        let mut proxy = self.proxy.borrow_mut();

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

        drop(proxy);

        let wait_for = if self.ready.is_empty() { min_wait } else { 0 };
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
