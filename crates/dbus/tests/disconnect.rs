use app::prelude::*;
use dbus::fake::{FakeBus, UNIQUE};
use dbus::*;
use ring::prelude::*;

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";

#[derive(Default)]
struct Log {
    replies: Vec<MethodReply>,
    gone: usize,
    kept: Vec<IncomingCall>,
}
impl Resource for Log {}

fn on_reply(app: &mut App, e: &MethodReply) {
    app.resource_mut::<Log>().replies.push(e.clone());
}
fn on_gone(app: &mut App, _: &BusDisconnected) {
    app.resource_mut::<Log>().gone += 1;
}
fn on_call(app: &mut App, e: &MethodCallReceived) {
    app.resource_mut::<Log>().kept.push(e.call.clone());
}

fn fake() -> FakeBus {
    let mut f = FakeBus::new();
    f.app
        .init_resource::<Log>()
        .system(on_reply)
        .system(on_gone)
        .system(on_call);
    f.received();
    f
}

fn call(app: &mut App) -> CallId {
    app.resource_mut::<DBus>()
        .session()
        .object(NM, NM_PATH)
        .call(NM, "GetDevices", &())
}

/// A tick and a `BeforeWait`, without blocking: a dead bus has no read
/// armed, so a `Ring::turn` could wait forever.
fn round_without_waiting(app: &mut App) {
    app.tick();
    app.signal(BeforeWait);
    app.flush();
}

#[test]
fn a_hang_up_fails_pending_calls_and_signals_once() {
    let mut f = fake();
    let pending = call(&mut f.app);
    f.turn();
    let mut app = f.hang_up();
    Ring::turn(&mut app);
    {
        let log = app.resource::<Log>();
        assert_eq!(log.gone, 1);
        assert_eq!(log.replies.len(), 1);
        assert_eq!(log.replies[0].call, pending);
        assert!(matches!(
            log.replies[0].result,
            Err(CallError::Disconnected)
        ));
    }
    assert!(!app.resource_mut::<DBus>().session().is_connected());

    let later = call(&mut app);
    round_without_waiting(&mut app);
    let log = app.resource::<Log>();
    assert_eq!(log.gone, 1, "raised once");
    assert_eq!(log.replies.len(), 2);
    assert_eq!(log.replies[1].call, later);
    assert!(matches!(
        log.replies[1].result,
        Err(CallError::Disconnected)
    ));
}

#[test]
fn replying_after_the_bus_died_does_nothing() {
    let mut f = fake();
    f.app
        .resource_mut::<DBus>()
        .session()
        .export(
            "/mecha/Test",
            InterfaceSpec::new("mecha.Test").method("Nothing", "", ""),
        )
        .unwrap();
    f.send(
        Message::method_call("/mecha/Test", "Nothing")
            .with_destination(UNIQUE)
            .with_sender(":1.9")
            .with_interface("mecha.Test"),
    );
    f.turn();
    let mut app = f.hang_up();
    Ring::turn(&mut app);
    let kept = app.resource_mut::<Log>().kept.pop().unwrap();
    app.resource_mut::<DBus>().reply(&kept, &());
    drop(kept);
    round_without_waiting(&mut app);
    assert_eq!(app.resource::<Log>().gone, 1);
}

#[test]
fn a_malformed_message_drops_the_connection() {
    let mut f = fake();
    f.write_raw(&[b'X'; 16]);
    Ring::turn(&mut f.app);
    assert_eq!(f.app.resource::<Log>().gone, 1);
    assert!(f.peer_closed(), "the socket is shut down on death");
}

#[test]
fn a_message_declaring_missing_fds_drops_the_connection() {
    let mut f = fake();
    let (r, _w) = std::io::pipe().unwrap();
    let with_fd = Message::signal("/a", "a.b", "C")
        .with_sender(":1.9")
        .with_body(&(UnixFd::new(r.into()),));
    let (bytes, _fds) = with_fd.encode(77);
    f.write_raw(&bytes);
    Ring::turn(&mut f.app);
    assert_eq!(f.app.resource::<Log>().gone, 1);
}

#[test]
fn an_oversized_message_drops_the_connection() {
    let mut f = fake();
    let mut header = b"l\x02\x00\x01".to_vec();
    header.extend_from_slice(&(200u32 << 20).to_le_bytes());
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    f.write_raw(&header);
    Ring::turn(&mut f.app);
    assert_eq!(f.app.resource::<Log>().gone, 1);
}
