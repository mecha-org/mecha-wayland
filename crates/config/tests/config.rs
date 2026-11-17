//! Tests the module on local files and through a fake daemon.

use std::fs;
use std::path::Path;

use app::prelude::*;
use config::prelude::*;
use dbus::fake::FakeBus;
use dbus::{BusName, InterfaceName, MemberName, Message, ObjectPath, PropMap, Value as DValue};
use mxconfig::host::Roots;
use mxconfig::{
    Change, Detach, Event, FromSettings, Namespace, Settings, Source, State, Unserved, Value,
};
use ring::prelude::*;

const NS: &str = "org.mechanix.test";
const SCHEMA: &str = r#"
namespace = "org.mechanix.test"
version = 1

[keys.greeting]
type = "string"
default = "hi"
"#;
/// `SCHEMA`, for an app that asks for the daemon only.
const REMOTE: &str = r#"
namespace = "org.mechanix.test"
version = 1
backend = "daemon-only"

[keys.greeting]
type = "string"
default = "hi"
"#;
const DAEMON: &str = ":1.7";

/// mxconfigd's bus name, which is also its interface.
const BUS_NAME: &str = "org.mechanix.MxConfig";
const INTERFACE: &str = BUS_NAME;

/// The daemon's object path.
const OBJECT_PATH: &str = "/org/mechanix/MxConfig";

/// A hand-written client, like a generated one.
#[derive(Clone)]
struct Test(Settings);

impl FromSettings for Test {
    fn from_settings(settings: Settings) -> Self {
        Test(settings)
    }
}

impl Namespace for Test {
    const NAMESPACE: &'static str = NS;
    const SCHEMA: &'static str = SCHEMA;
    type Change = (String, Option<Value>);

    fn change(change: &Change) -> Option<Self::Change> {
        Some((change.key.clone(), change.value.clone()))
    }
}

/// `Test`, for an app that asks for the daemon only.
#[derive(Clone)]
struct Remote(Settings);

impl FromSettings for Remote {
    fn from_settings(settings: Settings) -> Self {
        Remote(settings)
    }
}

impl Namespace for Remote {
    const NAMESPACE: &'static str = NS;
    const SCHEMA: &'static str = REMOTE;
    type Change = (String, Option<Value>);

    fn change(change: &Change) -> Option<Self::Change> {
        Test::change(change)
    }
}

/// A second client type of `Test`'s namespace.
#[derive(Clone)]
struct Second(Settings);

impl FromSettings for Second {
    fn from_settings(settings: Settings) -> Self {
        Second(settings)
    }
}

impl Namespace for Second {
    const NAMESPACE: &'static str = NS;
    const SCHEMA: &'static str = SCHEMA;
    type Change = (String, Option<Value>);

    fn change(change: &Change) -> Option<Self::Change> {
        Test::change(change)
    }
}

impl Handle for Second {
    fn settings(&self) -> &Settings {
        &self.0
    }
}

/// A client's handle.
trait Handle: Namespace<Change = (String, Option<Value>)> + 'static {
    fn settings(&self) -> &Settings;
}

impl Handle for Test {
    fn settings(&self) -> &Settings {
        &self.0
    }
}

impl Handle for Remote {
    fn settings(&self) -> &Settings {
        &self.0
    }
}

const SHARED: &str = "org.mechanix.desktop.test";
/// A desktop namespace, which only the daemon serves.
const SHARED_SCHEMA: &str = r#"
namespace = "org.mechanix.desktop.test"
version = 1

[keys.greeting]
type = "string"
default = "hi"
"#;

/// A client of a shared namespace.
#[derive(Clone)]
struct Shared(Settings);

impl FromSettings for Shared {
    fn from_settings(settings: Settings) -> Self {
        Shared(settings)
    }
}

impl Namespace for Shared {
    const NAMESPACE: &'static str = SHARED;
    const SCHEMA: &'static str = SHARED_SCHEMA;
    type Change = (String, Option<Value>);

