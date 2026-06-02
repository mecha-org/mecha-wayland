// cargo run -p app --example propagation --release
// objdump: cargo build -p app --example propagation --release &&
//          objdump -d target/release/examples/propagation | less
use app::prelude::*;

// ── Events ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct Tick;
impl Event for Tick {}

#[derive(Debug)]
struct BatteryLow;
impl Event for BatteryLow {}

#[derive(Debug)]
struct BatteryCritical;
impl Event for BatteryCritical {}

#[derive(Debug)]
struct NetworkLost;
impl Event for NetworkLost {}

#[derive(Debug)]
struct NetworkRestored;
impl Event for NetworkRestored {}

#[derive(Debug)]
struct Notify(pub &'static str);
impl Event for Notify {}

#[derive(Debug)]
struct Render;
impl Event for Render {}

// ── State ─────────────────────────────────────────────────────────────────────

struct BatteryAlerts {
    muted: bool,
}

struct Battery {
    level: u8,
    warned_low: bool,
    warned_critical: bool,
    alerts: BatteryAlerts,
}

struct Network {
    connected: bool,
    reconnect_attempts: u32,
    dropped: bool,
}

struct NotificationQueue {
    pending: [&'static str; 4],
    pending_len: usize,
    displayed: u32,
}

struct AppState {
    battery: Battery,
    network: Network,
    notifications: NotificationQueue,
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let state = AppState {
        battery: Battery {
            level: 30,
            warned_low: false,
            warned_critical: false,
            alerts: BatteryAlerts { muted: false },
        },
        network: Network {
            connected: true,
            reconnect_attempts: 0,
            dropped: false,
        },
        notifications: NotificationQueue {
            pending: [""; 4],
            pending_len: 0,
            displayed: 0,
        },
    };

    // Mount order matters: emitted events propagate to modules that sit deeper
    // in the HList (mounted earlier). Notifications is mounted first so battery
    // and network can reach it when they emit Notify.
    let mut app = App::new(state)
        .mount(|s: &mut AppState| &mut s.notifications, {
            Module::new()
                // Every Notify queues a message and triggers an immediate Render.
                .on(|s: &mut NotificationQueue, n: &Notify| {
                    println!("[notify] queued: \"{}\"", n.0);
                    s.pending[s.pending_len] = n.0;
                    s.pending_len += 1;
                    Render
                })
                .on(|s: &mut NotificationQueue, _: &Render| {
                    for msg in &s.pending[..s.pending_len] {
                        s.displayed += 1;
                        println!("[render] #{}: {}", s.displayed, msg);
                    }
                    s.pending_len = 0;
                })
        })
        .mount(|s: &mut AppState| &mut s.network, {
            Module::new()
                // On the first Tick the network drops. Subsequent NetworkLost
                // events retry until reconnected, then emit NetworkRestored.
                .on(|s: &mut Network, _: &Tick| -> Option<NetworkLost> {
                    if !s.dropped {
                        s.dropped = true;
                        s.connected = false;
                        println!("[network] connection dropped");
                        Some(NetworkLost)
                    } else {
                        None
                    }
                })
                // Re-emits NetworkLost until enough retries accumulate, then
                // switches to NetworkRestored. This creates a self-driven retry
                // chain entirely within one propagation pass.
                .on(|s: &mut Network, _: &NetworkLost| {
                    s.reconnect_attempts += 1;
                    println!("[network] reconnect attempt #{}", s.reconnect_attempts);
                    if s.reconnect_attempts >= 3 {
                        s.connected = true;
                        s.reconnect_attempts = 0;
                        println!("[network] reconnected");
                        hlist![None::<NetworkLost>, Some(NetworkRestored)]
                    } else {
                        hlist![Some(NetworkLost), None::<NetworkRestored>]
                    }
                })
                .on(|_: &mut Network, _: &NetworkRestored| {
                    println!("[network] stable — emitting notification");
                    Notify("Network restored")
                })
        })
        .mount(|s: &mut AppState| &mut s.battery, {
            Module::new()
                // Tick drains battery. Uses hlist to emit BatteryLow and
                // BatteryCritical independently in one handler — either, both,
                // or neither can fire on any given tick.
                .on(|s: &mut Battery, _: &Tick| {
                    s.level = s.level.saturating_sub(3);
                    println!("[battery] tick — level={}", s.level);
                    hlist![
                        if s.level <= 20 && s.level > 0 && !s.warned_low {
                            s.warned_low = true;
                            Some(BatteryLow)
                        } else {
                            None
                        },
                        if s.level == 0 && !s.warned_critical {
                            s.warned_critical = true;
                            Some(BatteryCritical)
                        } else {
                            None
                        },
                    ]
                })
                // BatteryAlerts submodule: owns the warning responses so the
                // parent Battery module only needs to track level/flags.
                .mount(|s: &mut Battery| &mut s.alerts, {
                    Module::new()
                        .on(|s: &mut BatteryAlerts, _: &BatteryLow| {
                            if s.muted {
                                println!("[alerts] LOW suppressed (muted)");
                                None
                            } else {
                                println!("[alerts] LOW handler fired");
                                Some(Notify("Battery low — please plug in"))
                            }
                        })
                        .on(|s: &mut BatteryAlerts, _: &BatteryCritical| {
                            s.muted = true;
                            println!("[alerts] CRITICAL handler fired — muting further alerts");
                            Notify("Battery critical — suspending soon")
                        })
                })
        });

    println!("=== simulation start ===\n");
    for i in 1..=12u64 {
        println!("─── tick {} ───────────────────────────", i);
        app.dispatch(&Tick);
        println!();
    }

    let s = app.state();
    println!("=== final state ===");
    println!("battery level        : {}", s.battery.level);
    println!("network connected    : {}", s.network.connected);
    println!("notifications shown  : {}", s.notifications.displayed);
}
