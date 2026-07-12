use app::{RegisteredModule, prelude::*};
use dbus::{
    DbusConnection, DbusEvent, DbusProxy, DbusSignal, Pending, SignalMatch, Subscription,
    SystemBus, dbus_method, dbus_signal, module as dbus_module, prop,
};
use io_ring::{Ring, RingSettings};
use std::collections::HashMap;

use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const NM_SERVICE: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_IFACE: &str = "org.freedesktop.NetworkManager";
const NM_DEVICE_IFACE: &str = "org.freedesktop.NetworkManager.Device";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

dbus_method!(GetDevices {
    dest: NM_SERVICE, path: NM_PATH, iface: NM_IFACE, member: "GetDevices",
    args: (), reply: Vec<OwnedObjectPath>,
});

dbus_method!(GetProps {
    dest: NM_SERVICE, path: "", iface: PROPS_IFACE, member: "GetAll",
    args: (&'static str,), reply: HashMap<String, OwnedValue>,
});

dbus_signal!(StateChanged {
    iface: NM_DEVICE_IFACE,
    member: "StateChanged",
    args: (u32, u32, u32),
});

dbus_signal!(DeviceAdded {
    iface: NM_IFACE,
    member: "DeviceAdded",
    args: (OwnedObjectPath,),
});
dbus_signal!(DeviceRemoved {
    iface: NM_IFACE,
    member: "DeviceRemoved",
    args: (OwnedObjectPath,),
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Unknown,
    Unmanaged,
    Unavailable,
    Disconnected,
    Preparing,
    NeedAuth,
    IpConfig,
    Activated,
    Deactivating,
    Failed,
    Other(u32),
}

impl DeviceState {
    fn from_u32(v: u32) -> Self {
        match v {
            10 => Self::Unmanaged,
            20 => Self::Unavailable,
            30 => Self::Disconnected,
            40 | 50 => Self::Preparing,
            60 => Self::NeedAuth,
            70 | 80 | 90 => Self::IpConfig,
            100 => Self::Activated,
            110 => Self::Deactivating,
            120 => Self::Failed,
            0 => Self::Unknown,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug)]
pub struct DeviceStateChanged {
    pub path: String,
    pub interface_name: Option<String>, // e.g. "wlan0"
    pub old_state: DeviceState,
    pub new_state: DeviceState,
    pub reason: u32,
}
impl Event for DeviceStateChanged {}

#[derive(Debug, Default, Clone)]
struct DeviceInfo {
    interface_name: Option<String>,
    state: DeviceState,
}
impl Default for DeviceState {
    fn default() -> Self {
        DeviceState::Unknown
    }
}

#[derive(State)]
pub struct NetworkManager {
    proxy: DbusProxy<SystemBus>,
    devices: HashMap<String, DeviceInfo>,
    get_devices: Pending<GetDevices>,
    device_props: Pending<GetProps, String>,
    watches: HashMap<String, Subscription>,
}

impl NetworkManager {
    pub fn new(proxy: DbusProxy<SystemBus>) -> Self {
        Self {
            proxy,
            get_devices: Pending::new(),
            device_props: Pending::new(),
            devices: HashMap::new(),
            watches: HashMap::new(),
        }
    }

    /// Initialize the NetworkManager client
    fn bootstrap(&mut self) {
        // Subscribe globally till this process is running
        // i.e. no need to unsubscribe
        self.proxy.subscribe::<DeviceAdded>();
        self.proxy.subscribe::<DeviceRemoved>();

        // Trigger GetDevices to fetch all active devices (interfaces)
        self.get_devices.call(&self.proxy, &(), ());
    }

    // fn request_device_props(&mut self, path: &str) {
    //     self.proxy.call_at::<GetProps>(path, &(NM_DEVICE_IFACE,));
    //     self.device_props
    //         .call_at(&self.proxy, path, &(NM_DEVICE_IFACE,), path.to_string());
    // }

    /// Track a device, configure its subscriptions and trigger all props
    fn track_device(&mut self, path: &str) {
        if self.watches.contains_key(path) {
            // already being tracked
            return;
        }

        self.devices.entry(path.to_string()).or_default();

        // Listen to StateChanged for every device
        let sub = self
            .proxy
            .subscribe_rule(StateChanged::match_rule().path(path));

        self.watches.insert(path.to_string(), sub);

        // Trigger get all properties for this device (interface)
        self.device_props
            .call_at(&self.proxy, path, &(NM_DEVICE_IFACE,), path.to_string());
    }

    /// Stop tracking a device, unsubscribes its watch
    fn untrack_device(&mut self, path: &str) {
        if let Some(sub) = self.watches.remove(path) {
            self.proxy.unsubscribe(&sub);
        }
        self.devices.remove(path);
    }
}

pub fn nm_module<S>() -> impl RegisteredModule<NetworkManager, S> {
    Module::<NetworkManager, _, _>::new()
        .on(|nm: &mut NetworkManager, _: &app::Start| nm.bootstrap())
        .on(
            |nm: &mut NetworkManager, ev: &DbusEvent<SystemBus>| -> Option<DeviceStateChanged> {
                // 1. GetDevices
                if let Some((_, result)) = nm.get_devices.resolve(&ev.msg) {
                    match result {
                        Ok(paths) => {
                            println!("NetworkManager: {} device(s)", paths.len());
                            for p in paths {
                                nm.track_device(p.as_str());
                            }
                        }
                        Err(e) => eprintln!("GetDevices failed: {e}"),
                    }
                    return None;
                }

                // 2. GetAll properties (for each device)
                if let Some((path, result)) = nm.device_props.resolve(&ev.msg) {
                    if let Ok(props) = result {
                        let iface = prop::<String>(&props, "Interface");
                        let state = prop::<u32>(&props, "State")
                            .map(DeviceState::from_u32)
                            .unwrap_or(DeviceState::Unknown);
                        println!(
                            "{} ({}) -> {:?}",
                            path,
                            iface.as_deref().unwrap_or("?"),
                            state
                        );
                        let entry = nm.devices.entry(path).or_default();
                        entry.interface_name = iface;
                        entry.state = state;
                    }
                    return None;
                }

                // 3. DeviceAdded signal -> start watching the new device.
                if let Some(Ok(sig)) = SignalMatch::<DeviceAdded>::try_from(&ev.msg) {
                    let (device,) = sig.args;
                    let path = device.as_str().to_string();
                    println!("device added: {path}");
                    nm.track_device(&path);
                    return None;
                }

                // 4. DeviceRemoved signal -> unsubscribe that device's watch.
                if let Some(Ok(sig)) = SignalMatch::<DeviceRemoved>::try_from(&ev.msg) {
                    let (device,) = sig.args;
                    let path = device.as_str().to_string();
                    println!("device removed: {path}");
                    nm.untrack_device(&path);
                    return None;
                }

                // 5. StateChanged signal.
                if let Some(Ok(sig)) = SignalMatch::<StateChanged>::try_from(&ev.msg) {
                    let path = sig.path.unwrap_or_default();
                    let (new_s, old_s, reason) = sig.args;
                    let new_state = DeviceState::from_u32(new_s);

                    let entry = nm.devices.entry(path.clone()).or_default();
                    entry.state = new_state;
                    let interface_name = entry.interface_name.clone();

                    return Some(DeviceStateChanged {
                        path,
                        interface_name,
                        old_state: DeviceState::from_u32(old_s),
                        new_state,
                        reason,
                    });
                }

                None
            },
        )
        .on(|_: &mut NetworkManager, e: &DeviceStateChanged| {
            println!(
                "[state] {} {:?} -> {:?} (reason {})",
                e.interface_name.as_deref().unwrap_or(&e.path),
                e.old_state,
                e.new_state,
                e.reason,
            );
        })
}

#[derive(State)]
pub struct AppRoot {
    network_manager: NetworkManager,
    dbus_conn: DbusConnection<SystemBus>,
    ring: Ring,
}

fn main() {
    let ring = Ring::new(RingSettings::default());
    let dbus_conn = DbusConnection::<SystemBus>::new(ring.proxy());
    let network_manager = NetworkManager::new(dbus_conn.proxy());

    let root = AppRoot {
        network_manager,
        dbus_conn,
        ring,
    };

    let mut app = App::new(root)
        .mount(nm_module())
        .mount(dbus_module::<SystemBus, _>())
        .mount(io_ring::module());
    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}