    fn change(change: &Change) -> Option<Self::Change> {
        Test::change(change)
    }
}

impl Handle for Shared {
    fn settings(&self) -> &Settings {
        &self.0
    }
}

/// A key and the value it holds.
type Held = (String, Option<Value>);

#[derive(Default)]
struct Log {
    opened: Vec<Result<Settings, String>>,
    changed: Vec<(String, Option<Value>)>,
    /// Each failed write's error, and what its keys still hold.
    failed: Vec<(String, Vec<Held>)>,
    detached: Vec<Detach>,
}
impl Resource for Log {}

fn on_config<T: Handle>(app: &mut App, ConfigEvent(event): &ConfigEvent<T>) {
    let mut log = app.resource_mut::<Log>();
    match event {
        Event::Opened(client) => log.opened.push(Ok(client.settings().clone())),
        Event::OpenFailed(e) => log.opened.push(Err(e.to_string())),
        Event::Changed(change) => log.changed.push(change.clone()),
        Event::Detached(cause) => log.detached.push(*cause),
        Event::WriteFailed { error, current } => {
            log.failed.push((error.to_string(), current.clone()));
        }
        _ => {}
    }
}

/// Log the events of client `T`.
fn listen<T: Handle>(app: &mut App) {
    app.init_resource::<Log>().system(on_config::<T>);
}

/// A tree without an installed schema.
fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(format!("user/{NS}"))).unwrap();
    dir
}

fn user_file(dir: &Path) -> std::path::PathBuf {
    dir.join(format!("user/{NS}/config.toml"))
}

fn local_app(dir: &Path) -> App {
    let mut app = App::new();
    let source = Source::with_roots(Roots::under(dir));
    app.add_module(RingModule::default())
        .add_module(ConfigModule::new().with_source(source));
    listen::<Test>(&mut app);
    app
}

fn greeting(settings: &Settings) -> Option<String> {
    settings
        .get("greeting")
        .and_then(|v| v.as_str().map(str::to_string))
}

#[test]
fn a_local_open_answers_on_the_next_turn() {
    let dir = tree();
    let mut app = local_app(dir.path());
    app.resource_mut::<Config>().request::<Test>();
    assert!(
        app.resource::<Log>().opened.is_empty(),
        "nothing runs until a turn"
    );
    Ring::turn(&mut app);
    let opened = std::mem::take(&mut app.resource_mut::<Log>().opened);
    let settings = opened.into_iter().next().unwrap().unwrap();
    assert_eq!(greeting(&settings).as_deref(), Some("hi"));
    let kept = &app.resource::<Client<Test>>().0;
    assert_eq!(
        greeting(&kept.0).as_deref(),
        Some("hi"),
        "the client is kept"
    );
}

/// Request `Test` and run the turn that opens it.
fn open_local(app: &mut App) -> Settings {
    app.resource_mut::<Config>().request::<Test>();
    Ring::turn(app);
    let opened = std::mem::take(&mut app.resource_mut::<Log>().opened);
    opened.into_iter().next().expect("opened").unwrap()
}

#[test]
fn an_edited_file_is_a_typed_change() {
    let dir = tree();
    let mut app = local_app(dir.path());
    let settings = open_local(&mut app);

    fs::write(user_file(dir.path()), "greeting = \"hello\"\n").unwrap();
    // The armed read may return the file's creation before its write.
    for _ in 0..5 {
        Ring::turn(&mut app);
        if !app.resource::<Log>().changed.is_empty() {
            break;
        }
    }
    let changed = std::mem::take(&mut app.resource_mut::<Log>().changed);
    let hello = Some(Value::String("hello".into()));
    assert_eq!(changed, [("greeting".to_string(), hello)]);
    assert_eq!(greeting(&settings).as_deref(), Some("hello"));
}

