// The session lock, over the fake compositor. Every test needs the GPU:
// `fake()` returns `None` and prints a skip line without one.

use std::sync::{Mutex, MutexGuard};

use animation::AnimationModule;
use app::prelude::*;
use atlas::Atlas;
use gles::{Budget, Device, Error};
use interactivity::prelude::*;
use layout::prelude::*;
use paint::prelude::*;
use presentation::prelude::*;
use render::prelude::*;
use wayland::fake::Fake;
use wayland::prelude::*;
use window::prelude::*;

const GLOBALS: &[(&str, u32)] = &[
    ("wl_compositor", 6),
    ("zwp_linux_dmabuf_v1", 3),
    ("xdg_wm_base", 7),
    ("zwlr_layer_shell_v1", 5),
    ("wl_seat", 9),
    ("ext_session_lock_manager_v1", 1),
    ("wl_output", 4),
];

// Ids: registry 2, sync 3, compositor 4, dmabuf 5, wm_base 6, seat 7,
// layer shell 8, lock manager 9 (bound by presentation's install), output
// 10 (by `OutputsModule`'s). A lock is 11; its first window's surface 12
// and lock surface 13.
const COMPOSITOR: u32 = 4;
const MANAGER: u32 = 9;
const OUTPUT: u32 = 10;
const LOCK: u32 = 11;
const SURFACE: u32 = 12;
const LOCK_SURFACE: u32 = 13;

mod op {
    pub const CREATE_SURFACE: u16 = 0;
    pub const SURFACE_DESTROY: u16 = 0;
    pub const MANAGER_LOCK: u16 = 1;
    pub const LOCK_DESTROY: u16 = 0;
    pub const GET_LOCK_SURFACE: u16 = 1;
    pub const UNLOCK_AND_DESTROY: u16 = 2;
    pub const LOCK_SURFACE_DESTROY: u16 = 0;
    pub const LOCK_SURFACE_ACK: u16 = 1;
}
mod ev {
    pub const OUTPUT_MODE: u16 = 1;
    pub const OUTPUT_DONE: u16 = 2;
    pub const OUTPUT_SCALE: u16 = 3;
    pub const OUTPUT_NAME: u16 = 4;
    pub const LOCKED: u16 = 0;
    pub const FINISHED: u16 = 1;
    pub const LOCK_CONFIGURE: u16 = 0;
}

#[derive(Default)]
struct Log(Vec<String>);
impl Resource for Log {}

fn log_locked(app: &mut App, _: &SessionLocked) {
    app.resource_mut::<Log>().0.push("locked".into());
}
fn log_finished(app: &mut App, _: &SessionLockFinished) {
    app.resource_mut::<Log>().0.push("finished".into());
}
fn log_resized(app: &mut App, e: &Emitted<Resized>) {
    let s = format!("resized {}x{}", e.event.size.width, e.event.size.height);
    app.resource_mut::<Log>().0.push(s);
}

static GPU: Mutex<()> = Mutex::new(());

fn fake() -> Option<(MutexGuard<'static, ()>, Fake)> {
    let guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    match Device::try_open(Budget::default()) {
        Ok(_) => {}
        Err(Error::NoDevice) => {
            eprintln!("skip: no render node");
            return None;
        }
        Err(e) => panic!("the device did not open: {e:?}"),
    }
    let mut f = Fake::new(GLOBALS, |m| {
        m.bind::<WlCompositor>()
            .bind::<ZwpLinuxDmabufV1>()
            .bind::<XdgWmBase>()
            .bind::<WlSeat>()
    });
    f.app
        .add_module(LayoutModule)
        .add_module(PaintModule)
        .add_module(WindowModule)
        .add_module(InteractivityModule)
        .add_module(AnimationModule)
        .add_module(RenderModule::default())
        .insert_resource(Atlas::new());
    f.app
        .init_resource::<Log>()
        .system(log_locked)
        .system(log_finished)
        .system(log_resized)
        .add_module(PresentationModule {
            app_id: "test".into(),
            budget: Budget::default(),
        })
        .add_module(OutputsModule);
    f.turn();
    f.requests();
    Some((guard, f))
}

fn log(f: &mut Fake) -> Vec<String> {
    std::mem::take(&mut f.app.resource_mut::<Log>().0)
}

fn shape(f: &mut Fake) -> Vec<(u32, u16)> {
    f.requests()
        .iter()
        .map(|r| (r.sender.0, r.opcode))
        .collect()
}

/// The output's first state, settled by `done`.
fn announce(f: &mut Fake) {
    f.send(OUTPUT, ev::OUTPUT_MODE, |w| {
        w.uint(1);
        w.int(1080);
        w.int(1240);
        w.int(60_000);
    });
    f.send(OUTPUT, ev::OUTPUT_SCALE, |w| w.int(2));
    f.send(OUTPUT, ev::OUTPUT_NAME, |w| w.string("DSI-1"));
    f.send(OUTPUT, ev::OUTPUT_DONE, |_| {});
    f.turn();
}

fn lock(f: &mut Fake) {
    let ok = {
        let (mut session, mut wl) = f.app.query::<(ResMut<SessionLock>, ResMut<Wayland>)>();
        session.lock(&mut wl)
    };
    assert!(ok, "the lock was asked for");
    f.turn();
}

fn unlock(f: &mut Fake) {
    {
        let (mut session, mut wl) = f.app.query::<(ResMut<SessionLock>, ResMut<Wayland>)>();
        session.unlock(&mut wl);
    }
    f.turn();
}

