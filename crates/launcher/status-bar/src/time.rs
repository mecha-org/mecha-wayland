use std::time::Duration;
use timer::{Absolute, Timer, TimerEvent, TimerId};

pub enum Precision {
    Minutes,
    Seconds,
}

pub fn local_time() -> (u32, u32, u32, u32, u32) {
    // SAFETY: time(NULL) is well-defined POSIX.
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: libc::tm is #[repr(C)], zero-init is valid.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: now and tm are valid pointers; null return handled below.
    if unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
        return (0, 0, 0, 1, 0);
    }
    (
        tm.tm_hour as u32,
        tm.tm_min as u32,
        tm.tm_sec as u32,
        tm.tm_mday as u32,
        tm.tm_mon as u32,
    )
}

pub fn next_deadline(precision: Precision) -> Duration {
    // SAFETY: time(NULL) is well-defined POSIX.
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    Duration::from_secs(match precision {
        Precision::Minutes => ((now / 60) + 1) * 60,
        Precision::Seconds => now + 1,
    } as u64)
}

pub fn arm_clock(timer: &mut Timer, id: &mut Option<TimerId>, precision: Precision) {
    *id = Some(timer.start_deadline(Absolute {
        at: next_deadline(precision),
    }));
}

pub fn try_clock_tick(id: Option<TimerId>, ev: &TimerEvent) -> Option<(u32, u32, u32, u32, u32)> {
    if id != Some(ev.id()) {
        return None;
    }
    Some(local_time())
}
