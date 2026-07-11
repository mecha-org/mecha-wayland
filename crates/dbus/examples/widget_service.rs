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
}

impl WidgetService {
    pub fn new(proxy: DbusProxy<SessionBus>) -> Self {
        Self {
            proxy,
            request: Pending::new(),
            value: 0,
            owned: false,
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
        .on(|s: &mut WidgetService, ev: &DbusEvent<SessionBus>| {
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

            // 4a. Properties.Get(interface, "Value") -> variant.
            if let Some(Ok(call)) = IncomingCall::<fdo::PropertiesGet>::try_from(&ev.msg) {
                let (iface, prop) = &call.args;
                if iface != WIDGET_IFACE {
                    call.error(&s.proxy, fdo::ERR_UNKNOWN_INTERFACE, "no such interface");
                } else if prop == "Value" {
                    call.respond(&s.proxy, &variant(s.value));
                } else {
                    call.error(&s.proxy, fdo::ERR_UNKNOWN_PROPERTY, "no such property");
                }
                return;
            }

            // 4b. Properties.GetAll(interface) -> a{sv}.
            if let Some(Ok(call)) = IncomingCall::<fdo::PropertiesGetAll>::try_from(&ev.msg) {
                let (iface,) = &call.args;
                if iface != WIDGET_IFACE {
                    call.error(&s.proxy, fdo::ERR_UNKNOWN_INTERFACE, "no such interface");
                } else {
                    let mut all = HashMap::new();
                    all.insert("Value".to_string(), variant(s.value));
                    call.respond(&s.proxy, &all);
                }
                return;
            }

            // 4c. Properties.Set(interface, "Value", v).
            if let Some(Ok(call)) = IncomingCall::<fdo::PropertiesSet>::try_from(&ev.msg) {
                let (iface, prop, value) = &call.args;
                if iface != WIDGET_IFACE {
                    call.error(&s.proxy, fdo::ERR_UNKNOWN_INTERFACE, "no such interface");
                } else if prop != "Value" {
                    call.error(&s.proxy, fdo::ERR_UNKNOWN_PROPERTY, "no such property");
                } else if let Ok(v) = u32::try_from(value.clone()) {
                    call.respond(&s.proxy, &());
                    s.set_value(v);
                } else {
                    call.error(
                        &s.proxy,
                        "org.freedesktop.DBus.Error.InvalidArgs",
                        "Value must be u32",
                    );
                }
                return;
            }

            // 5. Standard interfaces: Peer.Ping / GetMachineId, Introspect
            if Widget::handle_standard(&s.proxy, &ev.msg) {
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
