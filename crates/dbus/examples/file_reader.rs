//! A minimal fd-passing demo over D-Bus — no XDG portal spec involved.
//!
//! A tiny service `com.example.FileReader` serves `ReadFd(fd h) -> (content s)`:
//! it receives an open file descriptor, reads the file, and returns its text.
//! A client opens a file, passes its fd in the call, and prints what comes back.
//!
//! Both run in one process on the session bus; the descriptor still travels for
//! real over `SCM_RIGHTS` (to the bus daemon and back), so this exercises both
//! the send-fd and receive-fd transport paths end to end.
//!
//! Run:  cargo run --example file_reader -- /path/to/text/file   (default /etc/hostname)

use std::fs::File;
use std::io::Read;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, RawFd};

use app::{RegisteredModule, prelude::*};
use io_ring::{Ring, RingSettings};

use dbus::{
    DbusConnection, DbusEvent, DbusMessage, DbusProxy, IncomingCall, Pending, SessionBus,
    dbus_handler, dbus_method, fdo, module as dbus_module,
};
// zvariant's fd type serializes/deserializes as the D-Bus `h` type; std's
// OwnedFd has no serde impls and can't appear in a message body.
use zbus::zvariant::OwnedFd;

const READER_NAME: &str = "com.example.FileReader";
const READER_PATH: &str = "/com/example/FileReader";
const READER_IFACE: &str = "com.example.FileReader";

// Service side: a method we serve. The `h` arg arrives as an OwnedFd.
dbus_handler!(ServeReadFd {
    iface: READER_IFACE,
    member: "ReadFd",
    args: (OwnedFd,),
    ret: String,
});

// Client side: the same method, as something we call.
dbus_method!(ReadFd {
    dest: READER_NAME,
    path: READER_PATH,
    iface: READER_IFACE,
    member: "ReadFd",
    args: (OwnedFd,),
    reply: String,
});

/// Emitted by the service once it owns its name, so the client fires only then.
#[derive(Debug)]
pub struct ServiceReady;
impl Event for ServiceReady {}

/// Read a file's text via a passed descriptor without consuming the caller's fd:
/// dup it, wrap the dup in a File, read, and let the dup close on drop.
fn read_fd(raw: RawFd) -> std::io::Result<String> {
    let dup = unsafe { libc::dup(raw) };
    if dup < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut file = unsafe { File::from_raw_fd(dup) };
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    Ok(buf)
}

// --- Service ----------------------------------------------------------------

#[derive(State)]
pub struct ReaderService {
    proxy: DbusProxy<SessionBus>,
    request: Pending<fdo::RequestName>,
}

impl ReaderService {
    pub fn new(proxy: DbusProxy<SessionBus>) -> Self {
        Self {
            proxy,
            request: Pending::new(),
        }
    }
}

pub fn service_module<S>() -> impl RegisteredModule<ReaderService, S> {
    Module::<ReaderService, _, _>::new()
        .on(|s: &mut ReaderService, _: &app::Start| {
            s.request.call(
                &s.proxy,
                &(READER_NAME.to_string(), fdo::NAME_DO_NOT_QUEUE),
                (),
            );
        })
        .on(
            |s: &mut ReaderService, ev: &DbusEvent<SessionBus>| -> Option<ServiceReady> {
                // Name acquired -> tell the client it can call.
                if let Some((_, res)) = s.request.resolve(&ev.msg) {
                    match res {
                        Ok(code)
                            if code == fdo::REQUEST_NAME_PRIMARY_OWNER
                                || code == fdo::REQUEST_NAME_ALREADY_OWNER =>
                        {
                            println!("[service] owning {READER_NAME}");
                            return Some(ServiceReady);
                        }
                        Ok(code) => eprintln!("[service] could not own name (code {code})"),
                        Err(e) => eprintln!("[service] RequestName failed: {e}"),
                    }
                    return None;
                }

                // The fd-bearing call: read from the passed descriptor and reply.
                if let Some(Ok(call)) = IncomingCall::<ServeReadFd>::try_from(&ev.msg) {
                    let raw = call.args.0.as_fd().as_raw_fd();
                    match read_fd(raw) {
                        Ok(content) => {
                            println!("[service] read {} bytes from the passed fd", content.len());
                            call.respond(&s.proxy, &content);
                        }
                        Err(e) => {
                            call.error(
                                &s.proxy,
                                "com.example.FileReader.Error.ReadFailed",
                                &e.to_string(),
                            );
                        }
                    }
                    return None;
                }

                // Anything else on our object -> proper error.
                if let DbusMessage::Call(m) = &ev.msg {
                    let ours = m.header().path().map(|p| p.as_str().to_string()).as_deref()
                        == Some(READER_PATH);
                    if ours {
                        s.proxy.reply_unknown_method(m);
                    }
                }
                None
            },
        )
}

// --- Client -----------------------------------------------------------------

#[derive(State)]
pub struct ReaderClient {
    proxy: DbusProxy<SessionBus>,
    read: Pending<ReadFd, String>, // context = the path, for logging
    path: String,
}

impl ReaderClient {
    pub fn new(proxy: DbusProxy<SessionBus>, path: String) -> Self {
        Self {
            proxy,
            read: Pending::new(),
            path,
        }
    }
}

pub fn client_module<S>() -> impl RegisteredModule<ReaderClient, S> {
    Module::<ReaderClient, _, _>::new()
        .on(|c: &mut ReaderClient, _: &ServiceReady| {
            match File::open(&c.path) {
                Ok(file) => {
                    let std_fd: std::os::fd::OwnedFd = file.into();
                    let fd = OwnedFd::from(std_fd);
                    println!("[client] passing fd for {}", c.path);
                    // send() dup's the fd synchronously, so the tuple may drop after.
                    let path = c.path.clone();
                    c.read.call(&c.proxy, &(fd,), path);
                }
                Err(e) => eprintln!("[client] cannot open {}: {e}", c.path),
            }
        })
        .on(|c: &mut ReaderClient, ev: &DbusEvent<SessionBus>| {
            if let Some((path, res)) = c.read.resolve(&ev.msg) {
                match res {
                    Ok(content) => {
                        println!("[client] got {} bytes back for {path}:", content.len());
                        print!("{content}");
                    }
                    Err(e) => eprintln!("[client] ReadFd failed: {e}"),
                }
            }
        })
}

#[derive(State)]
pub struct AppRoot {
    service: ReaderService,
    client: ReaderClient,
    dbus_session: DbusConnection<SessionBus>,
    ring: Ring,
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/etc/hostname".to_string());

    let ring = Ring::new(RingSettings::default());
    let dbus_session = DbusConnection::<SessionBus>::new(ring.proxy());
    let service = ReaderService::new(dbus_session.proxy());
    let client = ReaderClient::new(dbus_session.proxy(), path);

    let mut app = App::new(AppRoot {
        service,
        client,
        dbus_session,
        ring,
    })
    .mount(service_module())
    .mount(client_module())
    .mount(dbus_module::<SessionBus, _>())
    .mount(io_ring::module());

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}