fn spawn_lock_window(f: &mut Fake) -> NodeId {
    let root = f.app.root();
    let win = f
        .app
        .spawn_with(
            root,
            window().layout(LayoutStyle::default().column()),
            (Role::Lock(WlOutput(ObjectId(OUTPUT))),),
        )
        .id();
    f.app.tick();
    f.turn();
    win
}

fn state(f: &Fake) -> LockState {
    f.app.resource::<SessionLock>().state()
}

#[test]
fn a_lock_window_gets_a_lock_surface_on_its_output() {
    let Some((_gpu, mut f)) = fake() else { return };
    announce(&mut f);
    assert!(f.app.resource::<SessionLock>().available());
    lock(&mut f);
    assert_eq!(shape(&mut f), vec![(MANAGER, op::MANAGER_LOCK)]);
    assert_eq!(state(&f), LockState::Locking);
    spawn_lock_window(&mut f);
    let reqs = f.requests();
    let got: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    assert_eq!(
        got,
        vec![
            (COMPOSITOR, op::CREATE_SURFACE),
            (LOCK, op::GET_LOCK_SURFACE)
        ],
        "no commit before the first configure is acked"
    );
    let mut r = reqs[1].reader();
    assert_eq!(r.object(), Some(ObjectId(LOCK_SURFACE)));
    assert_eq!(r.object(), Some(ObjectId(SURFACE)));
    assert_eq!(r.object(), Some(ObjectId(OUTPUT)));
}

#[test]
fn a_lock_surface_configure_is_acked_and_sizes_the_window() {
    let Some((_gpu, mut f)) = fake() else { return };
    lock(&mut f);
    spawn_lock_window(&mut f);
    f.requests();
    log(&mut f);
    f.send(LOCK_SURFACE, ev::LOCK_CONFIGURE, |w| {
        w.uint(5);
        w.uint(540);
        w.uint(620);
    });
    f.turn();
    let ack = f.expect(LOCK_SURFACE, op::LOCK_SURFACE_ACK);
    assert_eq!(ack.reader().uint(), Some(5));
    assert_eq!(log(&mut f), vec!["resized 540x620".to_string()]);
}

#[test]
fn locked_is_reported_and_unlock_unlocks_and_destroys() {
    let Some((_gpu, mut f)) = fake() else { return };
    lock(&mut f);
    spawn_lock_window(&mut f);
    f.requests();
    f.send(LOCK, ev::LOCKED, |_| {});
    f.turn();
    assert_eq!(log(&mut f), vec!["locked".to_string()]);
    assert_eq!(state(&f), LockState::Locked);
    unlock(&mut f);
    assert_eq!(shape(&mut f), vec![(LOCK, op::UNLOCK_AND_DESTROY)]);
    assert_eq!(state(&f), LockState::Unlocked);
}

#[test]
fn an_unlock_before_locked_waits_for_it_then_unlocks_unreported() {
    let Some((_gpu, mut f)) = fake() else { return };
    lock(&mut f);
    f.requests();
    unlock(&mut f);
    assert!(
        shape(&mut f).is_empty(),
        "no destroy: locked may be in flight"
    );
    assert_eq!(state(&f), LockState::Locking);
    {
        let (mut session, mut wl) = f.app.query::<(ResMut<SessionLock>, ResMut<Wayland>)>();
        assert!(!session.lock(&mut wl), "still waiting for the answer");
    }
    f.send(LOCK, ev::LOCKED, |_| {});
    f.turn();
    assert_eq!(shape(&mut f), vec![(LOCK, op::UNLOCK_AND_DESTROY)]);
    assert!(log(&mut f).is_empty());
    assert_eq!(state(&f), LockState::Unlocked);
}

#[test]
fn an_unlock_before_finished_destroys_unreported() {
    let Some((_gpu, mut f)) = fake() else { return };
    lock(&mut f);
    f.requests();
    unlock(&mut f);
    f.send(LOCK, ev::FINISHED, |_| {});
    f.turn();
    assert_eq!(shape(&mut f), vec![(LOCK, op::LOCK_DESTROY)]);
    assert!(log(&mut f).is_empty());
    assert_eq!(state(&f), LockState::Unlocked);
}

#[test]
fn finished_unasked_is_reported_and_destroys_the_lock() {
    let Some((_gpu, mut f)) = fake() else { return };
    lock(&mut f);
    f.requests();
    f.send(LOCK, ev::FINISHED, |_| {});
    f.turn();
    assert_eq!(shape(&mut f), vec![(LOCK, op::LOCK_DESTROY)]);
    assert_eq!(log(&mut f), vec!["finished".to_string()]);
    assert_eq!(state(&f), LockState::Unlocked);
    // A new lock can be asked for.
    lock(&mut f);
    assert_eq!(shape(&mut f), vec![(MANAGER, op::MANAGER_LOCK)]);
}

#[test]
fn despawning_a_lock_window_destroys_its_lock_surface_then_its_surface() {
    let Some((_gpu, mut f)) = fake() else { return };
    lock(&mut f);
    let win = spawn_lock_window(&mut f);
    f.send(LOCK, ev::LOCKED, |_| {});
    f.turn();
    unlock(&mut f);
    f.requests();
    f.app.remove(win);
    f.app.tick();
    f.turn();
    assert_eq!(
        shape(&mut f),
        vec![
            (LOCK_SURFACE, op::LOCK_SURFACE_DESTROY),
            (SURFACE, op::SURFACE_DESTROY),
        ]
    );
}

#[test]
#[should_panic(expected = "call SessionLock::lock first")]
fn a_lock_window_without_a_lock_panics() {
    let Some((_gpu, mut f)) = fake() else {
        panic!("call SessionLock::lock first (skipped: no render node)")
    };
    spawn_lock_window(&mut f);
}
