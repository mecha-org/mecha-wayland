/// Declare a struct implementing [`DbusMethod`].
///
/// ```ignore
/// dbus_method!(GetDevices {
///     dest: "org.freedesktop.NetworkManager",
///     path: "/org/freedesktop/NetworkManager",
///     iface: "org.freedesktop.NetworkManager",
///     member: "GetDevices",
///     args: (), reply: Vec<OwnedObjectPath>,
/// });
/// ```
/// A leading visibility works too: `dbus_method!(pub GetDevices { … });`
#[macro_export]
macro_rules! dbus_method {
    ($vis:vis $name:ident {
        dest: $dest:expr,
        path: $path:expr,
        iface: $iface:expr,
        member: $member:expr,
        args: $args:ty,
        reply: $reply:ty $(,)?
    }) => {
        $vis struct $name;
        impl $crate::DbusMethod for $name {
            const DESTINATION: &'static str = $dest;
            const PATH: &'static str = $path;
            const INTERFACE: &'static str = $iface;
            const MEMBER: &'static str = $member;
            type Args = $args;
            type Reply = $reply;
        }
    };
}

/// Declare a struct implementing [`DbusSignal`].
///
/// ```ignore
/// dbus_signal!(StateChanged {
///     iface: "org.freedesktop.NetworkManager.Device",
///     member: "StateChanged",
///     args: (u32, u32, u32),
/// });
/// ```
#[macro_export]
macro_rules! dbus_signal {
    ($vis:vis $name:ident {
        iface: $iface:expr,
        member: $member:expr,
        args: $args:ty $(,)?
    }) => {
        $vis struct $name;
        impl $crate::DbusSignal for $name {
            const INTERFACE: &'static str = $iface;
            const MEMBER: &'static str = $member;
            type Args = $args;
        }
    };
}

/// Declare a struct implementing [`DbusHandler`] — a method your service serves.
///
/// ```ignore
/// dbus_handler!(GetValue {
///     iface: "org.example.Widget",
///     member: "GetValue",
///     args: (), ret: u32,
/// });
/// ```
#[macro_export]
macro_rules! dbus_handler {
    ($vis:vis $name:ident {
        iface: $iface:expr,
        member: $member:expr,
        args: $args:ty,
        ret: $ret:ty $(,)?
    }) => {
        $vis struct $name;
        impl $crate::DbusHandler for $name {
            const INTERFACE: &'static str = $iface;
            const MEMBER: &'static str = $member;
            type Args = $args;
            type Ret = $ret;
        }
    };
}

/// Declare an interface: generates a `DbusHandler` per method, a
/// `DbusSignal` per signal, and an `introspect()` method that builds the
/// `<interface>` XML — with each arg's D-Bus signature derived from its Rust
/// type via `zbus::zvariant::Type`, so the handlers and the XML can't drift.
///
/// ```ignore
/// dbus_interface!(pub Widget = "org.example.Widget";
///     method GetValue() -> (value: u32);
///     method SetValue(value: u32) -> ();
///     signal ValueChanged(value: u32);
///     property Value: u32, readwrite;
/// );
/// ```
#[macro_export]
macro_rules! dbus_interface {
    (
        $ivis:vis $iname:ident = $iface:expr ;
        $(
            method $mname:ident ( $( $an:ident : $at:ty ),* $(,)? )
                -> ( $( $rn:ident : $rt:ty ),* $(,)? ) ;
        )*
        $(
            signal $sname:ident ( $( $sn:ident : $st:ty ),* $(,)? ) ;
        )*
        $(
            property $pname:ident : $pty:ty , $paccess:ident ;
        )*
    ) => {
        $ivis struct $iname;
        impl $iname {
            pub const INTERFACE: &'static str = $iface;

            /// The `<interface>` introspection node for this interface.
            pub fn introspect() -> ::std::string::String {
                let mut s = ::std::string::String::new();
                s.push_str(&::std::format!("  <interface name=\"{}\">\n", $iface));
                $(
                    s.push_str(&::std::format!(
                        "    <method name=\"{}\">\n", ::core::stringify!($mname)));
                    $(
                        s.push_str(&::std::format!(
                            "      <arg name=\"{}\" type=\"{}\" direction=\"in\"/>\n",
                            ::core::stringify!($an),
                            <$at as $crate::zbus::zvariant::Type>::SIGNATURE));
                    )*
                    $(
                        s.push_str(&::std::format!(
                            "      <arg name=\"{}\" type=\"{}\" direction=\"out\"/>\n",
                            ::core::stringify!($rn),
                            <$rt as $crate::zbus::zvariant::Type>::SIGNATURE));
                    )*
                    s.push_str("    </method>\n");
                )*
                $(
                    s.push_str(&::std::format!(
                        "    <signal name=\"{}\">\n", ::core::stringify!($sname)));
                    $(
                        s.push_str(&::std::format!(
                            "      <arg name=\"{}\" type=\"{}\"/>\n",
                            ::core::stringify!($sn),
                            <$st as $crate::zbus::zvariant::Type>::SIGNATURE));
                    )*
                    s.push_str("    </signal>\n");
                )*
                $(
                    s.push_str(&::std::format!(
                        "    <property name=\"{}\" type=\"{}\" access=\"{}\"/>\n",
                        ::core::stringify!($pname),
                        <$pty as $crate::zbus::zvariant::Type>::SIGNATURE,
                        ::core::stringify!($paccess)));
                )*
                s.push_str("  </interface>\n");
                s
            }

            /// Answer the standard object interfaces — `Peer.Ping`,
            /// `Peer.GetMachineId` (any path) and `Introspectable.Introspect`
            /// (for `path` only) — using this interface's derived XML. The
            /// Properties interface is advertised iff the declaration has
            /// `property` lines.
            pub fn handle_standard<B: $crate::Bus>(
                proxy: &$crate::DbusProxy<B>,
                path: &str,
                msg: &$crate::DbusMessage,
            ) -> bool {
                let props: &[&str] = &[$(stringify!($pname)),*];
                $crate::fdo::handle_standard(
                    proxy,
                    path,
                    &Self::introspect(),
                    !props.is_empty(),
                    msg,
                )
            }
        }

        $(
            $ivis struct $mname;
            impl $crate::DbusHandler for $mname {
                const INTERFACE: &'static str = $iface;
                const MEMBER: &'static str = ::core::stringify!($mname);
                type Args = ( $( $at, )* );
                type Ret = ( $( $rt, )* );
            }
        )*
        $(
            $ivis struct $sname;
            impl $crate::DbusSignal for $sname {
                const INTERFACE: &'static str = $iface;
                const MEMBER: &'static str = ::core::stringify!($sname);
                type Args = ( $( $st, )* );
            }
        )*
    };
}
