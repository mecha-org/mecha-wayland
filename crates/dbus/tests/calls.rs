use std::io::{Read, Write};
use std::time::{Duration, Instant};

use app::prelude::*;
use dbus::fake::{FakeBus, UNIQUE};
use dbus::*;
use ring::prelude::*;

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";

#[derive(Default)]
struct Replies(Vec<MethodReply>);
impl Resource for Replies {}

fn log(app: &mut App, e: &MethodReply) {
    app.resource_mut::<Replies>().0.push(e.clone());
}

fn with_log(mut f: FakeBus) -> FakeBus {
    f.app.init_resource::<Replies>().system(log);
    f.received();
    f
}

fn fake() -> FakeBus {
    with_log(FakeBus::new())
}

fn replies(f: &mut FakeBus) -> Vec<MethodReply> {
    std::mem::take(&mut f.app.resource_mut::<Replies>().0)
}

fn call_devices(f: &mut FakeBus) -> CallId {
    f.app
        .resource_mut::<DBus>()
        .session()
        .object(NM, NM_PATH)
        .call(NM, "GetDevices", &())
}

#[test]
fn a_call_gets_its_reply() {
    let mut f = fake();
    let id = call_devices(&mut f);
    f.turn();
    let call = f.expect_call("GetDevices");
    assert_eq!(call.serial(), id.serial);
    assert_eq!(call.destination().unwrap().as_str(), NM);
    assert_eq!(call.interface().unwrap().as_str(), NM);
    f.reply(&call, &(vec![ObjectPath::from("/dev/1")],));
    f.turn();
    let r = replies(&mut f);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].call, id);
    let (paths,): (Vec<ObjectPath>,) = r[0].result.as_ref().unwrap().body().unwrap();
    assert_eq!(paths, [ObjectPath::from("/dev/1")]);
}

#[test]
fn property_helpers_call_the_properties_interface() {
    let mut f = fake();
    {
        let mut dbus = f.app.resource_mut::<DBus>();
        let mut nm = dbus.session().object(NM, NM_PATH);
        nm.get_property(NM, "State");
        nm.get_all(NM);
        nm.set_property(NM, "WirelessEnabled", Value::Bool(false));
    }
    f.turn();
    let get = f.expect_call("Get");
    assert_eq!(
        get.interface().unwrap().as_str(),
        "org.freedesktop.DBus.Properties"
    );
    assert_eq!(
        get.body::<(String, String)>().unwrap(),
        (NM.to_string(), "State".to_string())
    );
    let all = f.expect_call("GetAll");
    assert_eq!(all.body::<(String,)>().unwrap().0, NM);
    let set = f.expect_call("Set");
    assert_eq!(set.signature(), "ssv");
    assert_eq!(
        set.body::<(String, String, Value)>().unwrap().2,
        Value::Bool(false)
    );
}

