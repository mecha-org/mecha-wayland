//! Tests against a real mxconfigd on a private bus: ignored unless asked
//! for, with `MXCONFIGD` set to the daemon binary.
//!
//! FIXME: run these in CI: build mxconfigd at the pinned mxconfig rev, set
//! `MXCONFIGD`, and run `cargo test -p config --test mxconfigd -- --ignored`.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use app::prelude::*;
use config::prelude::*;
use dbus::{BusId, DBusModule};
use mxconfig::host::Roots;
use mxconfig::{Change, Edit, Event, FromSettings, Namespace, Settings, Source, Value};
use ring::prelude::*;

const NS: &str = "org.mechanix.test";
const SCHEMA: &str = r#"
namespace = "org.mechanix.test"
version = 1

[keys.greeting]
type = "string"
default = "hi"

[assets.banner]
path = "banner.txt"
"#;

/// `SCHEMA`, for an app that asks for the daemon only.
const REMOTE: &str = r#"
namespace = "org.mechanix.test"
version = 1
backend = "daemon-only"

[keys.greeting]
type = "string"
default = "hi"

[assets.banner]
path = "banner.txt"
"#;

#[derive(Clone)]
struct Test(Settings);

impl FromSettings for Test {
    fn from_settings(settings: Settings) -> Self {
        Test(settings)
    }
}

impl Namespace for Test {
    const NAMESPACE: &'static str = NS;
    const SCHEMA: &'static str = REMOTE;
    type Change = Change;

    fn change(change: &Change) -> Option<Change> {
        Some(change.clone())
    }

    fn asset_change(id: &str) -> Option<Change> {
        // Report an asset change as a change without a value.
        Some(Change::new(NS, format!("asset:{id}"), None, 0))
    }
}

#[derive(Default)]
struct Log {
    opened: Vec<Settings>,
    changed: Vec<Change>,
    failed: Vec<String>,
}
impl Resource for Log {}

fn on_config(app: &mut App, ConfigEvent(event): &ConfigEvent<Test>) {
    let mut log = app.resource_mut::<Log>();
    match event {
        Event::Opened(client) => log.opened.push(client.0.clone()),
        Event::OpenFailed(e) => panic!("the daemon opens it: {e}"),
        Event::Changed(change) => log.changed.push(change.clone()),
        Event::WriteFailed { error, .. } => log.failed.push(error.to_string()),
        _ => {}
    }
}

/// Kills its processes when dropped.
struct Kill(Vec<Child>);

