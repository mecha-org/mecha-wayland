use std::collections::HashMap;

use zbus::zvariant::OwnedValue;

use crate::{Bus, DbusMessage, DbusProxy, IncomingCall, dbus_handler, dbus_method, dbus_signal};

dbus_method!(pub Hello {
    dest: "org.freedesktop.DBus",
    path: "/org/freedesktop/DBus",
    iface: "org.freedesktop.DBus",
    member: "Hello",
    args: (), reply: String,
});
dbus_method!(pub AddMatch {
    dest: "org.freedesktop.DBus",
    path: "/org/freedesktop/DBus",
    iface: "org.freedesktop.DBus",
    member: "AddMatch",
    args: (String,), reply: (),
});
dbus_method!(pub RemoveMatch {
    dest: "org.freedesktop.DBus",
    path: "/org/freedesktop/DBus",
    iface: "org.freedesktop.DBus",
    member: "RemoveMatch",
    args: (String,), reply: (),
});

dbus_method!(pub RequestName {
    dest: "org.freedesktop.DBus",
    path: "/org/freedesktop/DBus",
    iface: "org.freedesktop.DBus",
    member: "RequestName",
    args: (String, u32), reply: u32,
});
dbus_method!(pub ReleaseName {
    dest: "org.freedesktop.DBus",
    path: "/org/freedesktop/DBus",
    iface: "org.freedesktop.DBus",
    member: "ReleaseName",
    args: (String,), reply: u32,
});

/// `RequestName` flags.
pub const NAME_ALLOW_REPLACEMENT: u32 = 0x1;
pub const NAME_REPLACE_EXISTING: u32 = 0x2;
pub const NAME_DO_NOT_QUEUE: u32 = 0x4;

/// `RequestName` reply codes.
pub const REQUEST_NAME_PRIMARY_OWNER: u32 = 1;
pub const REQUEST_NAME_IN_QUEUE: u32 = 2;
pub const REQUEST_NAME_EXISTS: u32 = 3;
pub const REQUEST_NAME_ALREADY_OWNER: u32 = 4;

// --- org.freedesktop.DBus.Properties (serve side) -------------------
pub const PROPERTIES_IFACE: &str = "org.freedesktop.DBus.Properties";
/// Standard error names for property access.
pub const ERR_UNKNOWN_PROPERTY: &str = "org.freedesktop.DBus.Error.UnknownProperty";
pub const ERR_UNKNOWN_INTERFACE: &str = "org.freedesktop.DBus.Error.UnknownInterface";
pub const ERR_PROPERTY_READ_ONLY: &str = "org.freedesktop.DBus.Error.PropertyReadOnly";
pub const ERR_INVALID_ARGS: &str = "org.freedesktop.DBus.Error.InvalidArgs";

// Get(interface s, prop s) -> value v
dbus_handler!(pub PropertiesGet {
    iface: "org.freedesktop.DBus.Properties",
    member: "Get",
    args: (String, String), ret: OwnedValue,

});
// GetAll(interface s) -> props a{sv}
dbus_handler!(pub PropertiesGetAll {
    iface: "org.freedesktop.DBus.Properties",
    member: "GetAll",
    args: (String,), ret: HashMap<String, OwnedValue>,
});

// Set(interface s, prop s, value v) -> ()
dbus_handler!(pub PropertiesSet {
    iface: "org.freedesktop.DBus.Properties",
    member: "Set",
    args: (String, String, OwnedValue), ret: (),
});

// PropertiesChanged(interface s, changed a{sv}, invalidated as)
dbus_signal!(pub PropertiesChanged {
    iface: "org.freedesktop.DBus.Properties",
    member: "PropertiesChanged",
    args: (String, HashMap<String, OwnedValue>, Vec<String>),
});

pub const STD_INTERFACES_XML: &str = concat!(
    "  <interface name=\"org.freedesktop.DBus.Peer\">\n",
    "    <method name=\"Ping\"/>\n",
    "    <method name=\"GetMachineId\"><arg type=\"s\" direction=\"out\"/></method>\n",
    "  </interface>\n",
    "  <interface name=\"org.freedesktop.DBus.Introspectable\">\n",
    "    <method name=\"Introspect\"><arg type=\"s\" direction=\"out\"/></method>\n",
    "  </interface>\n",
);

