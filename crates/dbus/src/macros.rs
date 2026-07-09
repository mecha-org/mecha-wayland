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
