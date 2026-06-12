use app::Event;
use io_ring::{IoEvent, IoToken, RingProxy};
use io_uring::{opcode, types};
use std::{collections::HashMap, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerId(pub u64);

#[derive(Clone, Copy, Debug)]
pub struct Relative {
    pub duration: Duration,
    pub repeat: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Absolute {
    pub at: Duration,
    pub clock: Clock,
}

#[derive(Clone, Copy, Debug)]
pub enum Clock {
    Monotonic,
    Realtime,
}

#[derive(Debug, Clone, Copy)]
pub enum TimerEvent {
    Finished { id: TimerId },
    Restarted { id: TimerId },
}

impl TimerEvent {
    pub fn id(&self) -> TimerId {
        match self {
            TimerEvent::Finished { id } | TimerEvent::Restarted { id } => *id,
        }
    }
}

impl Event for TimerEvent {}

enum Timing {
    Relative(Relative),
    Absolute(Absolute),
}

struct ActiveTimer {
    id: TimerId,
    timing: Timing,
    ts: Box<types::Timespec>,
}

pub struct Timer {
    ring_proxy: RingProxy,
    active_by_token: HashMap<IoToken, ActiveTimer>,
    next_timer_id: u64,
}

impl Timer {
    pub fn new(ring_proxy: RingProxy) -> Self {
        Self {
            ring_proxy,
            active_by_token: HashMap::new(),
            next_timer_id: 1,
        }
    }

    fn next_id(&mut self) -> TimerId {
        let id = TimerId(self.next_timer_id);
        self.next_timer_id += 1;
        id
    }

    pub fn start_timer(&mut self, relative: Relative) -> TimerId {
        let id = self.next_id();
        self.submit(id, Timing::Relative(relative));
        id
    }

    pub fn start_deadline(&mut self, absolute: Absolute) -> TimerId {
        let id = self.next_id();
        self.submit(id, Timing::Absolute(absolute));
        id
    }

    fn submit(&mut self, id: TimerId, timing: Timing) -> IoToken {
        let (duration, flags) = match timing {
            Timing::Relative(s) => (s.duration, types::TimeoutFlags::empty()),
            Timing::Absolute(d) => {
                let clock_flag = match d.clock {
                    Clock::Monotonic => types::TimeoutFlags::ABS,
                    Clock::Realtime => types::TimeoutFlags::ABS | types::TimeoutFlags::REALTIME,
                };
                (d.at, clock_flag)
            }
        };

        let ts = Box::new(
            types::Timespec::new()
                .sec(duration.as_secs())
                .nsec(duration.subsec_nanos()),
        );

        let sqe = opcode::Timeout::new(&*ts as *const _).flags(flags).build();
        let token = self.ring_proxy.push(sqe);

        self.active_by_token
            .insert(token, ActiveTimer { id, timing, ts });
        token
    }

    pub fn try_finish(&mut self, event: &IoEvent) -> Option<TimerEvent> {
        match event {
            IoEvent::Completed { token, result } => {
                if let Some(active) = self.active_by_token.remove(token) {
                    let is_timeout = *result == -libc::ETIME || *result == 0;

                    if is_timeout {
                        if let Timing::Relative(relative) = &active.timing {
                            if relative.repeat {
                                self.submit(active.id, active.timing);
                                return Some(TimerEvent::Restarted { id: active.id });
                            }
                        }
                        return Some(TimerEvent::Finished { id: active.id });
                    }
                }
                None
            }
        }
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<Timer, AppState> {
    app::Module::<Timer, _, _>::new()
        .on(|timer: &mut Timer, event: &io_ring::IoEvent| timer.try_finish(event))
}