#[test]
fn an_error_reply_is_a_remote_error() {
    let mut f = fake();
    call_devices(&mut f);
    f.turn();
    let call = f.expect_call("GetDevices");
    f.error(
        &call,
        "org.freedesktop.DBus.Error.AccessDenied",
        "not allowed",
    );
    f.turn();
    let r = replies(&mut f);
    match &r[0].result {
        Err(e @ CallError::Remote { message, .. }) => {
            assert!(e.is("org.freedesktop.DBus.Error.AccessDenied"));
            assert_eq!(message, "not allowed");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_call_times_out_once_and_a_late_reply_is_dropped() {
    let mut f = fake();
    let start = Instant::now();
    let id = f
        .app
        .resource_mut::<DBus>()
        .session()
        .send(MethodCall::new(NM, NM_PATH, NM, "Slow").timeout(Duration::from_millis(30)));
    f.turn();
    let call = f.expect_call("Slow");
    let mut turns = 0;
    while f.app.resource::<Replies>().0.is_empty() {
        Ring::turn(&mut f.app);
        turns += 1;
        assert!(turns < 8, "the timeout never fired");
    }
    assert!(start.elapsed() >= Duration::from_millis(30));
    let r = replies(&mut f);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].call, id);
    assert!(matches!(r[0].result, Err(CallError::Timeout)));
    f.reply(&call, &());
    f.turn();
    assert!(replies(&mut f).is_empty(), "a late reply is dropped");
}

#[test]
fn a_cancelled_call_raises_nothing() {
    let mut f = fake();
    let id = call_devices(&mut f);
    f.app.resource_mut::<DBus>().session().cancel(id);
    f.turn();
    let call = f.expect_call("GetDevices");
    f.reply(&call, &());
    f.turn();
    assert!(replies(&mut f).is_empty());
}

#[test]
fn a_reply_split_across_reads_is_reassembled() {
    let mut f = fake();
    let id = call_devices(&mut f);
    f.turn();
    let call = f.expect_call("GetDevices");
    let big = "x".repeat(200_000);
    let bytes = f.encode(
        &Message::method_return(call.serial())
            .with_destination(UNIQUE)
            .with_sender(NM)
            .with_body(&(big.as_str(),)),
    );
    let (head, tail) = bytes.split_at(100);
    f.write_raw(head);
    Ring::turn(&mut f.app);
    assert!(
        f.app.resource::<Replies>().0.is_empty(),
        "half a message is carried"
    );
    f.write_raw(tail);
    let mut turns = 0;
    while f.app.resource::<Replies>().0.is_empty() {
        Ring::turn(&mut f.app);
        turns += 1;
        assert!(turns < 16, "the reply never completed");
    }
    let r = replies(&mut f);
    assert_eq!(r[0].call, id);
    let (s,): (String,) = r[0].result.as_ref().unwrap().body().unwrap();
    assert_eq!(s.len(), 200_000);
}

#[test]
fn several_replies_in_one_read_arrive_in_order() {
    let mut f = fake();
    let a = call_devices(&mut f);
    let b = call_devices(&mut f);
    f.turn();
    let first = f.expect_call("GetDevices");
    let second = f.expect_call("GetDevices");
    f.reply(&second, &());
    f.reply(&first, &());
    f.turn();
    let order: Vec<CallId> = replies(&mut f).iter().map(|r| r.call).collect();
    assert_eq!(order, [b, a]);
}

#[test]
fn an_undecodable_body_is_delivered_and_errs_on_decode() {
    let mut f = fake();
    call_devices(&mut f);
    call_devices(&mut f);
    f.turn();
    let first = f.expect_call("GetDevices");
    let second = f.expect_call("GetDevices");
    f.reply(&first, &(7u32,));
    f.send(
        Message::method_return(second.serial())
            .with_destination(UNIQUE)
            .with_sender(NM)
            .with_raw_body(Signature::from("as"), vec![100, 0, 0, 0]),
    );
    f.turn();
    let r = replies(&mut f);
    assert_eq!(r.len(), 2, "both replies arrive");
    let wrong_type = r[0].result.as_ref().unwrap().body::<(String,)>();
    assert!(matches!(wrong_type, Err(DecodeError::Mismatch { .. })));
    let truncated = r[1].result.as_ref().unwrap().body::<(Vec<String>,)>();
    assert_eq!(truncated, Err(DecodeError::Truncated));
    assert!(f.app.resource_mut::<DBus>().session().is_connected());
}

#[test]
fn fds_on_a_bus_without_fd_passing_fail_the_call() {
    let mut f = with_log(FakeBus::without_fds());
    let (r, _w) = std::io::pipe().unwrap();
    let id = f
        .app
        .resource_mut::<DBus>()
        .session()
        .send(MethodCall::new(NM, NM_PATH, NM, "TakeFd").body(&(UnixFd::new(r.into()),)));
    f.turn();
    assert!(
        f.received()
            .iter()
            .all(|m| m.member().is_none_or(|x| x.as_str() != "TakeFd")),
        "nothing was sent"
    );
    let r = replies(&mut f);
    assert_eq!(r[0].call, id);
    assert!(matches!(r[0].result, Err(CallError::FdsUnsupported)));
}

#[test]
fn an_fd_in_a_reply_arrives_usable() {
    let mut f = fake();
    call_devices(&mut f);
    f.turn();
    let call = f.expect_call("GetDevices");
    let (mut reader, writer) = std::io::pipe().unwrap();
    f.send_with_fds(
        Message::method_return(call.serial())
            .with_destination(UNIQUE)
            .with_sender(NM)
            .with_body(&(UnixFd::new(writer.into()),)),
    );
    f.turn();
    let r = replies(&mut f);
    let (fd,): (UnixFd,) = r[0].result.as_ref().unwrap().body().unwrap();
    let mut w = std::io::PipeWriter::from(fd.try_clone_owned().unwrap());
    w.write_all(b"hi").unwrap();
    drop(w);
    drop(fd);
    drop(r);
    let mut s = String::new();
    reader.read_to_string(&mut s).unwrap();
    assert_eq!(s, "hi");
}

fn resend_on_fds_unsupported(app: &mut App, e: &MethodReply) {
    if matches!(e.result, Err(CallError::FdsUnsupported)) {
        app.resource_mut::<DBus>()
            .session()
            .send(MethodCall::new(NM, NM_PATH, NM, "Second"));
    }
}

#[test]
fn a_call_failed_before_sending_is_answered_in_the_next_tick() {
    let mut f = with_log(FakeBus::without_fds());
    f.app.system(resend_on_fds_unsupported);
    f.turn();
    let (r, _w) = std::io::pipe().unwrap();
    f.app
        .resource_mut::<DBus>()
        .session()
        .send(MethodCall::new(NM, NM_PATH, NM, "TakeFd").body(&(UnixFd::new(r.into()),)));
    // `BeforeWait` raises nothing: the reply is work left for the tick.
    f.app.signal(BeforeWait);
    f.app.flush();
    assert!(replies(&mut f).is_empty());
    assert!(f.app.has_pending_work());
    f.app.tick();
    let r = replies(&mut f);
    assert!(matches!(
        r[..],
        [MethodReply {
            result: Err(CallError::FdsUnsupported),
            ..
        }]
    ));
    // The handler's send goes out with the round.
    f.turn();
    f.expect_call("Second");
}

fn stop_on_fds_unsupported(app: &mut App, e: &MethodReply) {
    if matches!(e.result, Err(CallError::FdsUnsupported)) {
        app.signal(Stop);
    }
}

#[test]
fn a_stop_on_a_call_failed_before_sending_ends_the_run() {
    // The bus stays up and idle, so a turn that blocked would never
    // return. The app is not `Send`: it is built on the watched thread.
    let (done, waited) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut f = FakeBus::without_fds();
        f.app.system(stop_on_fds_unsupported);
        let (r, _w) = std::io::pipe().unwrap();
        f.app
            .resource_mut::<DBus>()
            .session()
            .send(MethodCall::new(NM, NM_PATH, NM, "TakeFd").body(&(UnixFd::new(r.into()),)));
        let app = std::mem::replace(&mut f.app, App::new());
        app.run();
        let _ = done.send(());
    });
    waited
        .recv_timeout(Duration::from_secs(5))
        .expect("the run did not end on the reply's Stop");
}

#[test]
fn a_huge_timeout_never_expires() {
    let mut f = fake();
    let id = f
        .app
        .resource_mut::<DBus>()
        .session()
        .send(MethodCall::new(NM, NM_PATH, NM, "GetDevices").timeout(Duration::MAX));
    f.turn();
    let call = f.expect_call("GetDevices");
    f.reply(&call, &());
    f.turn();
    let r = replies(&mut f);
    assert_eq!(r[0].call, id);
    assert!(r[0].result.is_ok());
}
