use app::Event;
use io_ring::{IoEvent, IoToken, RingProxy};
use io_uring::{opcode, types};
use std::{collections::HashMap, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerId(pub u64);

pub struct TimerSettings {
    pub duration: Duration,
    pub repeat: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum TimerEvent {
    Finished { id: TimerId },
    Restarted { id: TimerId },
}

impl Event for TimerEvent {}

struct ActiveTimer {
    id: TimerId,
    settings: TimerSettings,
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

    pub fn start_timer(&mut self, settings: TimerSettings) -> TimerId {
        let id = TimerId(self.next_timer_id);
        self.next_timer_id += 1;

        self.submit_timer(id, settings);
        id
    }

    fn submit_timer(&mut self, id: TimerId, settings: TimerSettings) -> IoToken {
        let ts = Box::new(
            types::Timespec::new()
                .sec(settings.duration.as_secs())
                .nsec(settings.duration.subsec_nanos()),
        );

        let sqe = opcode::Timeout::new(&*ts as *const _).build();
        let token = self.ring_proxy.push(sqe);

        self.active_by_token
            .insert(token, ActiveTimer { id, settings, ts });
        token
    }

    pub fn try_finish(&mut self, event: &IoEvent) -> Option<TimerEvent> {
        match event {
            IoEvent::Completed { token, result } => {
                if let Some(active) = self.active_by_token.remove(token) {
                    // io_uring timeouts return -ETIME.
                    // result == 0 usually indicates the timer was explicitly canceled.
                    let is_timeout = *result == -libc::ETIME || *result == 0;

                    if is_timeout {
                        if active.settings.repeat {
                            self.submit_timer(active.id, active.settings);
                            return Some(TimerEvent::Restarted { id: active.id });
                        } else {
                            return Some(TimerEvent::Finished { id: active.id });
                        }
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