#[test]
fn a_write_arrives_as_a_change() {
    let dir = tree();
    let mut app = local_app(dir.path());
    let settings = open_local(&mut app);
    let edit = mxconfig::Edit::set("greeting", Value::String("yo".into()));
    settings.write(&[edit]);
    Ring::turn(&mut app);
    let log = std::mem::take(&mut *app.resource_mut::<Log>());
    assert!(log.failed.is_empty(), "{:?}", log.failed);
    assert_eq!(
        log.changed,
        [("greeting".to_string(), Some(Value::String("yo".into())))]
    );
    let file = fs::read_to_string(user_file(dir.path())).unwrap();
    assert!(file.contains("yo"), "the file holds the write: {file}");
}

#[test]
fn without_dbus_a_shared_namespace_stays_on_its_defaults() {
    let dir = tree();
    // A shared file is there, but only the daemon may read it.
    let file = dir.path().join(format!("user/mxconfig/{SHARED}/conf.toml"));
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "greeting = \"from the file\"\n").unwrap();
    let mut app = local_app(dir.path());
    listen::<Shared>(&mut app);
    app.resource_mut::<Config>().request::<Shared>();
    Ring::turn(&mut app);
    let opened = std::mem::take(&mut app.resource_mut::<Log>().opened);
    let settings = opened.into_iter().next().expect("opened").unwrap();
    assert_eq!(settings.state(), State::Defaults(Unserved::NoBus));
    assert_eq!(greeting(&settings).as_deref(), Some("hi"));

    let edit = mxconfig::Edit::set("greeting", Value::String("yo".into()));
    settings.write(&[edit]);
    Ring::turn(&mut app);
    let log = std::mem::take(&mut *app.resource_mut::<Log>());
    let hi = Some(Value::String("hi".into()));
    let still = vec![("greeting".to_string(), hi)];
    assert_eq!(
        log.failed,
        [("the runtime cannot reach mxconfigd".to_string(), still)]
    );
    assert!(log.changed.is_empty());
    assert_eq!(greeting(&settings).as_deref(), Some("hi"));
}

/// Two client types of one namespace each get their open, and both get its writes and changes.
#[test]
fn every_client_type_of_a_namespace_gets_its_signals() {
    let dir = tree();
    let mut app = local_app(dir.path());
    listen::<Second>(&mut app);
    app.resource_mut::<Config>().request::<Test>();
    app.resource_mut::<Config>().request::<Second>();
    Ring::turn(&mut app);
    let opened = std::mem::take(&mut app.resource_mut::<Log>().opened);
    assert_eq!(opened.len(), 2, "one open each");
    let settings = opened.into_iter().next().unwrap().unwrap();

    let edit = mxconfig::Edit::set("greeting", Value::String("yo".into()));
    settings.write(&[edit]);
    Ring::turn(&mut app);
    let log = std::mem::take(&mut *app.resource_mut::<Log>());
    assert_eq!(log.changed.len(), 2);
}

/// A fake bus where `DAEMON` owns mxconfigd's name, with the module installed.
fn daemon_app(dir: &Path) -> FakeBus {
    let mut f = FakeBus::new();
    f.owners.insert(BUS_NAME.into(), DAEMON.into());
    let source = Source::with_roots(Roots::under(dir));
    f.app.add_module(ConfigModule::new().with_source(source));
    listen::<Remote>(&mut f.app);
    f.turn();
    f.received();
    f
}

/// The daemon restarting as `owner`.
fn restart(f: &mut FakeBus, owner: &str) {
    let changed = Message::signal(
        ObjectPath::new("/org/freedesktop/DBus").unwrap(),
        InterfaceName::new("org.freedesktop.DBus").unwrap(),
        MemberName::new("NameOwnerChanged").unwrap(),
    )
    .with_sender(BusName::new("org.freedesktop.DBus").unwrap())
    .with_body(&(BUS_NAME, DAEMON, owner));
    f.owners.insert(BUS_NAME.into(), owner.into());
    f.send(changed);
    f.turn();
}

