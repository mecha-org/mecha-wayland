//! A minimal D-Bus *service* on the same io_uring runtime
//!
//! TEST COMMANDS:
//!   busctl --user introspect org.example.Widget /org/example/Widget
//!   busctl --user get-property org.example.Widget /org/example/Widget org.example.Widget Value
//!   busctl --user set-property org.example.Widget /org/example/Widget org.example.Widget Value u 42
//!   busctl --user call org.example.Widget /org/example/Widget org.example.Widget GetValue
//!   busctl --user call org.example.Widget /org/example/Widget org.example.Widget SetValue u 42

use std::collections::HashMap;

use app::{RegisteredModule, prelude::*};
use io_ring::{Ring, RingSettings};

use dbus::{
    DbusConnection, DbusEvent, DbusMessage, DbusProxy, IncomingCall, Pending, SessionBus,
    dbus_interface,
    fdo::{self, STD_INTERFACES_XML, introspect_node},
    module as dbus_module, variant,
};

const WIDGET_NAME: &str = "org.example.Widget";
const WIDGET_PATH: &str = "/org/example/Widget";
const WIDGET_IFACE: &str = "org.example.Widget";

dbus_interface!(Widget = WIDGET_IFACE;
    method GetValue() -> (value: u32);
    method SetValue(value: u32) -> ();
    signal ValueChanged(value: u32);
    property Value: u32, readwrite;
);

#[derive(State)]
pub struct WidgetService {
    proxy: DbusProxy<SessionBus>,
    request: Pending<fdo::RequestName>,
    value: u32,
    owned: bool,
    #[lens(skip)]
    disconnected: bool,
    #[lens(skip)]
    retry_tick: u32,
}

impl WidgetService {
    pub fn new(proxy: DbusProxy<SessionBus>) -> Self {
        Self {
            proxy,
            request: Pending::new(),
            value: 0,
            owned: false,
            disconnected: false,
            retry_tick: 0,
        }
    }

    fn bootstrap(&mut self) {
        // Ask the bus for our well-known name
        self.request.call(
            &self.proxy,
            &(WIDGET_NAME.to_string(), fdo::NAME_DO_NOT_QUEUE),
            (),
        );
    }

    /// Update `Value` and notify: the app-specific `ValueChanged` signal AND the
    /// standard `PropertiesChanged
    fn set_value(&mut self, v: u32) {
        self.value = v;
        self.proxy.emit::<ValueChanged>(WIDGET_PATH, &(v,));
        let mut changed = HashMap::new();
        changed.insert("Value".to_string(), variant(v));
        self.proxy
            .emit_properties_changed(WIDGET_PATH, WIDGET_IFACE, changed, &[]);
        println!("value set to {v}");
    }
}

pub fn widget_module<S>() -> impl RegisteredModule<WidgetService, S> {
    Module::<WidgetService, _, _>::new()
        .on(|s: &mut WidgetService, _: &app::Start| s.bootstrap())
        .on(|s: &mut WidgetService, _: &app::PrePoll| {
            if !s.disconnected {
                return;
            }
            s.retry_tick += 1;
            if s.retry_tick % 240 != 0 {
                return;
            }
            match s.proxy.reconnect() {
                Ok(()) => {
                    // Established; DbusMessage::Reconnected arrives once the
                    // new Hello resolves — re-bootstrap happens there.
                    s.disconnected = false;
                    println!("reconnected to the session bus");
                }
                Err(e) => eprintln!("reconnect attempt failed: {e}"),
            }
        })
        .on(|s: &mut WidgetService, ev: &DbusEvent<SessionBus>| {
            match &ev.msg {
                DbusMessage::Disconnected => {
                    // Replies to anything in flight can no longer arrive.
                    s.request.clear();
                    s.owned = false;
                    s.disconnected = true;
                    s.retry_tick = 0;
                    return;
                }
                DbusMessage::Reconnected => {
                    // Fresh connection: the bus forgot our name; request again.
                    s.bootstrap();
                    return;
                }
                _ => {}
            }

            // 1. Reply to RequestName call.
            if let Some((_, res)) = s.request.resolve(&ev.msg) {
                match res {
                    Ok(code)
                        if code == fdo::REQUEST_NAME_PRIMARY_OWNER
                            || code == fdo::REQUEST_NAME_ALREADY_OWNER =>
                    {
                        s.owned = true;
                        println!("now serving {WIDGET_NAME} at {WIDGET_PATH}");
                    }
                    Ok(code) => eprintln!("could not own {WIDGET_NAME} (code {code})"),
                    Err(e) => eprintln!("RequestName failed: {e}"),
                }
                return;
            }

            // 2. GetValue -> return current value.
            if let Some(Ok(call)) = IncomingCall::<GetValue>::try_from(&ev.msg) {
                call.respond(&s.proxy, &(s.value,));
                return;
            }

            // 3. SetValue -> update + notify.
            if let Some(Ok(call)) = IncomingCall::<SetValue>::try_from(&ev.msg) {
                let v = call.args.0;
                call.respond(&s.proxy, &());
                s.set_value(v);
                return;
            }

            // 4. Properties Get/GetAll/Set for "Value", one closure over state.
            // (Clone the proxy first: the closure borrows `s` mutably.)
            let proxy = s.proxy.clone();
            if fdo::route_properties(&proxy, &ev.msg, WIDGET_IFACE, &["Value"], |op| match op {
                fdo::PropAccess::Get("Value") => fdo::PropReply::Value(variant(s.value)),
                fdo::PropAccess::Set("Value", v) => match u32::try_from(v.clone()) {
                    Ok(v) => {
                        s.set_value(v);
                        fdo::PropReply::Set
                    }
                    Err(_) => fdo::PropReply::Invalid("Value must be u32".to_string()),
                },
                _ => fdo::PropReply::Unknown,
            }) {
                return;
            }

            // 5. Standard interfaces: Peer.Ping / GetMachineId, Introspect
            if Widget::handle_standard(&s.proxy, WIDGET_PATH, &ev.msg) {
                return;
            }

            // 6. Fallback: Reply unknown method
            if let DbusMessage::Call(m) = &ev.msg {
                let on_our_object = m.header().path().map(|p| p.as_str().to_string()).as_deref()
                    == Some(WIDGET_PATH);
                if on_our_object {
                    s.proxy.reply_unknown_method(m);
                }
            }
        })
}

// Wire the WidgetService to an application
#[derive(State)]
pub struct AppRoot {
    widget: WidgetService,
    dbus_session: DbusConnection<SessionBus>,
    ring: Ring,
}

fn main() {
    let ring = Ring::new(RingSettings::default());
    let dbus_session = DbusConnection::<SessionBus>::new(ring.proxy());
    let widget = WidgetService::new(dbus_session.proxy());

    let mut app = App::new(AppRoot {
        widget,
        dbus_session,
        ring,
    })
    .mount(widget_module())
    .mount(dbus_module::<SessionBus, _>())
    .mount(io_ring::module());

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}
