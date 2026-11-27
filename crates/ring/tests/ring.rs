use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use app::prelude::*;
use ring::prelude::*;
use ring::{Completed, Ring};

/// Wait until every token completed, at most ten waits and at most five
/// seconds. `Ring` is not `Send`, so the waits cannot be moved to a
/// watched thread; the deadline bounds a wait that keeps returning the
/// wrong completions, which is what a regression here looks like.
fn wait_for(ring: &mut Ring, tokens: &[ring::Token]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = Vec::new();
    for _ in 0..10 {
        for e in ring.wait() {
            seen.push(e.token);
        }
        if tokens.iter().all(|t| seen.contains(t)) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "ops did not complete within five seconds: seen {seen:?}"
        );
    }
    panic!("ops did not complete: seen {seen:?}");
}

/// The `flags:` line of `/proc/self/fdinfo/<fd>`, which the kernel fills
/// with the file's status flags plus `O_CLOEXEC` when the descriptor is
/// close-on-exec. Octal, as `fdinfo` prints it.
fn fd_flags(fd: std::os::fd::RawFd) -> u32 {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{fd}")).unwrap();
    let line = info
        .lines()
        .find_map(|l| l.strip_prefix("flags:"))
        .expect("fdinfo has a flags line");
    u32::from_str_radix(line.trim(), 8).expect("the flags are octal")
}

#[test]
fn sendmsg_and_recvmsg_move_bytes_and_fds() {
    let (a, b) = UnixStream::pair().unwrap();
    let (mut reader, writer) = std::io::pipe().unwrap();
    let mut ring = Ring::new(8);

    let sent = ring.sendmsg(a.as_fd(), b"hello".to_vec(), vec![OwnedFd::from(writer)]);
    let recv = ring.recvmsg(b.as_fd(), Vec::with_capacity(64), 4);
    wait_for(&mut ring, &[sent, recv]);

    // The send op owns its fds until finished; drop them first, or the
    // pipe never reaches EOF below.
    let Completed { buf, fds } = ring.finish(sent).unwrap();
    assert_eq!(buf, b"hello", "the send buffer comes back for reuse");
    assert_eq!(fds.len(), 1, "the sent fds come back to be dropped");
    drop(fds);

    let Completed { buf, fds } = ring.finish(recv).unwrap();
    assert_eq!(buf, b"hello");
    assert_eq!(fds.len(), 1);
    let mut got = std::io::PipeWriter::from(fds.into_iter().next().unwrap());
    got.write_all(b"via fd").unwrap();
    drop(got);
    let mut s = String::new();
    reader.read_to_string(&mut s).unwrap();
    assert_eq!(s, "via fd");
}

#[test]
fn a_received_fd_comes_back_close_on_exec() {
    let (a, b) = UnixStream::pair().unwrap();
    let (_reader, writer) = std::io::pipe().unwrap();
    let mut ring = Ring::new(8);

    let sent = ring.sendmsg(a.as_fd(), b"x".to_vec(), vec![OwnedFd::from(writer)]);
    let recv = ring.recvmsg(b.as_fd(), Vec::with_capacity(8), 4);
    wait_for(&mut ring, &[sent, recv]);
    drop(ring.finish(sent).unwrap().fds);

    let Completed { fds, .. } = ring.finish(recv).unwrap();
    let got = fds.into_iter().next().expect("the fd came through");
    // `MSG_CMSG_CLOEXEC`: a compositor fd must not survive an exec.
    const O_CLOEXEC: u32 = 0o2000000;
    assert_eq!(
        fd_flags(got.as_raw_fd()) & O_CLOEXEC,
        O_CLOEXEC,
        "a received fd is close-on-exec"
    );
}

#[test]
fn recvmsg_with_no_fd_room_gets_the_bytes_only() {
    let (a, b) = UnixStream::pair().unwrap();
    let (_reader, writer) = std::io::pipe().unwrap();
    let mut ring = Ring::new(8);
    let sent = ring.sendmsg(a.as_fd(), b"x".to_vec(), vec![OwnedFd::from(writer)]);
    let recv = ring.recvmsg(b.as_fd(), Vec::with_capacity(8), 0);
    wait_for(&mut ring, &[sent, recv]);
    let Completed { buf, fds } = ring.finish(recv).unwrap();
    assert_eq!(buf, b"x");
    assert!(fds.is_empty());
}

#[test]
fn finish_is_once_and_only_for_own_tokens() {
    let (a, _b) = UnixStream::pair().unwrap();
    let mut ring = Ring::new(8);
    let sent = ring.sendmsg(a.as_fd(), b"x".to_vec(), vec![]);
    wait_for(&mut ring, &[sent]);
    assert!(ring.finish(sent).is_some());
    assert!(ring.finish(sent).is_none());
    assert!(ring.finish(ring::Token::from_raw(999)).is_none());
}