impl Drop for Kill {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Run the app until `done`. Each turn waits on the ring.
fn run_until(app: &mut App, done: impl Fn(&Log) -> bool) {
    for _ in 0..50 {
        app.tick();
        if done(app.resource::<Log>()) {
            return;
        }
        Ring::turn(app);
    }
    panic!("not done after 50 turns");
}

fn greeting(settings: &Settings) -> Option<String> {
    settings
        .get("greeting")
        .and_then(|v| v.as_str().map(str::to_string))
}

fn text(change: &Change) -> Option<&str> {
    change.value.as_ref().and_then(Value::as_str)
}

/// Start a private bus and mxconfigd. Returns the bus address.
fn start(daemon: &str, dir: &Path, kill: &mut Kill) -> String {
    let mut bus = Command::new("dbus-daemon")
        .args(["--session", "--print-address"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("dbus-daemon");
    let mut address = String::new();
    BufReader::new(bus.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    let address = address.trim().to_string();
    kill.0.push(bus);
    let none = dir.join("none");
    let mxconfigd = Command::new(daemon)
        .env("DBUS_SESSION_BUS_ADDRESS", &address)
        .env("MXCONFIG_SCHEMA_DIR", dir.join("schemas"))
        .env("MXCONFIG_USER_DIR", dir.join("user"))
        .env("MXCONFIG_VENDOR_DIR", &none)
        .env("MXCONFIG_ADMIN_DIR", &none)
        .spawn()
        .expect("mxconfigd");
    kill.0.push(mxconfigd);
    // `gdbus wait` waits for the name owner signal.
    let owned = Command::new("gdbus")
        .args([
            "wait",
            "--session",
            "--timeout",
            "10",
            "org.mechanix.MxConfig",
        ])
        .env("DBUS_SESSION_BUS_ADDRESS", &address)
        .status()
        .expect("gdbus");
    assert!(owned.success(), "mxconfigd never owned its name");
    address
}

#[test]
#[ignore = "needs MXCONFIGD"]
fn opens_writes_and_hears_another_writer_through_mxconfigd() {
    // Asked for, the test fails rather than pass without its daemon.
    let daemon = std::env::var("MXCONFIGD").expect("MXCONFIGD names the mxconfigd binary");
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("schemas")).unwrap();
    fs::write(dir.path().join(format!("schemas/{NS}.toml")), SCHEMA).unwrap();
    let mut kill = Kill(Vec::new());
    let address = start(&daemon, dir.path(), &mut kill);

    // The app carries a daemon-only schema.
    let app_dir = dir.path().join("app");
    let source = Source::with_roots(Roots::under(&app_dir));
    let mut app = App::new();
    app.add_module(RingModule::default())
        .add_module(DBusModule::new().address(BusId::Session, &address))
        .add_module(ConfigModule::new().with_source(source))
        .init_resource::<Log>()
        .system(on_config);

    app.resource_mut::<Config>().request::<Test>();
    run_until(&mut app, |log| !log.opened.is_empty());
    let settings = app.resource::<Log>().opened[0].clone();
    assert_eq!(greeting(&settings).as_deref(), Some("hi"));

    let edit = Edit::set("greeting", Value::String("yo".into()));
    settings.write(&[edit]);
    run_until(&mut app, |log| {
        !log.changed.is_empty() || !log.failed.is_empty()
    });
    let log = std::mem::take(&mut *app.resource_mut::<Log>());
    assert!(log.failed.is_empty(), "{:?}", log.failed);
    assert_eq!(
        log.changed.iter().map(text).collect::<Vec<_>>(),
        [Some("yo")]
    );
    let file = dir.path().join(format!("user/{NS}/config.toml"));
    assert!(fs::read_to_string(&file).unwrap().contains("yo"));

    // Edit the file directly, atomically. The daemon sees it through inotify.
    let tmp = dir.path().join("config.tmp");
    fs::write(&tmp, "greeting = \"from the file\"\n").unwrap();
    fs::rename(&tmp, &file).unwrap();
    run_until(&mut app, |log| !log.changed.is_empty());
    let changed = &app.resource::<Log>().changed;
    assert_eq!(text(&changed[0]), Some("from the file"));
    assert_eq!(greeting(&settings).as_deref(), Some("from the file"));

    // The daemon passes an fd for the asset when it changes.
    std::mem::take(&mut *app.resource_mut::<Log>());
    let banner = dir.path().join(format!("user/{NS}/banner.txt"));
    fs::write(dir.path().join("banner.tmp"), "hello").unwrap();
    fs::rename(dir.path().join("banner.tmp"), &banner).unwrap();
    run_until(&mut app, |log| {
        log.changed.iter().any(|c| c.key == "asset:banner")
    });
    let asset = settings.asset("banner").expect("served");
    assert_eq!(asset.text().unwrap(), "hello");
    assert!(
        !asset.path.starts_with(dir.path().join("app")),
        "the daemon's file"
    );

    // Write the asset from bytes, then from a file, then clear it.
    let picked = dir.path().join("picked.txt");
    fs::write(&picked, "picked").unwrap();
    let mut picked = Some(std::os::fd::OwnedFd::from(fs::File::open(&picked).unwrap()));
    for text in [Some("bytes"), Some("picked"), None] {
        std::mem::take(&mut *app.resource_mut::<Log>());
        match text {
            Some("bytes") => settings.write_asset("banner", b"bytes"),
            Some(_) => settings.write_asset_from("banner", picked.take().unwrap()),
            None => settings.remove_asset("banner"),
        }
        run_until(&mut app, |log| {
            log.changed.iter().any(|c| c.key == "asset:banner") || !log.failed.is_empty()
        });
        let failed = &app.resource::<Log>().failed;
        assert!(failed.is_empty(), "{failed:?}");
        let now = settings.asset("banner").map(|asset| asset.text().unwrap());
        assert_eq!(now.as_deref(), text);
        assert_eq!(fs::read_to_string(&banner).ok().as_deref(), text);
    }
}