/// The daemon's `Changed`, with every value at `revision`.
fn changed(namespace: &str, revision: u64, values: PropMap) -> Message {
    Message::signal(
        ObjectPath::new(OBJECT_PATH).unwrap(),
        InterfaceName::new(INTERFACE).unwrap(),
        MemberName::new("Changed").unwrap(),
    )
    .with_sender(BusName::new(DAEMON).unwrap())
    .with_destination(BusName::new(dbus::fake::UNIQUE).unwrap())
    .with_body(&(namespace, revision, values))
}

/// `greeting` set to the TOML `text`, as the daemon sends values.
fn greeting_is(text: &str) -> PropMap {
    PropMap::from([("greeting".to_string(), DValue::Str(text.into()))])
}

/// An empty asset list for a `Subscribe` reply.
fn no_assets() -> Vec<(String, String, dbus::UnixFd)> {
    Vec::new()
}

/// Open through the fake daemon, with `greeting = "there"` at revision 1.
fn subscribe(f: &mut FakeBus) -> Settings {
    f.app.resource_mut::<Config>().request::<Remote>();
    f.turn();
    let call = f.expect_call("Subscribe");
    let (namespace, schema, app): (String, String, String) = call.body().unwrap();
    // The client sends its schema for the daemon to register, as its own app.
    assert_eq!((namespace.as_str(), schema.as_str()), (NS, REMOTE));
    assert_eq!(app, NS);
    let values = PropMap::from([("greeting".to_string(), DValue::Str("\"there\"".into()))]);
    // The subscription's descriptor. Any open file stands in for the daemon's pipe.
    let subscription = dbus::UnixFd::new(fs::File::open("/dev/null").unwrap().into());
    let reply = Message::method_return(call.serial())
        .with_destination(BusName::new(dbus::fake::UNIQUE).unwrap())
        .with_sender(BusName::new(DAEMON).unwrap())
        .with_body(&(subscription, SCHEMA, 1u64, values, no_assets()));
    f.send_with_fds(reply);
    f.turn();
    let opened = std::mem::take(&mut f.app.resource_mut::<Log>().opened);
    opened.into_iter().next().expect("opened").unwrap()
}

#[test]
fn a_daemon_open_subscribes_and_follows_its_changes() {
    let dir = tree();
    let mut f = daemon_app(dir.path());
    let settings = subscribe(&mut f);
    assert_eq!(greeting(&settings).as_deref(), Some("there"));

    f.send(changed(NS, 2, greeting_is("\"again\"")));
    f.turn();
    let log = std::mem::take(&mut *f.app.resource_mut::<Log>());
    assert_eq!(
        log.changed,
        [("greeting".to_string(), Some(Value::String("again".into())))]
    );
    assert_eq!(greeting(&settings).as_deref(), Some("again"));
}

#[test]
fn a_changed_from_anyone_but_the_daemon_is_ignored() {
    let dir = tree();
    let mut f = daemon_app(dir.path());
    let settings = subscribe(&mut f);
    let forged =
        changed(NS, 2, greeting_is("\"forged\"")).with_sender(BusName::new(":1.9").unwrap());
    f.send(forged);
    f.turn();
    assert!(f.app.resource::<Log>().changed.is_empty());
    assert_eq!(greeting(&settings).as_deref(), Some("there"));
}

#[test]
fn a_daemon_write_asks_with_interactive_auth_and_lands() {
    let dir = tree();
    let mut f = daemon_app(dir.path());
    let settings = subscribe(&mut f);
    let edit = mxconfig::Edit::set("greeting", Value::String("yo".into()));
    settings.write(&[edit]);
    f.turn();
    let call = f.expect_call("Apply");
    assert!(
        call.flags()
            .contains(dbus::Flags::ALLOW_INTERACTIVE_AUTHORIZATION)
    );
    let (namespace, edits, app): (String, Vec<(String, bool, String)>, String) =
        call.body().unwrap();
    assert_eq!((namespace.as_str(), app.as_str()), (NS, NS));
    assert_eq!(
        edits,
        [("greeting".to_string(), true, "\"yo\"".to_string())]
    );
    // The daemon sends `Changed` before the reply.
    f.send(changed(NS, 2, greeting_is("\"yo\"")));
    f.reply(&call, &(2u64, greeting_is("\"yo\"")));
    f.turn();
    let log = std::mem::take(&mut *f.app.resource_mut::<Log>());
    assert_eq!(
        log.changed,
        [("greeting".to_string(), Some(Value::String("yo".into())))]
    );
    assert!(log.failed.is_empty(), "{:?}", log.failed);
}