#[test]
fn dropping_a_ring_with_a_pending_recv_returns_instead_of_hanging() {
    // `Ring` holds raw pointers (it is not `Send`), so the ring itself is
    // built and dropped inside the watched thread; only the completion
    // signal crosses the thread boundary.
    let (done, waited) = mpsc::channel();
    std::thread::spawn(move || {
        let (_a, b) = UnixStream::pair().unwrap();
        let mut ring = Ring::new(8);
        // Armed but never completed: nothing is ever sent on `_a`.
        // Room for fds, so the cancelled recv's zeroed control buffer is
        // walked by `Drop for Op` on the way out.
        let _recv = ring.recvmsg(b.as_fd(), Vec::with_capacity(64), 4);
        drop(ring);
        let _ = done.send(());
    });
    waited
        .recv_timeout(Duration::from_secs(5))
        .expect("dropping the ring hung with a recv still in flight");
}

#[test]
fn dropping_a_ring_before_finishing_a_completed_recv_closes_both_copies_of_the_fd() {
    let (a, b) = UnixStream::pair().unwrap();
    let (mut reader, writer) = std::io::pipe().unwrap();
    let mut ring = Ring::new(8);

    let sent = ring.sendmsg(a.as_fd(), b"hello".to_vec(), vec![OwnedFd::from(writer)]);
    let recv = ring.recvmsg(b.as_fd(), Vec::with_capacity(64), 4);
    wait_for(&mut ring, &[sent, recv]);

    // Neither op is finished: the sent fd is still held by the send op,
    // and the received fd is still undecoded in the recv op's control
    // buffer. Dropping the ring must close both on its own.
    drop(ring);

    let mut s = String::new();
    reader.read_to_string(&mut s).unwrap();
    assert_eq!(s, "");
}

#[derive(Default)]
struct Log(Vec<IoEvent>);
impl Resource for Log {}

fn log(app: &mut App, e: &IoEvent) {
    app.resource_mut::<Log>().0.push(*e);
}

#[test]
fn turn_signals_before_wait_then_one_io_event_per_completion() {
    let (a, _b) = UnixStream::pair().unwrap();
    let mut app = App::new();
    app.add_module(RingModule::default())
        .init_resource::<Log>()
        .system(log);
    let token = app
        .resource_mut::<Ring>()
        .sendmsg(a.as_fd(), b"x".to_vec(), vec![]);
    Ring::turn(&mut app);
    let events = &app.resource::<Log>().0;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].token, token);
    assert_eq!(events[0].result, 1);
}

#[test]
fn one_turn_delivers_every_completion_it_finds() {
    let (a, _b) = UnixStream::pair().unwrap();
    let mut app = App::new();
    app.add_module(RingModule::default())
        .init_resource::<Log>()
        .system(log);
    // Both armed before the turn: one wait collects both completions and
    // signals one `IoEvent` each, in completion order.
    let first = app
        .resource_mut::<Ring>()
        .sendmsg(a.as_fd(), b"one".to_vec(), vec![]);
    let second = app
        .resource_mut::<Ring>()
        .sendmsg(a.as_fd(), b"two".to_vec(), vec![]);
    Ring::turn(&mut app);
    let seen: Vec<_> = app.resource::<Log>().0.iter().map(|e| e.token).collect();
    assert!(seen.contains(&first), "the first send: seen {seen:?}");
    assert!(seen.contains(&second), "the second send: seen {seen:?}");
}

/// Work left that only an app's own check sees.
struct Busy;
impl Resource for Busy {}

/// An app whose [`App::has_pending_work`] always holds.
fn busy_app() -> App {
    let mut app = App::new();
    app.add_module(RingModule::default())
        .init_resource::<Log>()
        .system(log)
        .pending_check(|app| app.has_resource::<Busy>());
    app.insert_resource(Busy);
    app
}

#[test]
fn a_turn_with_work_left_does_not_block() {
    // No op is armed, so a blocking turn would wait forever. Built on a
    // watched thread, as the app is not `Send`.
    let (done, waited) = mpsc::channel();
    std::thread::spawn(move || {
        let mut app = busy_app();
        Ring::turn(&mut app);
        let _ = done.send(());
    });
    waited
        .recv_timeout(Duration::from_secs(5))
        .expect("the turn returned without blocking");
}