pub const PROPERTIES_INTERFACE_XML: &str = concat!(
    "  <interface name=\"org.freedesktop.DBus.Properties\">\n",
    "    <method name=\"Get\"><arg type=\"s\" direction=\"in\"/><arg type=\"s\" direction=\"in\"/><arg type=\"v\" direction=\"out\"/></method>\n",
    "    <method name=\"GetAll\"><arg type=\"s\" direction=\"in\"/><arg type=\"a{sv}\" direction=\"out\"/></method>\n",
    "    <method name=\"Set\"><arg type=\"s\" direction=\"in\"/><arg type=\"s\" direction=\"in\"/><arg type=\"v\" direction=\"in\"/></method>\n",
    "    <signal name=\"PropertiesChanged\"><arg type=\"s\"/><arg type=\"a{sv}\"/><arg type=\"as\"/></signal>\n",
    "  </interface>\n",
);

// Standard Objects for Peer, Introspectable
pub const PEER_IFACE: &str = "org.freedesktop.DBus.Peer";
pub const INTROSPECTABLE_IFACE: &str = "org.freedesktop.DBus.Introspectable";

dbus_handler!(pub Ping {
    iface: "org.freedesktop.DBus.Peer", member: "Ping", args: (), ret: (),
});
dbus_handler!(pub GetMachineId {
    iface: "org.freedesktop.DBus.Peer", member: "GetMachineId", args: (), ret: String,
});
dbus_handler!(pub Introspect {
    iface: "org.freedesktop.DBus.Introspectable", member: "Introspect", args: (), ret: String,
});

/// Wrap one or more `<interface>` XML nodes into a full introspection document.
pub fn introspect_node(interfaces: &[&str]) -> String {
    let mut s = String::from(
        "<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\"\n \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n<node>\n",
    );
    for i in interfaces {
        s.push_str(i);
    }
    s.push_str("</node>\n");
    s
}

