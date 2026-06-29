use app::Event;
use io_uring::{IoUring, squeue::Entry};
use std::{cell::RefCell, collections::VecDeque, io, rc::Rc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IoToken(pub u64);

#[derive(Clone, Debug)]
pub struct RingSettings {
    pub entries: u32,
    pub always_no_wait: bool,
}

impl Default for RingSettings {
    fn default() -> Self {
        Self {
            entries: 256,
            always_no_wait: false,
        }
    }
}

#[derive(Debug)]
pub enum IoEvent {
    Completed { token: IoToken, result: i32 },
}

impl Event for IoEvent {}

pub struct RingData {
    pending: VecDeque<Entry>,
    next_token: u64,
    always_no_wait: bool,
    skip_next_wait: bool,
}

impl RingData {
    fn new(always_no_wait: bool) -> Self {
        Self {
            pending: VecDeque::new(),
            next_token: 1,
            always_no_wait,
            skip_next_wait: false,
        }
    }

    fn push(&mut self, sqe: Entry) -> IoToken {
        let token = self.next_token;
        self.next_token += 1;
        self.pending.push_back(sqe.user_data(token));
        IoToken(token)
    }
}

#[derive(Clone)]
pub struct RingProxy(Rc<RefCell<RingData>>);

impl RingProxy {
    pub fn push(&self, sqe: Entry) -> IoToken {
        self.0.borrow_mut().push(sqe)
    }

    /// Prevents the ring from blocking the thread permanently
    pub fn set_always_no_wait(&self, no_wait: bool) {
        self.0.borrow_mut().always_no_wait = no_wait;
    }

    /// Prevents the ring from blocking the thread only on the very next poll
    pub fn skip_next_wait(&self) {
        self.0.borrow_mut().skip_next_wait = true;
    }
}

pub struct Ring {
    ring: IoUring,
    data: Rc<RefCell<RingData>>,
}

impl Default for Ring {
    fn default() -> Self {
        Self::new(RingSettings::default())
    }
}

impl Ring {
    pub fn new(settings: RingSettings) -> Self {
        Self {
            ring: IoUring::new(settings.entries).expect("failed to create io_uring"),
            data: Rc::new(RefCell::new(RingData::new(settings.always_no_wait))),
        }
    }

    pub fn proxy(&self) -> RingProxy {
        RingProxy(Rc::clone(&self.data))
    }

    pub fn poll(&mut self) -> Vec<IoEvent> {
        let mut data = self.data.borrow_mut();

        if !data.pending.is_empty() {
            let mut sq = self.ring.submission();
            while !sq.is_full() {
                if let Some(sqe) = data.pending.pop_front() {
                    unsafe {
                        let _ = sq.push(&sqe);
                    }
                } else {
                    break;
                }
            }
            sq.sync();
        }

        let always_no_wait = data.always_no_wait;
        let skip_next_wait = data.skip_next_wait;
        data.skip_next_wait = false;
        drop(data);

        let wait_for = if always_no_wait || skip_next_wait {
            0
        } else {
            1
        };

        if let Err(e) = self.ring.submit_and_wait(wait_for) {
            eprintln!("Ring flush error: {}", e);
            return Vec::new();
        }

        let mut cq = self.ring.completion();
        cq.sync();
        cq.map(|cqe| IoEvent::Completed {
            token: IoToken(cqe.user_data()),
            result: cqe.result(),
        })
        .collect()
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<Ring, AppState> {
    app::Module::<Ring, _, _>::new()
        .on(|ring: &mut Ring, _: &app::Poll| app::Many(ring.poll().into_iter()))
}