#[test]
fn a_turn_with_work_left_still_delivers_ready_io() {
    // Work left never starves I/O: what is ready comes in on the turns
    // that don't block.
    const POLLIN: u32 = 0x1;
    let (a, mut b) = UnixStream::pair().unwrap();
    b.write_all(b"x").unwrap();
    let mut app = busy_app();
    let token = app.resource_mut::<Ring>().poll(a.as_fd(), POLLIN);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.resource::<Log>().0.iter().any(|e| e.token == token) {
        assert!(Instant::now() < deadline, "the ready poll never came in");
        Ring::turn(&mut app);
    }
    app.resource_mut::<Ring>().finish(token);
}

fn stop_on_first_wait(app: &mut App, _: &BeforeWait) {
    app.signal(Stop);
}

#[test]
fn stop_ends_the_runner_after_the_turn() {
    let (a, _b) = UnixStream::pair().unwrap();
    let mut app = App::new();
    app.add_module(RingModule::default())
        .system(stop_on_first_wait);
    app.resource_mut::<Ring>()
        .sendmsg(a.as_fd(), b"x".to_vec(), vec![]);
    app.run();
}

fn stop_on_tick(app: &mut App, _: &Tick) {
    app.signal(Stop);
}

#[test]
fn stop_raised_by_a_tick_ends_the_runner_without_waiting() {
    // No op is armed, so the turn after the tick would block forever;
    // the runner has to notice the stop before it gets there. The app
    // holds a `Ring` and is not `Send`, so it is built on the watched
    // thread and only the "it returned" signal crosses back.
    let (done, waited) = mpsc::channel();
    std::thread::spawn(move || {
        let mut app = App::new();
        app.add_module(RingModule::default()).system(stop_on_tick);
        app.run();
        let _ = done.send(());
    });
    waited
        .recv_timeout(Duration::from_secs(5))
        .expect("a Stop raised by a tick left the runner blocked in the turn");
}

#[test]
fn a_timeout_completes_with_etime_after_its_duration() {
    let mut ring = Ring::new(8);
    let start = Instant::now();
    let t = ring.timeout(Duration::from_millis(20));
    let events = ring.wait();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].token, t);
    assert_eq!(events[0].result, -libc::ETIME);
    assert!(start.elapsed() >= Duration::from_millis(20));
    let done = ring.finish(t).unwrap();
    assert!(done.buf.is_empty() && done.fds.is_empty());
}

#[test]
fn a_shorter_timeout_submitted_later_completes_first() {
    let mut ring = Ring::new(8);
    let long = ring.timeout(Duration::from_millis(300));
    let short = ring.timeout(Duration::from_millis(10));
    let first = ring.wait();
    assert_eq!(first[0].token, short);
    assert!(first.iter().all(|e| e.token != long));
}

#[test]
fn dropping_the_ring_does_not_wait_out_a_pending_timeout() {
    let mut ring = Ring::new(8);
    ring.timeout(Duration::from_secs(30));
    let start = Instant::now();
    drop(ring);
    assert!(start.elapsed() < Duration::from_secs(1));
}

/// A pipe with a non-blocking read end, like an inotify fd.
fn non_blocking_pipe() -> (std::io::PipeReader, std::io::PipeWriter) {
    let (r, w) = std::io::pipe().unwrap();
    // SAFETY: `r` is an open fd for the whole call.
    let set = unsafe { libc::fcntl(r.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
    assert_eq!(set, 0);
    (r, w)
}

#[test]
fn a_read_on_a_non_blocking_fd_waits_for_data() {
    let (r, mut w) = non_blocking_pipe();
    let mut ring = Ring::new(8);
    let read = ring.read(r.as_fd(), Vec::with_capacity(64));
    let timer = ring.timeout(Duration::from_millis(20));
    let first = ring.wait();
    assert_eq!(first[0].token, timer, "the read completed empty: {first:?}");

    w.write_all(b"hi").unwrap();
    wait_for(&mut ring, &[read]);
    let done = ring.finish(read).unwrap();
    assert_eq!(done.buf, b"hi");
}

#[test]
fn a_read_takes_at_most_the_capacity_and_leaves_the_rest() {
    let (r, mut w) = non_blocking_pipe();
    w.write_all(b"abcdef").unwrap();
    let mut ring = Ring::new(8);
    let first = ring.read(r.as_fd(), Vec::with_capacity(4));
    wait_for(&mut ring, &[first]);
    assert_eq!(ring.finish(first).unwrap().buf, b"abcd");
    let rest = ring.read(r.as_fd(), Vec::with_capacity(4));
    wait_for(&mut ring, &[rest]);
    assert_eq!(ring.finish(rest).unwrap().buf, b"ef");
}