/// The D-Bus machine id, for `org.freedesktop.DBus.Peer.GetMachineId`. Reads the
/// standard files
pub fn machine_id() -> String {
    ::std::fs::read_to_string("/var/lib/dbus/machine-id")
        .or_else(|_| ::std::fs::read_to_string("/etc/machine-id"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Answer the standard object interfaces for a service object: `Peer.Ping` and
/// `Peer.GetMachineId` (connection-level, any path) and
/// `Introspectable.Introspect` (per-object: only for `path`, with
/// `interface_xml` as this object's `<interface>` node). Pass
/// `has_properties = true` for objects that serve `Properties`, so the
/// introspection XML advertises that interface too. Returns `true` if the
/// message was consumed. `dbus_interface!`'s generated `handle_standard`
/// forwards here, deriving both arguments from the declaration.
pub fn handle_standard<B: Bus>(
    proxy: &DbusProxy<B>,
    path: &str,
    interface_xml: &str,
    has_properties: bool,
    msg: &DbusMessage,
) -> bool {
    if let Some(Ok(call)) = IncomingCall::<Ping>::try_from(msg) {
        call.respond(proxy, &());
        return true;
    }
    if let Some(Ok(call)) = IncomingCall::<GetMachineId>::try_from(msg) {
        call.respond(proxy, &machine_id());
        return true;
    }
    if let Some(Ok(call)) = IncomingCall::<Introspect>::try_from(msg) {
        if call.path.as_deref() == Some(path) {
            let xml = if has_properties {
                introspect_node(&[interface_xml, STD_INTERFACES_XML, PROPERTIES_INTERFACE_XML])
            } else {
                introspect_node(&[interface_xml, STD_INTERFACES_XML])
            };
            call.respond(proxy, &xml);
            return true;
        }
        // Always answer Introspect — an unanswered call hangs the caller
        let req = call.path.as_deref().unwrap_or("/");
        let xml = if req == path {
            introspect_node(&[interface_xml, STD_INTERFACES_XML])
        } else if let Some(child) = introspect_child(path, req) {
            introspect_node(&[&format!("  <node name=\"{child}\"/>\n")])
        } else {
            // Unrelated path: a valid, empty node (no interfaces, no children).
            introspect_node(&[])
        };
        call.respond(proxy, &xml);
        return true;
    }
    false
}

/// If `req` is a proper ancestor of object path `object`, return the next path
/// segment from `req` toward `object` — what a caller introspecting `req`
/// should see as a child `<node>`. `None` if `req` is not on the way to
/// `object` (unrelated, partial segment, or at/below the object).
fn introspect_child(object: &str, req: &str) -> Option<String> {
    let rest = if req == "/" {
        object.strip_prefix('/')?
    } else {
        // require a path-separator boundary so "/org/exa" isn't an ancestor of
        // "/org/example/..."
        object.strip_prefix(req)?.strip_prefix('/')?
    };
    if rest.is_empty() {
        return None;
    }
    Some(rest.split('/').next()?.to_string())
}

/// One property access, handed to the closure of [`route_properties`].
pub enum PropAccess<'a> {
    /// `Properties.Get` for this property (also used per-name for `GetAll`).
    Get(&'a str),
    /// `Properties.Set` of this property to this value.
    Set(&'a str, &'a OwnedValue),
}

/// The closure's verdict on a [`PropAccess`].
pub enum PropReply {
    /// Get: here is the value.
    Value(OwnedValue),
    /// Set: accepted (and applied).
    Set,
    /// Set: this property is read-only.
    ReadOnly,
    /// No such property (Get or Set).
    Unknown,
    /// Set: value rejected, with a reason for the InvalidArgs error.
    Invalid(String),
}

/// Serve `org.freedesktop.DBus.Properties` (`Get`/`GetAll`/`Set`) for one
/// interface with a single closure, so module state is borrowed exactly once.
/// `names` lists the properties (drives `GetAll`). Returns `true` if the
/// message was one of the three calls and has been answered.
///
/// Since the closure usually needs `&mut` module state *and* the state owns the
/// proxy, clone the proxy first (it's an `Rc` handle):
/// `let proxy = s.proxy.clone();`
pub fn route_properties<B: Bus>(
    proxy: &DbusProxy<B>,
    msg: &DbusMessage,
    iface: &str,
    names: &[&str],
    mut f: impl FnMut(PropAccess<'_>) -> PropReply,
) -> bool {
    if let Some(Ok(call)) = IncomingCall::<PropertiesGet>::try_from(msg) {
        let (req_iface, prop) = &call.args;
        if req_iface != iface {
            call.error(proxy, ERR_UNKNOWN_INTERFACE, "no such interface");
        } else {
            match f(PropAccess::Get(prop)) {
                PropReply::Value(v) => {
                    call.respond(proxy, &v);
                }
                _ => {
                    call.error(proxy, ERR_UNKNOWN_PROPERTY, "no such property");
                }
            }
        }
        return true;
    }
    if let Some(Ok(call)) = IncomingCall::<PropertiesGetAll>::try_from(msg) {
        let (req_iface,) = &call.args;
        if req_iface != iface {
            call.error(proxy, ERR_UNKNOWN_INTERFACE, "no such interface");
        } else {
            let mut all = std::collections::HashMap::new();
            for name in names {
                if let PropReply::Value(v) = f(PropAccess::Get(name)) {
                    all.insert(name.to_string(), v);
                }
            }
            call.respond(proxy, &all);
        }
        return true;
    }
    if let Some(Ok(call)) = IncomingCall::<PropertiesSet>::try_from(msg) {
        let (req_iface, prop, value) = &call.args;
        if req_iface != iface {
            call.error(proxy, ERR_UNKNOWN_INTERFACE, "no such interface");
        } else {
            match f(PropAccess::Set(prop, value)) {
                PropReply::Set => {
                    call.respond(proxy, &());
                }
                PropReply::ReadOnly => {
                    call.error(proxy, ERR_PROPERTY_READ_ONLY, "property is read-only");
                }
                PropReply::Invalid(reason) => {
                    call.error(proxy, ERR_INVALID_ARGS, &reason);
                }
                _ => {
                    call.error(proxy, ERR_UNKNOWN_PROPERTY, "no such property");
                }
            }
        }
        return true;
    }
    false
}
