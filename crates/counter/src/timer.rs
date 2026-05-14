use crate::ring::{IoEvent, SharedRingProxy};
use app::event::Event;
use io_uring::{opcode, types};
use std::collections::HashMap;

pub struct TimerSettings {
    pub duration: std::time::Duration,
}

#[derive(Clone, Copy, Debug)]
pub enum TimerEvents {
    TimerFinished { id: u64 },
}

impl Event for TimerEvents {}

pub struct Timer {
    proxy: SharedRingProxy,
    active: HashMap<u64, Box<types::Timespec>>,
}

impl Timer {
    pub fn new(proxy: SharedRingProxy) -> Self {
        Self {
            proxy,
            active: HashMap::new(),
        }
    }

    pub fn start_timer(&mut self, settings: TimerSettings) -> u64 {
        let ts = Box::new(
            types::Timespec::new()
                .sec(settings.duration.as_secs())
                .nsec(settings.duration.subsec_nanos()),
        );

        let sqe = opcode::Timeout::new(&*ts as *const _).build();

        let token = self.proxy.borrow_mut().push(sqe);

        self.active.insert(token, ts);

        token
    }

    pub fn try_finish(&mut self, event: &IoEvent) -> Option<TimerEvents> {
        match event {
            IoEvent::Complete { token, result } => {
                if self.active.remove(token).is_some() {
                    // Note: io_uring timeouts usually return -ETIME (-62) on success.
                    // If result == 0, it means the timer was canceled.
                    let is_timeout = *result == -libc::ETIME || *result == 0;

                    if is_timeout {
                        return Some(TimerEvents::TimerFinished { id: *token });
                    }
                }
                None
            }
        }
    }
}

#[macro_export]
macro_rules! register_timer {
    () => {
        app::module::Module::<crate::timer::Timer>::new().processor(
            |timer: &mut crate::timer::Timer, event: &crate::ring::IoEvent| timer.try_finish(event),
        )
    };
}