#[test]
fn a_write_answered_before_its_signal_takes_the_reply() {
    let dir = tree();
    let mut f = daemon_app(dir.path());
    let settings = subscribe(&mut f);
    let edit = mxconfig::Edit::clear("greeting");
    settings.write(&[edit]);
    f.turn();
    let call = f.expect_call("Apply");
    // The reply carries every value, so the missing signal is made up.
    f.reply(&call, &(2u64, PropMap::new()));
    f.turn();
    let log = std::mem::take(&mut *f.app.resource_mut::<Log>());
    assert_eq!(log.changed, [("greeting".to_string(), None)]);
    assert!(log.failed.is_empty(), "{:?}", log.failed);
}

#[test]
fn a_new_daemon_owner_resubscribes() {
    let dir = tree();
    let mut f = daemon_app(dir.path());
    let _settings = subscribe(&mut f);
    restart(&mut f, ":1.8");
    f.expect_call("Subscribe");
}

#[test]
fn a_daemon_open_without_dbus_fails_at_once() {
    let dir = tree();
    let mut app = App::new();
    let source = Source::with_roots(Roots::under(dir.path()));
    app.add_module(RingModule::default())
        .add_module(ConfigModule::new().with_source(source));
    listen::<Remote>(&mut app);
    app.resource_mut::<Config>().request::<Remote>();
    Ring::turn(&mut app);
    let opened = std::mem::take(&mut app.resource_mut::<Log>().opened);
    assert!(opened[0].is_err(), "{:?}", opened[0].as_ref().err());
}

/// A failed open goes only to the client type that asked for it.
#[test]
fn a_failed_open_answers_only_its_own_request() {
    let dir = tree();
    let mut app = local_app(dir.path());
    listen::<Remote>(&mut app);
    app.resource_mut::<Config>().request::<Remote>();
    app.resource_mut::<Config>().request::<Test>();
    Ring::turn(&mut app);
    let opened = std::mem::take(&mut app.resource_mut::<Log>().opened);
    let failed = opened.iter().filter(|o| o.is_err()).count();
    assert_eq!((opened.len(), failed), (2, 1), "{opened:?}");
}

/// A daemon restarted on another schema version detaches the handle, as a signal.
#[test]
fn a_handle_another_version_takes_over_is_detached() {
    let dir = tree();
    let mut f = daemon_app(dir.path());
    let settings = subscribe(&mut f);
    restart(&mut f, ":1.8");
    let call = f.expect_call("Subscribe");
    f.error(
        &call,
        "org.mechanix.MxConfig.Error.VersionConflict",
        "v2 holds it",
    );
    f.turn();
    assert_eq!(f.app.resource::<Log>().detached, [Detach::Moved]);
    assert_eq!(
        greeting(&settings).as_deref(),
        Some("there"),
        "it keeps its values"
    );
}

/// A write in flight when the bus goes away may have landed.
#[test]
fn a_write_in_flight_when_the_bus_goes_is_unconfirmed() {
    let dir = tree();
    let mut f = daemon_app(dir.path());
    let settings = subscribe(&mut f);
    let edit = mxconfig::Edit::set("greeting", Value::String("yo".into()));
    settings.write(&[edit]);
    f.turn();
    f.expect_call("Apply");
    let mut app = f.hang_up();
    Ring::turn(&mut app);
    let log = app.resource::<Log>();
    let [(e, current)] = &log.failed[..] else {
        panic!("{:?}", log.failed)
    };
    assert!(e.contains("may or may not have landed"), "{e}");
    let there = Some(Value::String("there".into()));
    assert_eq!(current, &[("greeting".to_string(), there)]);
}
