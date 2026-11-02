// The fixture below (ids, opcodes, event opcodes, `configured`, `attaches`,
// `log`, the logging systems) serves every section below. Every test
// needs the GPU: `fake()` returns `None` and prints a skip line without one.

use std::sync::{Mutex, MutexGuard};

use animation::AnimationModule;
use app::prelude::*;
use atlas::{Atlas, Rescaled};
use geometry::{Color, Rect};
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
];

// Deterministic ids: registry 2, sync callback 3, compositor 4, dmabuf 5,
// wm_base 6, seat 7, layer shell 8 (bound by presentation's install). The
// first window: surface 9, xdg surface 10, toplevel 11 (or layer surface
// 10). Its slots after configure: params 12 and buffer 13, params 14 and
// buffer 15; the first frame callback is 16. Ids are never reused: the
// fake sends no `delete_id`.
const DMABUF: u32 = 5;
const COMPOSITOR: u32 = 4;
const SEAT: u32 = 7;
const SURFACE: u32 = 9;
const XDG: u32 = 10;
const TOPLEVEL: u32 = 11;
const PARAMS_A: u32 = 12;
const BUF_A: u32 = 13;
const PARAMS_B: u32 = 14;
const BUF_B: u32 = 15;
const CALLBACK: u32 = 16;

// A layer window allocates only surface 9 and layer surface 10 (no
// toplevel), so its first slot's params is 11 and buffer 12.
const LAYER_BUF_A: u32 = 12;

/// `DRM_FORMAT_XRGB8888`.
const XRGB: u32 = 0x3432_5258;

/// `DRM_FORMAT_ARGB8888`.
const ARGB: u32 = 0x3432_5241;

/// `I915_FORMAT_MOD_Y_TILED`, the second layout the fake advertises after
/// the linear one. A vendor modifier the local GBM may or may not be able
/// to produce, so the slots report whichever of the two it picked.
const VENDOR_MOD: u64 = 0x0100_0000_0000_0002;

// Opcodes, from the XML order.
mod op {
    pub const CREATE_SURFACE: u16 = 0;
    pub const CREATE_REGION: u16 = 1;
    pub const REGION_DESTROY: u16 = 0;
    pub const REGION_ADD: u16 = 1;
    pub const SET_INPUT_REGION: u16 = 5;
    pub const CREATE_PARAMS: u16 = 1;
    pub const PARAMS_DESTROY: u16 = 0;
    pub const PARAMS_ADD: u16 = 1;
    pub const PARAMS_CREATE_IMMED: u16 = 3;
    /// Both a buffer's and a target's teardown; used by the scale test
    /// and by the removal test.
    pub const BUFFER_DESTROY: u16 = 0;
    pub const SURFACE_DESTROY: u16 = 0;
    pub const ATTACH: u16 = 1;
    pub const FRAME: u16 = 3;
    pub const COMMIT: u16 = 6;
    pub const SET_BUFFER_SCALE: u16 = 8;
    pub const DAMAGE_BUFFER: u16 = 9;
    pub const GET_XDG_SURFACE: u16 = 2;
    pub const PONG: u16 = 3;
    pub const XDG_DESTROY: u16 = 0;
    pub const GET_TOPLEVEL: u16 = 1;
    pub const ACK_CONFIGURE: u16 = 4;
    pub const TOPLEVEL_DESTROY: u16 = 0;
    pub const SET_TITLE: u16 = 2;
    pub const SET_APP_ID: u16 = 3;
    pub const GET_LAYER_SURFACE: u16 = 0;
    pub const LAYER_SET_SIZE: u16 = 0;
    pub const LAYER_SET_ANCHOR: u16 = 1;
    pub const LAYER_SET_ZONE: u16 = 2;
    pub const LAYER_SET_KEYBOARD: u16 = 4;
    pub const LAYER_ACK: u16 = 6;
    pub const LAYER_SET_LAYER: u16 = 8;
    /// No test removes a layer window (only a toplevel's removal is
    /// covered), so this opcode names a request no test sends.
    #[allow(dead_code)]
    pub const LAYER_DESTROY: u16 = 7;
    pub const GET_POINTER: u16 = 0;
    pub const GET_TOUCH: u16 = 2;
}
mod ev {
    pub const PING: u16 = 0;
    pub const XDG_CONFIGURE: u16 = 0;
    pub const TOPLEVEL_CONFIGURE: u16 = 0;
    pub const CLOSE: u16 = 1;
    pub const LAYER_CONFIGURE: u16 = 0;
    pub const LAYER_CLOSED: u16 = 1;
    pub const DONE: u16 = 0;
    pub const RELEASE: u16 = 0;
    pub const PREFERRED_SCALE: u16 = 2;
    pub const DMABUF_MODIFIER: u16 = 1;
    pub const CAPABILITIES: u16 = 0;
    pub const POINTER_ENTER: u16 = 0;
    pub const POINTER_LEAVE: u16 = 1;
    pub const POINTER_MOTION: u16 = 2;
    pub const POINTER_BUTTON: u16 = 3;
    pub const TOUCH_DOWN: u16 = 0;
    pub const TOUCH_UP: u16 = 1;
    pub const TOUCH_MOTION: u16 = 2;
    pub const TOUCH_CANCEL: u16 = 4;
}

#[derive(Default)]
struct Log(Vec<String>);
/// The `time` of every `ContactInput`, in arrival order.
#[derive(Default)]
struct ContactTimes(Vec<u32>);
impl Resource for Log {}
impl Resource for ContactTimes {}
fn log_resized(app: &mut App, e: &Emitted<Resized>) {
    app.resource_mut::<Log>().0.push(format!(
        "Resized {}x{}",
        e.event.size.width, e.event.size.height
    ));
}
fn log_scale(app: &mut App, e: &Emitted<ScaleFactorChanged>) {
    app.resource_mut::<Log>()
        .0
        .push(format!("Scale {}", e.event.scale));
}
#[derive(Default)]
struct Rescales(Vec<Vec<NodeId>>);
impl Resource for Rescales {}
fn log_rescaled(app: &mut App, e: &Emitted<Rescaled>) {
    app.resource_mut::<Rescales>().0.push(e.targets.to_vec());
}
fn log_close(app: &mut App, e: &Emitted<CloseRequested>) {
    let target = e.targets.iter().next().copied();
    app.resource_mut::<Log>()
        .0
        .push(format!("Close {target:?}"));
}
fn log_frame(app: &mut App, f: &Frame) {
    app.resource_mut::<Log>().0.push(format!("Frame {:?}", f.0));
}
fn log_contact(app: &mut App, e: &ContactInput) {
    app.resource_mut::<ContactTimes>().0.push(e.time);
    app.resource_mut::<Log>().0.push(format!(
        "{:?} {:?} {:?} {}x{}",
        e.window, e.contact, e.phase, e.position.x, e.position.y
    ));
}
fn log_press(app: &mut App, e: &Emitted<Press>) {
    let target = e.targets.iter().next().copied();
    app.resource_mut::<Log>()
        .0
        .push(format!("Press {target:?}"));
}
fn log_release(app: &mut App, e: &Emitted<Release>) {
    let target = e.targets.iter().next().copied();
    app.resource_mut::<Log>()
        .0
        .push(format!("Release {target:?}"));
}
fn log_clicked(app: &mut App, e: &Emitted<Clicked>) {
    let target = e.targets.iter().next().copied();
    app.resource_mut::<Log>()
        .0
        .push(format!("Clicked {target:?}"));
}

static GPU: Mutex<()> = Mutex::new(());

/// The fake compositor with the whole stack installed, behind the GPU
/// lock, after it advertised two layouts for XRGB8888: linear first, then
/// [`VENDOR_MOD`], and linear for ARGB8888. GBM picks the first of the two
/// it can render into, so which one a target reports is the driver's
/// business, not the test's. `None` with a skip line when there is no
/// render node.
fn fake() -> Option<(MutexGuard<'static, ()>, Fake)> {
    fake_advertising(true, true)
}

/// [`fake`], with or without XRGB8888 and ARGB8888 among the formats
/// advertised. Neither is what a compositor bound at version 4 or above
/// sends: its formats come by feedback instead.
fn fake_advertising(xrgb: bool, argb: bool) -> Option<(MutexGuard<'static, ()>, Fake)> {
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
    if xrgb {
        f.send(DMABUF, ev::DMABUF_MODIFIER, |w| {
            w.uint(XRGB);
            w.uint(0);
            w.uint(0);
        });
        f.send(DMABUF, ev::DMABUF_MODIFIER, |w| {
            w.uint(XRGB);
            w.uint((VENDOR_MOD >> 32) as u32);
            w.uint(VENDOR_MOD as u32);
        });
    }
    if argb {
        f.send(DMABUF, ev::DMABUF_MODIFIER, |w| {
            w.uint(ARGB);
            w.uint(0);
            w.uint(0);
        });
    }
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
        .init_resource::<ContactTimes>()
        .init_resource::<Rescales>()
        .system(log_resized)
        .system(log_scale)
        .system(log_rescaled)
        .system(log_close)
        .system(log_frame)
        .system(log_contact)
        .system(log_press)
        .system(log_release)
        .system(log_clicked)
        .add_module(PresentationModule {
            app_id: "test".into(),
            budget: Budget::default(),
        });
    f.turn();
    f.requests();
    Some((guard, f))
}

/// The `(hi, lo)` a `zwp_linux_buffer_params_v1.add` carries, past the
/// plane index, the offset and the stride.
fn added_modifier(r: &wayland::fake::Request) -> (u32, u32) {
    let mut a = r.reader();
    let (_plane, _offset, _stride) = (a.uint(), a.uint(), a.uint());
    (a.uint().unwrap(), a.uint().unwrap())
}

/// Every `wl_surface.damage_buffer` of the drained requests, in order,
/// as `(x, y, width, height)`.
fn damage(reqs: &[wayland::fake::Request]) -> Vec<(i32, i32, i32, i32)> {
    reqs.iter()
        .filter(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::DAMAGE_BUFFER)
        .map(|r| {
            let mut d = r.reader();
            (
                d.int().unwrap(),
                d.int().unwrap(),
                d.int().unwrap(),
                d.int().unwrap(),
            )
        })
        .collect()
}

fn log(f: &Fake) -> Vec<String> {
    f.app.resource::<Log>().0.clone()
}

/// A toplevel of the given style, spawned, ticked and flushed to the fake.
fn spawn(f: &mut Fake, style: LayoutStyle) -> NodeId {
    let root = f.app.root();
    let win = f
        .app
        .spawn(root, window().title("hello").layout(style))
        .id();
    f.app.tick();
    f.turn();
    win
}

/// A toplevel configured to `width` by `height` and drawn: the configure
/// changes the layout, so the frame comes one tick later.
fn configured(f: &mut Fake, width: i32, height: i32) -> NodeId {
    let win = spawn(f, LayoutStyle::default().column());
    f.requests();
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(width);
        w.int(height);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(1));
    f.turn();
    f.app.tick();
    f.turn();
    win
}

fn attaches(f: &mut Fake) -> Vec<u32> {
    f.requests()
        .iter()
        .filter(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::ATTACH)
        .map(|r| r.reader().object().unwrap().0)
        .collect()
}

/// The `(sender, opcode)` shape of the requests, drained.
fn shape(f: &mut Fake) -> Vec<(u32, u16)> {
    f.requests()
        .iter()
        .map(|r| (r.sender.0, r.opcode))
        .collect()
}

/// Requests a `wl_pointer` and returns its id, read off the wire rather
/// than assumed, since which id it gets depends on what the test already
/// spawned.
fn pointer(f: &mut Fake) -> u32 {
    f.send(SEAT, ev::CAPABILITIES, |w| {
        w.uint(WlSeatCapability::POINTER.bits())
    });
    f.turn();
    f.expect(SEAT, op::GET_POINTER).reader().object().unwrap().0
}

/// Requests a `wl_touch` and returns its id, read off the wire.
fn touch(f: &mut Fake) -> u32 {
    f.send(SEAT, ev::CAPABILITIES, |w| {
        w.uint(WlSeatCapability::TOUCH.bits())
    });
    f.turn();
    f.expect(SEAT, op::GET_TOUCH).reader().object().unwrap().0
}

/// Changes the window's clear colour through a child quad, so render
/// requests a frame for a real change.
fn repaint(f: &mut Fake, win: NodeId) {
    struct Leaf;
    impl Build for Leaf {
        type Widget = Leaf;
    }
    impl Widget for Leaf {
        type Builder = Leaf;
        fn build(b: Leaf, _: Handle<Self>, _: &mut Spawner<'_, Self>) -> Self {
            b
        }
    }
    f.app.spawn_with(
        win,
        Leaf,
        (
            LayoutStyle::default().size(px(10.0), px(10.0)),
            Paint::Quad(Quad::new(Color::rgb(1.0, 0.0, 0.0))),
        ),
    );
    f.app.tick();
}

// ── spawn and roles ────────────────────────────────────────────────────

#[test]
fn a_toplevel_window_gets_a_surface_a_role_and_an_initial_commit() {
    let Some((_gpu, mut f)) = fake() else { return };
    spawn(&mut f, LayoutStyle::default().column());
    let reqs = f.requests();
    let shape: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    assert_eq!(
        shape,
        vec![
            (4, op::CREATE_SURFACE),
            (6, op::GET_XDG_SURFACE),
            (XDG, op::GET_TOPLEVEL),
            (TOPLEVEL, op::SET_TITLE),
            (TOPLEVEL, op::SET_APP_ID),
            (SURFACE, op::COMMIT),
        ]
    );
    assert_eq!(reqs[0].reader().object(), Some(ObjectId(SURFACE)));
    let mut r = reqs[1].reader();
    assert_eq!(
        (r.object(), r.object()),
        (Some(ObjectId(XDG)), Some(ObjectId(SURFACE)))
    );
    assert_eq!(reqs[3].reader().string().as_deref(), Some("hello"));
    assert_eq!(reqs[4].reader().string().as_deref(), Some("test"));
    let win = *f
        .app
        .resource::<Windows>()
        .iter()
        .collect::<Vec<_>>()
        .first()
        .unwrap();
    let surfaces = f.app.resource::<Surfaces>();
    assert_eq!(surfaces.surface_of(win), Some(WlSurface(ObjectId(SURFACE))));
    assert!(!surfaces.is_configured(win));
}

#[test]
fn a_layer_window_gets_a_layer_surface_with_its_role() {
    let Some((_gpu, mut f)) = fake() else { return };
    let root = f.app.root();
    let role = Role::Layer(LayerRole {
        layer: Layer::Top,
        anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        exclusive_zone: 32,
        namespace: "bar".into(),
        keyboard_interactivity: KeyboardInteractivity::None,
    });
    f.app.spawn_with(
        root,
        window().layout(LayoutStyle::default().column().size(auto(), px(32.0))),
        (role,),
    );
    f.app.tick();
    f.turn();
    let reqs = f.requests();
    let shape: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    assert_eq!(
        shape,
        vec![
            (4, op::CREATE_SURFACE),
            (8, op::GET_LAYER_SURFACE),
            (XDG, op::LAYER_SET_ANCHOR),
            (XDG, op::LAYER_SET_ZONE),
            (XDG, op::LAYER_SET_KEYBOARD),
            (XDG, op::LAYER_SET_SIZE),
            (SURFACE, op::COMMIT),
        ]
    );
    let mut r = reqs[1].reader();
    assert_eq!(r.object(), Some(ObjectId(XDG)));
    assert_eq!(r.object(), Some(ObjectId(SURFACE)));
    assert_eq!(r.object_opt(), Some(None), "no output");
    assert_eq!(r.uint(), Some(2), "top");
    assert_eq!(r.string().as_deref(), Some("bar"));
    assert_eq!(reqs[2].reader().uint(), Some(1 | 4 | 8));
    assert_eq!(reqs[3].reader().int(), Some(32));
    let mut r = reqs[5].reader();
    assert_eq!((r.uint(), r.uint()), (Some(0), Some(32)));
}

#[test]
fn a_ping_is_answered_with_a_pong() {
    let Some((_gpu, mut f)) = fake() else { return };
    f.send(6, ev::PING, |w| w.uint(77));
    f.turn();
    let pong = f.expect(6, op::PONG);
    assert_eq!(pong.reader().uint(), Some(77));
}

// ── configure and slots ────────────────────────────────────────────────

#[test]
fn a_configure_acks_makes_two_dmabuf_slots_and_the_layout_frame_draws() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = spawn(&mut f, LayoutStyle::default().column());
    f.requests();
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(320);
        w.int(200);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(1));
    f.turn();
    // The ack and the slots, but no frame yet: the size changed the layout.
    let reqs = f.requests();
    let s: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    assert_eq!(
        s,
        vec![
            (XDG, op::ACK_CONFIGURE),
            (DMABUF, op::CREATE_PARAMS),
            (PARAMS_A, op::PARAMS_ADD),
            (PARAMS_A, op::PARAMS_CREATE_IMMED),
            (PARAMS_A, op::PARAMS_DESTROY),
            (DMABUF, op::CREATE_PARAMS),
            (PARAMS_B, op::PARAMS_ADD),
            (PARAMS_B, op::PARAMS_CREATE_IMMED),
            (PARAMS_B, op::PARAMS_DESTROY),
        ]
    );
    let mut add = reqs[2].reader();
    assert_eq!(add.uint(), Some(0), "plane 0");
    assert_eq!(add.uint(), Some(0), "offset 0");
    assert!(add.uint().unwrap() >= 320 * 4, "stride covers the row");
    // The fake advertised linear and then `VENDOR_MOD`; GBM picked one of
    // them, or fell back to linear because it could produce neither. The
    // test does not care which — only that the wire carries the layout the
    // target actually has, the same one for both slots, so the compositor
    // reads the buffer the way the GPU wrote it. On this machine (Mesa
    // 26.2 on the local render node) both `add`s carry 0, the linear
    // modifier: the driver took the first layout advertised.
    let (hi, lo) = added_modifier(&reqs[2]);
    let m = ((hi as u64) << 32) | lo as u64;
    assert!(
        m == 0 || m == VENDOR_MOD,
        "a layout the fake never advertised: {m:#018x}"
    );
    assert_eq!(
        added_modifier(&reqs[6]),
        (hi, lo),
        "both slots are laid out the same way"
    );
    let mut immed = reqs[3].reader();
    assert_eq!(immed.object(), Some(ObjectId(BUF_A)));
    assert_eq!((immed.int(), immed.int()), (Some(320), Some(200)));
    assert_eq!(immed.uint(), Some(XRGB));
    // The configure kicks whatever the layout says, and that frame draws
    // nothing: the queue is still the old size. It must kick — `window`
    // lets one frame request stand per window until a `Frame` answers it.
    // `Frame` logs before `Resized` for the ordering reason the scale test
    // spells out: `kick`'s signal queues ahead of the emitted event.
    assert_eq!(
        log(&f),
        vec![format!("Frame {win:?}"), "Resized 320x200".to_string()]
    );
    assert!(f.app.resource::<Surfaces>().is_configured(win));
    assert_eq!(
        f.app.component::<LayoutStyle>(win).unwrap().width,
        px(320.0)
    );

    // The layout change requests the frame; it is drawn on the next tick.
    f.app.tick();
    f.turn();
    let reqs = f.requests();
    let s: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    assert_eq!(
        s,
        vec![
            (SURFACE, op::ATTACH),
            (SURFACE, op::DAMAGE_BUFFER),
            (SURFACE, op::FRAME),
            (SURFACE, op::COMMIT),
        ]
    );
    assert_eq!(reqs[0].reader().object(), Some(ObjectId(BUF_A)));
    let mut dmg = reqs[1].reader();
    assert_eq!(
        (dmg.int(), dmg.int(), dmg.int(), dmg.int()),
        (Some(0), Some(0), Some(320), Some(200))
    );
    assert_eq!(reqs[2].reader().object(), Some(ObjectId(CALLBACK)));
    assert_eq!(
        log(&f),
        vec![
            format!("Frame {win:?}"),
            "Resized 320x200".to_string(),
            format!("Frame {win:?}"),
        ]
    );
}

#[test]
fn a_zero_configure_settles_on_the_layout_size_and_draws_at_once() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = spawn(
        &mut f,
        LayoutStyle::default().column().size(px(100.0), px(50.0)),
    );
    f.requests();
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(0);
        w.int(0);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(1));
    f.turn();
    // The settled size equals the layout's, so the frame is kicked now.
    let s = shape(&mut f);
    assert_eq!(s[0], (XDG, op::ACK_CONFIGURE));
    assert!(s.contains(&(SURFACE, op::ATTACH)), "{s:?}");
    assert!(s.contains(&(SURFACE, op::COMMIT)), "{s:?}");
    // `kick`'s `app.signal(Frame(w))` is queued directly, ahead of
    // `Resized`'s `Emitted<Resized>`, which only reaches the signal queue
    // after `app.flush` drains the `Resized` event job itself: "Frame"
    // logs before "Resized" every time settle's own kick fires.
    assert_eq!(
        log(&f),
        vec![format!("Frame {win:?}"), "Resized 100x50".to_string()]
    );
}

#[test]
fn a_zero_configure_with_no_layout_size_takes_the_default() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = spawn(&mut f, LayoutStyle::default().column());
    f.requests();
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(0);
        w.int(0);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(1));
    f.turn();
    // The configure's own kick draws nothing at the old size; the
    // relayout's frame follows a tick later.
    assert_eq!(
        log(&f),
        vec![format!("Frame {win:?}"), "Resized 640x480".to_string(),]
    );
    let reqs = f.requests();
    let immed = reqs
        .iter()
        .find(|r| r.opcode == op::PARAMS_CREATE_IMMED)
        .unwrap();
    let mut r = immed.reader();
    r.object();
    assert_eq!((r.int(), r.int()), (Some(640), Some(480)));
}

#[test]
fn a_layer_configure_acks_and_draws_too() {
    let Some((_gpu, mut f)) = fake() else { return };
    let root = f.app.root();
    let role = Role::Layer(LayerRole {
        layer: Layer::Top,
        anchor: Anchor::TOP,
        exclusive_zone: 0,
        namespace: "bar".into(),
        keyboard_interactivity: KeyboardInteractivity::None,
    });
    f.app.spawn_with(
        root,
        window().layout(LayoutStyle::default().column()),
        (role,),
    );
    f.app.tick();
    f.turn();
    f.requests();
    f.send(XDG, ev::LAYER_CONFIGURE, |w| {
        w.uint(3);
        w.uint(800);
        w.uint(32);
    });
    f.turn();
    f.app.tick();
    f.turn();
    assert!(log(&f).contains(&"Resized 800x32".to_string()));
    let reqs = f.requests();
    let s: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    let ack = reqs
        .iter()
        .find(|r| r.sender == ObjectId(XDG) && r.opcode == op::LAYER_ACK)
        .unwrap();
    assert_eq!(ack.reader().uint(), Some(3));
    assert!(s.contains(&(SURFACE, op::ATTACH)), "{s:?}");
    assert!(s.contains(&(SURFACE, op::COMMIT)), "{s:?}");
    let attach = reqs
        .iter()
        .find(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::ATTACH)
        .unwrap();
    assert_eq!(attach.reader().object(), Some(ObjectId(LAYER_BUF_A)));
}

fn layer_role() -> Role {
    Role::Layer(LayerRole {
        layer: Layer::Top,
        anchor: Anchor::TOP,
        exclusive_zone: 0,
        namespace: "bar".into(),
        keyboard_interactivity: KeyboardInteractivity::None,
    })
}

/// A layer window spawned and flushed, requests drained, not yet configured.
fn spawn_layer(f: &mut Fake) -> NodeId {
    let root = f.app.root();
    let win = f
        .app
        .spawn_with(
            root,
            window().layout(LayoutStyle::default().column()),
            (layer_role(),),
        )
        .id();
    f.app.tick();
    f.turn();
    f.requests();
    win
}

fn layer_configure(f: &mut Fake, serial: u32) {
    f.send(XDG, ev::LAYER_CONFIGURE, |w| {
        w.uint(serial);
        w.uint(800);
        w.uint(32);
    });
    f.turn();
    f.app.tick();
    f.turn();
}

fn set_mapped(f: &mut Fake, win: NodeId, mapped: bool) {
    {
        let (mut surfaces, mut wl) = f.app.query::<(ResMut<Surfaces>, ResMut<Wayland>)>();
        surfaces.set_mapped(win, mapped, &mut wl);
    }
    f.turn();
}

#[test]
fn hiding_before_spawn_suppresses_the_first_draw() {
    let Some((_gpu, mut f)) = fake() else { return };
    let root = f.app.root();
    let win = f
        .app
        .spawn_with(
            root,
            window().layout(LayoutStyle::default().column()),
            (layer_role(),),
        )
        .id();
    {
        let (mut surfaces, mut wl) = f.app.query::<(ResMut<Surfaces>, ResMut<Wayland>)>();
        surfaces.set_mapped(win, false, &mut wl);
    }
    f.app.tick();
    f.turn();
    f.requests();
    layer_configure(&mut f, 3);
    assert!(log(&f).contains(&"Resized 800x32".to_string()));
    let reqs = f.requests();
    let ack = reqs
        .iter()
        .find(|r| r.sender == ObjectId(XDG) && r.opcode == op::LAYER_ACK)
        .expect("the configure is acked");
    assert_eq!(ack.reader().uint(), Some(3));
    assert!(
        !reqs
            .iter()
            .any(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::ATTACH),
        "a hidden window draws nothing"
    );
    assert!(!f.app.resource::<Surfaces>().is_mapped(win));
}

/// What showing a drawn layer window sends: the layer state the unmap
/// reset, then an empty commit.
const REMAP_SHAPE: [(u32, u16); 6] = [
    (XDG, op::LAYER_SET_LAYER),
    (XDG, op::LAYER_SET_ANCHOR),
    (XDG, op::LAYER_SET_ZONE),
    (XDG, op::LAYER_SET_KEYBOARD),
    (XDG, op::LAYER_SET_SIZE),
    (SURFACE, op::COMMIT),
];

/// A layer window configured and drawn, requests drained.
fn drawn_layer(f: &mut Fake) -> NodeId {
    let win = spawn_layer(f);
    layer_configure(f, 3);
    assert_eq!(attaches(f), vec![LAYER_BUF_A]);
    win
}

#[test]
fn unmapping_attaches_null_and_commits() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_mapped(&mut f, win, false);
    let reqs = f.requests();
    let on_surface: Vec<&wayland::fake::Request> = reqs
        .iter()
        .filter(|r| r.sender == ObjectId(SURFACE))
        .collect();
    assert_eq!(on_surface.len(), 2, "{reqs:?}");
    assert_eq!(on_surface[0].opcode, op::ATTACH);
    assert_eq!(on_surface[0].reader().object(), Some(ObjectId(0)));
    assert_eq!(on_surface[1].opcode, op::COMMIT);
    assert!(!f.app.resource::<Surfaces>().is_mapped(win));
    assert!(!f.app.resource::<Surfaces>().is_configured(win));
}

#[test]
fn remapping_waits_for_a_new_configure_then_draws() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_mapped(&mut f, win, false);
    f.requests();
    set_mapped(&mut f, win, true);
    let s = shape(&mut f);
    assert_eq!(s, REMAP_SHAPE);
    assert!(f.app.resource::<Surfaces>().is_mapped(win));

    layer_configure(&mut f, 4);
    let reqs = f.requests();
    let ack = reqs
        .iter()
        .position(|r| r.sender == ObjectId(XDG) && r.opcode == op::LAYER_ACK)
        .expect("the new configure is acked");
    assert_eq!(reqs[ack].reader().uint(), Some(4));
    let attach = reqs
        .iter()
        .position(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::ATTACH)
        .expect("the remap draws");
    assert!(ack < attach);
    assert_ne!(reqs[attach].reader().object(), Some(ObjectId(0)));
}

#[test]
fn remapping_resends_the_layer_state_the_unmap_reset() {
    let Some((_gpu, mut f)) = fake() else { return };
    let root = f.app.root();
    let role = Role::Layer(LayerRole {
        layer: Layer::Top,
        anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        exclusive_zone: 32,
        namespace: "bar".into(),
        keyboard_interactivity: KeyboardInteractivity::None,
    });
    let win = f
        .app
        .spawn_with(
            root,
            window().layout(LayoutStyle::default().column().size(auto(), px(32.0))),
            (role,),
        )
        .id();
    f.app.tick();
    f.turn();
    f.requests();
    layer_configure(&mut f, 3);
    f.requests();
    {
        let (mut surfaces, mut wl) = f.app.query::<(ResMut<Surfaces>, ResMut<Wayland>)>();
        surfaces.request_layer_size(win, 0, 48, &mut wl);
    }
    f.turn();
    set_mapped(&mut f, win, false);
    f.requests();
    set_mapped(&mut f, win, true);
    let reqs = f.requests();
    let shape: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    assert_eq!(
        shape,
        vec![
            (XDG, op::LAYER_SET_LAYER),
            (XDG, op::LAYER_SET_ANCHOR),
            (XDG, op::LAYER_SET_ZONE),
            (XDG, op::LAYER_SET_KEYBOARD),
            (XDG, op::LAYER_SET_SIZE),
            (SURFACE, op::COMMIT),
        ],
        "the unmap reset the layer state (the layer to background), so the remap commit needs it again, with no attach"
    );
    assert_eq!(reqs[0].reader().uint(), Some(Layer::Top as u32));
    assert_eq!(reqs[1].reader().uint(), Some(1 | 4 | 8));
    assert_eq!(reqs[2].reader().int(), Some(32));
    let mut r = reqs[4].reader();
    assert_eq!((r.uint(), r.uint()), (Some(0), Some(48)));

    layer_configure(&mut f, 4);
    let reqs = f.requests();
    let ack = reqs
        .iter()
        .position(|r| r.sender == ObjectId(XDG) && r.opcode == op::LAYER_ACK)
        .expect("the fresh configure is acked");
    let attach = reqs
        .iter()
        .position(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::ATTACH)
        .expect("and the window draws");
    assert!(ack < attach);
}

#[test]
fn a_configure_in_flight_across_the_unmap_does_not_skip_the_remap_commit() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_mapped(&mut f, win, false);
    // Sent before the compositor saw the null attach, delivered after it.
    layer_configure(&mut f, 4);
    f.requests();
    set_mapped(&mut f, win, true);
    f.app.tick();
    f.turn();
    let s = shape(&mut f);
    assert_eq!(
        s, REMAP_SHAPE,
        "layer state then an empty commit, no attach"
    );
    assert!(!f.app.resource::<Surfaces>().is_configured(win));

    layer_configure(&mut f, 5);
    let reqs = f.requests();
    let ack = reqs
        .iter()
        .position(|r| r.sender == ObjectId(XDG) && r.opcode == op::LAYER_ACK)
        .expect("the fresh configure is acked");
    assert_eq!(reqs[ack].reader().uint(), Some(5));
    let attach = reqs
        .iter()
        .position(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::ATTACH)
        .expect("the remap draws after the fresh configure");
    assert!(ack < attach);
}

#[test]
fn set_mapped_on_a_toplevel_does_nothing() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 320, 200);
    f.requests();
    set_mapped(&mut f, win, false);
    set_mapped(&mut f, win, true);
    assert!(shape(&mut f).is_empty());
    assert!(f.app.resource::<Surfaces>().is_configured(win));
}

fn set_layer(f: &mut Fake, win: NodeId, layer: Layer) {
    {
        let (mut surfaces, mut wl) = f.app.query::<(ResMut<Surfaces>, ResMut<Wayland>)>();
        surfaces.set_layer(win, layer, &mut wl);
    }
    f.turn();
}

#[test]
fn set_layer_sends_the_layer_then_commits_without_unmapping() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_layer(&mut f, win, Layer::Background);
    let reqs = f.requests();
    let shape: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    assert_eq!(
        shape,
        vec![(XDG, op::LAYER_SET_LAYER), (SURFACE, op::COMMIT)],
        "{reqs:?}"
    );
    assert_eq!(reqs[0].reader().uint(), Some(Layer::Background as u32));
    assert!(!reqs.iter().any(|r| r.opcode == op::ATTACH));
    let s = f.app.resource::<Surfaces>();
    assert_eq!(s.layer_of(win), Some(Layer::Background));
    assert!(s.is_mapped(win));
    assert!(s.is_configured(win));
}

#[test]
fn set_layer_to_the_current_layer_sends_nothing() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_layer(&mut f, win, Layer::Top);
    assert!(shape(&mut f).is_empty());
    set_layer(&mut f, win, Layer::Background);
    f.requests();
    set_layer(&mut f, win, Layer::Background);
    assert!(shape(&mut f).is_empty());
    set_layer(&mut f, win, Layer::Top);
    let reqs = f.requests();
    assert_eq!(reqs[0].opcode, op::LAYER_SET_LAYER);
    assert_eq!(reqs[0].reader().uint(), Some(Layer::Top as u32));
}

#[test]
fn set_layer_before_spawn_makes_the_layer_surface_start_there() {
    let Some((_gpu, mut f)) = fake() else { return };
    let root = f.app.root();
    let win = f
        .app
        .spawn_with(
            root,
            window().layout(LayoutStyle::default().column()),
            (layer_role(),),
        )
        .id();
    {
        let (mut surfaces, mut wl) = f.app.query::<(ResMut<Surfaces>, ResMut<Wayland>)>();
        surfaces.set_layer(win, Layer::Background, &mut wl);
    }
    f.app.tick();
    f.turn();
    let reqs = f.requests();
    let get = reqs
        .iter()
        .find(|r| r.sender == ObjectId(8) && r.opcode == op::GET_LAYER_SURFACE)
        .expect("the layer surface is requested");
    let mut r = get.reader();
    r.object();
    r.object();
    r.object_opt();
    assert_eq!(r.uint(), Some(Layer::Background as u32));
    assert!(!reqs.iter().any(|r| r.opcode == op::LAYER_SET_LAYER));
    assert_eq!(
        f.app.resource::<Surfaces>().layer_of(win),
        Some(Layer::Background)
    );
}

#[test]
fn set_layer_on_a_toplevel_does_nothing() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 320, 200);
    f.requests();
    set_layer(&mut f, win, Layer::Background);
    assert!(shape(&mut f).is_empty());
    assert_eq!(f.app.resource::<Surfaces>().layer_of(win), None);
}

fn set_exclusive_zone(f: &mut Fake, win: NodeId, zone: i32) {
    {
        let (mut surfaces, mut wl) = f.app.query::<(ResMut<Surfaces>, ResMut<Wayland>)>();
        surfaces.set_exclusive_zone(win, zone, &mut wl);
    }
    f.turn();
}

#[test]
fn set_exclusive_zone_sends_the_zone_then_commits() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_exclusive_zone(&mut f, win, 252);
    let reqs = f.requests();
    let shape: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    assert_eq!(
        shape,
        vec![(XDG, op::LAYER_SET_ZONE), (SURFACE, op::COMMIT)],
        "{reqs:?}"
    );
    assert_eq!(reqs[0].reader().int(), Some(252));
    let s = f.app.resource::<Surfaces>();
    assert_eq!(s.exclusive_zone_of(win), Some(252));
    assert!(s.is_mapped(win));
}

#[test]
fn set_exclusive_zone_to_the_current_zone_sends_nothing() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_exclusive_zone(&mut f, win, 0);
    assert!(shape(&mut f).is_empty());
}

#[test]
fn set_exclusive_zone_while_hidden_is_sent_on_remap() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_mapped(&mut f, win, false);
    f.requests();
    set_exclusive_zone(&mut f, win, 252);
    assert!(shape(&mut f).is_empty(), "nothing is sent while hidden");
    set_mapped(&mut f, win, true);
    let reqs = f.requests();
    let zone = reqs
        .iter()
        .find(|r| r.sender == ObjectId(XDG) && r.opcode == op::LAYER_SET_ZONE)
        .expect("the remap sends the zone");
    assert_eq!(zone.reader().int(), Some(252));
}

#[test]
fn set_exclusive_zone_on_a_toplevel_does_nothing() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 320, 200);
    f.requests();
    set_exclusive_zone(&mut f, win, 252);
    assert!(shape(&mut f).is_empty());
    assert_eq!(f.app.resource::<Surfaces>().exclusive_zone_of(win), None);
}

fn set_keyboard_interactivity(f: &mut Fake, win: NodeId, ki: KeyboardInteractivity) {
    {
        let (mut surfaces, mut wl) = f.app.query::<(ResMut<Surfaces>, ResMut<Wayland>)>();
        surfaces.set_keyboard_interactivity(win, ki, &mut wl);
    }
    f.turn();
}

#[test]
fn set_keyboard_interactivity_sends_it_then_commits() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_keyboard_interactivity(&mut f, win, KeyboardInteractivity::Exclusive);
    let reqs = f.requests();
    let shape: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    assert_eq!(
        shape,
        vec![(XDG, op::LAYER_SET_KEYBOARD), (SURFACE, op::COMMIT)],
        "{reqs:?}"
    );
    assert_eq!(
        reqs[0].reader().uint(),
        Some(KeyboardInteractivity::Exclusive as u32)
    );
    let s = f.app.resource::<Surfaces>();
    assert_eq!(
        s.keyboard_interactivity_of(win),
        Some(KeyboardInteractivity::Exclusive)
    );
    assert!(s.is_mapped(win));
}

#[test]
fn set_keyboard_interactivity_to_the_current_value_sends_nothing() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    let current = f
        .app
        .resource::<Surfaces>()
        .keyboard_interactivity_of(win)
        .expect("a layer window");
    set_keyboard_interactivity(&mut f, win, current);
    assert!(shape(&mut f).is_empty());
}

#[test]
fn set_keyboard_interactivity_while_hidden_is_sent_on_remap() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_mapped(&mut f, win, false);
    f.requests();
    set_keyboard_interactivity(&mut f, win, KeyboardInteractivity::Exclusive);
    assert!(shape(&mut f).is_empty(), "nothing is sent while hidden");
    set_mapped(&mut f, win, true);
    let reqs = f.requests();
    let ki = reqs
        .iter()
        .find(|r| r.sender == ObjectId(XDG) && r.opcode == op::LAYER_SET_KEYBOARD)
        .expect("the remap sends the keyboard interactivity");
    assert_eq!(
        ki.reader().uint(),
        Some(KeyboardInteractivity::Exclusive as u32)
    );
}

#[test]
fn set_keyboard_interactivity_on_a_toplevel_does_nothing() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 320, 200);
    f.requests();
    set_keyboard_interactivity(&mut f, win, KeyboardInteractivity::Exclusive);
    assert!(shape(&mut f).is_empty());
    assert_eq!(
        f.app.resource::<Surfaces>().keyboard_interactivity_of(win),
        None
    );
}

#[test]
fn layer_surface_of_returns_the_windows_layer_surface() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    let ls = f.app.resource::<Surfaces>().layer_surface_of(win);
    assert_eq!(ls.map(|l| l.id()), Some(ObjectId(XDG)));
}

#[test]
fn layer_surface_of_a_toplevel_is_none() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 320, 200);
    assert_eq!(f.app.resource::<Surfaces>().layer_surface_of(win), None);
}

fn set_input_region(f: &mut Fake, win: NodeId, region: Option<&[Rect]>) {
    {
        let (mut surfaces, mut wl) = f.app.query::<(ResMut<Surfaces>, ResMut<Wayland>)>();
        surfaces.set_input_region(win, region, &mut wl);
    }
    f.turn();
}

#[test]
fn empty_input_region_sends_an_empty_region() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_input_region(&mut f, win, Some(&[]));
    let reqs = f.requests();
    let create = reqs
        .iter()
        .position(|r| r.sender == ObjectId(COMPOSITOR) && r.opcode == op::CREATE_REGION)
        .expect("a region is created");
    let region = reqs[create].reader().object().expect("new region id");
    assert!(
        !reqs
            .iter()
            .any(|r| r.sender == region && r.opcode == op::REGION_ADD)
    );
    let set = reqs
        .iter()
        .position(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::SET_INPUT_REGION)
        .expect("the region is set");
    assert_eq!(reqs[set].reader().object(), Some(region));
    let destroy = reqs
        .iter()
        .position(|r| r.sender == region && r.opcode == op::REGION_DESTROY)
        .expect("the region is destroyed");
    let commit = reqs
        .iter()
        .position(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::COMMIT)
        .expect("the change is committed");
    assert!(
        create < set && set < destroy && destroy < commit,
        "{reqs:?}"
    );
}

#[test]
fn input_region_rects_are_added() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_input_region(&mut f, win, Some(&[Rect::new(0., 0., 10.5, 20.)]));
    let reqs = f.requests();
    let region = reqs
        .iter()
        .find(|r| r.sender == ObjectId(COMPOSITOR) && r.opcode == op::CREATE_REGION)
        .and_then(|r| r.reader().object())
        .expect("a region is created");
    let adds: Vec<_> = reqs
        .iter()
        .filter(|r| r.sender == region && r.opcode == op::REGION_ADD)
        .collect();
    assert_eq!(adds.len(), 1, "{reqs:?}");
    let mut r = adds[0].reader();
    assert_eq!(
        (r.int(), r.int(), r.int(), r.int()),
        (Some(0), Some(0), Some(11), Some(20))
    );
}

#[test]
fn none_resets_the_input_region() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = drawn_layer(&mut f);
    set_input_region(&mut f, win, None);
    let reqs = f.requests();
    let set = reqs
        .iter()
        .find(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::SET_INPUT_REGION)
        .expect("the region is reset");
    assert_eq!(set.reader().object(), Some(ObjectId(0)));
    assert!(
        reqs.iter()
            .any(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::COMMIT)
    );
}

// ── the frame loop ─────────────────────────────────────────────────────

#[test]
fn frames_are_throttled_by_the_callback() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 320, 200);
    f.requests();
    repaint(&mut f, win);
    f.turn();
    assert!(attaches(&mut f).is_empty(), "the callback is outstanding");
    f.send(CALLBACK, ev::DONE, |w| w.uint(1));
    f.turn();
    assert_eq!(attaches(&mut f), vec![BUF_B]);

    f.send(CALLBACK + 1, ev::DONE, |w| w.uint(0));
    f.turn();
    assert!(attaches(&mut f).is_empty(), "nothing wanted");
}

#[test]
fn changing_window_direction_keeps_the_configured_size_and_draws() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 640, 480);
    f.requests();
    f.send(CALLBACK, ev::DONE, |w| w.uint(1));
    f.turn();
    f.requests();

    let mut style = f.app.component::<LayoutStyle>(win).unwrap().clone();
    style.direction = Direction::Row;
    *f.app.component_mut::<LayoutStyle>(win).unwrap() = style;
    repaint(&mut f, win);
    f.turn();
    assert_eq!(
        f.app.component::<LayoutStyle>(win).unwrap().width,
        px(640.0)
    );
    assert_eq!(
        f.app.component::<LayoutStyle>(win).unwrap().height,
        px(480.0)
    );
    assert!(
        !attaches(&mut f).is_empty(),
        "changing only direction must still attach a frame"
    );
}

#[test]
fn with_both_buffers_held_a_release_draws() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 64, 64);
    f.requests();
    repaint(&mut f, win);
    f.send(CALLBACK, ev::DONE, |w| w.uint(0));
    f.turn();
    assert_eq!(attaches(&mut f), vec![BUF_B]);

    repaint(&mut f, win);
    f.send(CALLBACK + 1, ev::DONE, |w| w.uint(0));
    f.turn();
    assert!(attaches(&mut f).is_empty(), "both buffers are held");

    f.send(BUF_A, ev::RELEASE, |_| {});
    f.turn();
    let reqs = f.requests();
    let attached: Vec<u32> = reqs
        .iter()
        .filter(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::ATTACH)
        .map(|r| r.reader().object().unwrap().0)
        .collect();
    assert_eq!(attached, vec![BUF_A]);

    // BUF_A last held frame 0 and the window is on frame 2, so it is
    // redrawn at age 2: render's scissor is the union of the last two
    // frames' damage, newest first, and nothing else. Each `repaint`
    // spawned a 10 by 10 leaf into the window's column, so the second sits
    // under the first: frame 2 damaged (0, 10, 10, 10) and frame 1 damaged
    // (0, 0, 10, 10). The window is 64 by 64, so this is incremental
    // damage, not a full repaint dressed up as one.
    assert_eq!(damage(&reqs), vec![(0, 10, 10, 10), (0, 0, 10, 10)]);
}

#[test]
fn a_repaint_into_a_released_slot_damages_only_that_rect() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 64, 64);
    f.requests();
    // Free the slot the first frame drew and clear the callback, without
    // wanting anything: no frame runs, so no walk files a damage list.
    f.send(CALLBACK, ev::DONE, |w| w.uint(1));
    f.send(BUF_A, ev::RELEASE, |_| {});
    f.turn();
    assert!(attaches(&mut f).is_empty(), "nothing wanted");

    // One 10 by 10 leaf changes. The redraw takes BUF_A back, which holds
    // frame 0 of 1, so age 1: the scissor is that one frame's damage.
    repaint(&mut f, win);
    f.turn();
    let reqs = f.requests();
    assert_eq!(
        reqs.iter()
            .filter(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::ATTACH)
            .count(),
        1,
        "the repaint drew"
    );
    assert_eq!(
        damage(&reqs),
        vec![(0, 0, 10, 10)],
        "only the leaf, not the 64 by 64 window"
    );
}

#[test]
fn two_configures_in_one_turn_draw_one_frame() {
    let Some((_gpu, mut f)) = fake() else { return };
    spawn(&mut f, LayoutStyle::default().column());
    f.requests();
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(64);
        w.int(64);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(1));
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(64);
        w.int(64);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(2));
    f.turn();
    f.app.tick();
    f.turn();
    assert_eq!(attaches(&mut f).len(), 1, "one frame for the whole burst");

    f.send(CALLBACK, ev::DONE, |w| w.uint(0));
    f.turn();
    assert_eq!(attaches(&mut f).len(), 0, "nothing wanted yet");
}

#[test]
fn a_frame_with_nothing_changed_commits_nothing() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 320, 200);
    f.requests();
    f.send(CALLBACK, ev::DONE, |w| w.uint(1));
    // The first draw's slot is released too, so the second draw reuses it
    // at age 1 rather than the other, never-drawn slot: a never-drawn
    // slot's age is 0, which `render::Scenes` always treats as "the whole
    // window", so nothing-changed could never be demonstrated on it.
    f.send(BUF_A, ev::RELEASE, |_| {});
    f.turn();
    // A second configure of the same size kicks a Frame; render's scissor
    // is empty, so nothing is attached or committed.
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(320);
        w.int(200);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(2));
    f.turn();
    let s = shape(&mut f);
    assert_eq!(s, vec![(XDG, op::ACK_CONFIGURE)]);
    assert!(log(&f).contains(&format!("Frame {win:?}")), "{:?}", log(&f));
}

#[test]
fn a_configure_at_a_new_size_draws_though_the_first_request_is_still_pending() {
    let Some((_gpu, mut f)) = fake() else { return };
    // A window its own layout sizes asks `window` for a frame before the
    // compositor has configured anything, and `window` lets one request
    // stand per window until a `Frame` answers it. So the configure must
    // kick even though it changes the size: every later `RequestFrame`,
    // the relayout's included, is swallowed until something does.
    let win = spawn(
        &mut f,
        LayoutStyle::default().column().size(px(200.0), px(100.0)),
    );
    f.requests();
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(320);
        w.int(200);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(1));
    f.turn();
    let s = shape(&mut f);
    assert!(
        !s.contains(&(SURFACE, op::ATTACH)),
        "the queue is still the old size, so that frame draws nothing: {s:?}"
    );
    assert!(
        log(&f).contains(&format!("Frame {win:?}")),
        "but the frame ran, which is what clears the window's pending bit: {:?}",
        log(&f)
    );
    f.app.tick();
    f.turn();
    assert!(
        !attaches(&mut f).is_empty(),
        "the relayout the configure caused draws the first frame"
    );
    assert_eq!(
        f.app.component::<LayoutStyle>(win).unwrap().width,
        px(320.0)
    );
}

// ── scale and resize ───────────────────────────────────────────────────

#[test]
fn a_preferred_scale_rescales_the_slots_and_redraws_at_once() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 320, 200);
    f.requests();
    f.send(CALLBACK, ev::DONE, |w| w.uint(1));
    f.turn();
    f.requests();
    f.send(SURFACE, ev::PREFERRED_SCALE, |w| w.int(2));
    f.turn();
    let reqs = f.requests();
    let s: Vec<(u32, u16)> = reqs.iter().map(|r| (r.sender.0, r.opcode)).collect();
    // Old buffers destroyed, two new slots made, then the frame at scale 2.
    assert_eq!(
        &s[..2],
        &[(BUF_A, op::BUFFER_DESTROY), (BUF_B, op::BUFFER_DESTROY)]
    );
    // `zwp_linux_buffer_params_v1.create_immed` and `wl_surface.frame` are
    // both opcode 3 in the real protocol, and the kick draws a frame in
    // this same turn, so the opcode alone is ambiguous: exclude `SURFACE`.
    let immed: Vec<&wayland::fake::Request> = reqs
        .iter()
        .filter(|r| r.opcode == op::PARAMS_CREATE_IMMED && r.sender != ObjectId(SURFACE))
        .collect();
    assert_eq!(immed.len(), 2);
    let mut r = immed[0].reader();
    r.object();
    assert_eq!(
        (r.int(), r.int()),
        (Some(640), Some(400)),
        "device pixels at scale 2"
    );
    let tail: Vec<(u32, u16)> = s[s.len() - 5..].to_vec();
    assert_eq!(
        tail,
        vec![
            (SURFACE, op::SET_BUFFER_SCALE),
            (SURFACE, op::ATTACH),
            (SURFACE, op::DAMAGE_BUFFER),
            (SURFACE, op::FRAME),
            (SURFACE, op::COMMIT),
        ]
    );
    let mut dmg = reqs[reqs.len() - 3].reader();
    assert_eq!(
        (dmg.int(), dmg.int(), dmg.int(), dmg.int()),
        (Some(0), Some(0), Some(640), Some(400)),
        "the whole window at the new scale"
    );
    // `kick`'s `app.signal(Frame(w))` inside `on_surface` queues directly,
    // ahead of `ScaleFactorChanged`'s `Emitted<ScaleFactorChanged>`, which
    // only reaches the signal queue after the event that carried it is
    // fully flushed: the same ordering Task 5 hit with `Resized`, so
    // "Frame" logs before "Scale 2" even though `on_surface` emitted the
    // scale change first.
    assert_eq!(
        log(&f),
        vec![
            format!("Frame {win:?}"),
            "Resized 320x200".to_string(),
            format!("Frame {win:?}"),
            format!("Frame {win:?}"),
            "Scale 2".to_string(),
        ]
    );
    assert_eq!(
        f.app.component::<LayoutStyle>(win).unwrap().width,
        px(320.0),
        "logical size unchanged"
    );
}

#[test]
fn a_configure_to_a_new_size_waits_for_the_layout_and_draws_the_new_size() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 320, 200);
    f.requests();
    f.send(CALLBACK, ev::DONE, |w| w.uint(1));
    f.turn();
    f.requests();
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(400);
        w.int(300);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(2));
    f.turn();
    let s = shape(&mut f);
    assert!(
        !s.contains(&(SURFACE, op::ATTACH)),
        "no frame before the layout: {s:?}"
    );
    assert_eq!(
        s.iter().filter(|r| r.1 == op::PARAMS_CREATE_IMMED).count(),
        2,
        "new slots"
    );
    f.app.tick();
    f.turn();
    let reqs = f.requests();
    let attach = reqs
        .iter()
        .find(|r| r.opcode == op::ATTACH)
        .expect("the layout's frame");
    let dmg = reqs.iter().find(|r| r.opcode == op::DAMAGE_BUFFER).unwrap();
    let mut d = dmg.reader();
    assert_eq!(
        (d.int(), d.int(), d.int(), d.int()),
        (Some(0), Some(0), Some(400), Some(300))
    );
    let _ = attach;
    assert_eq!(
        f.app.component::<LayoutStyle>(win).unwrap().width,
        px(400.0)
    );
}

// ── atlas and readback ─────────────────────────────────────────────────

#[test]
fn the_first_frame_holds_the_clear_colour() {
    let Some((_gpu, mut f)) = fake() else { return };
    let root = f.app.root();
    let win = f
        .app
        .spawn(
            root,
            window()
                .title("hello")
                .clear(Color::rgb(0.0, 0.0, 1.0))
                .layout(LayoutStyle::default().column().size(px(20.0), px(10.0))),
        )
        .id();
    f.app.tick();
    f.turn();
    f.requests();
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(0);
        w.int(0);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(1));
    f.turn();
    let (w, h, px) = f
        .app
        .resource_mut::<Surfaces>()
        .last_frame(win)
        .expect("a frame was drawn");
    assert_eq!((w, h), (20, 10));
    assert_eq!(&px[..4], &[0, 0, 255, 255]);
    assert_eq!(&px[px.len() - 4..], &[0, 0, 255, 255]);
}

#[test]
fn a_glyph_resolved_in_a_tick_is_on_the_gpu_before_that_ticks_frame() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 64, 64);
    f.requests();
    f.send(CALLBACK, ev::DONE, |w| w.uint(1));
    f.turn();
    // Resolve a glyph and paint it in one tick: the atlas upload runs on
    // OnChanged<Atlas> at PostTick, before render's request for the frame.
    let tile = {
        let mut atlas = f.app.resource_mut::<Atlas>();
        let inter = atlas
            .add_font(include_bytes!(
                "../../atlas/tests/fixtures/Inter-Regular.ttf"
            ))
            .unwrap();
        let (font, id) = atlas.lookup(&[inter], 'a').unwrap();
        atlas.glyph(font, id, 40).tile
    };
    struct Leaf;
    impl Build for Leaf {
        type Widget = Leaf;
    }
    impl Widget for Leaf {
        type Builder = Leaf;
        fn build(b: Leaf, _: Handle<Self>, _: &mut Spawner<'_, Self>) -> Self {
            b
        }
    }
    let size = tile.bounds.size;
    f.app.spawn_with(
        win,
        Leaf,
        (
            LayoutStyle::default().size(px(size.width), px(size.height)),
            Paint::Monochrome(vec![MonochromeSprite::new(
                tile,
                geometry::Point::ZERO,
                size,
                Color::WHITE,
            )]),
        ),
    );
    f.app.tick();
    f.turn();
    assert!(!attaches(&mut f).is_empty(), "the glyph frame was drawn");
    let (w, _h, px) = f.app.resource_mut::<Surfaces>().last_frame(win).unwrap();
    let brightest = px.chunks(4).map(|p| p[0]).max().unwrap();
    assert!(
        brightest > 200,
        "the glyph's ink is white on black: {brightest}"
    );
    let _ = w;
}

#[test]
fn a_window_scale_rescales_the_atlas() {
    struct Probe;
    impl Build for Probe {
        type Widget = Probe;
    }
    impl Widget for Probe {
        type Builder = Probe;
        fn build(b: Probe, _: Handle<Self>, _: &mut Spawner<'_, Self>) -> Self {
            b
        }
    }
    let Some((_gpu, mut f)) = fake() else { return };
    let a = configured(&mut f, 64, 64);
    let b = spawn(&mut f, LayoutStyle::default().column());
    let probe = f.app.spawn(b, Probe).id();
    f.app.tick();
    f.turn();
    let rescaled = |f: &Fake| f.app.resource::<Rescales>().0.clone();
    assert_eq!(f.app.resource::<Atlas>().scale(), 1.0);
    assert!(rescaled(&f).is_empty());

    // The window's own handler stores the scale before the `Emitted`
    // signal reaches the system, or the atlas would still read 1.
    f.app.emit(ScaleFactorChanged { scale: 2.0 }, a);
    f.app.tick();
    f.turn();
    assert_eq!(f.app.resource::<Atlas>().scale(), 2.0);
    let got = rescaled(&f);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains(&probe), "{:?}", got[0]);
    assert!(got[0].contains(&a) && got[0].contains(&b));

    // The same scale again changes nothing.
    f.app.emit(ScaleFactorChanged { scale: 2.0 }, a);
    f.app.tick();
    f.turn();
    assert_eq!(rescaled(&f).len(), 1);

    // The other window is still 1: the largest wins, so back to 1.
    f.app.emit(ScaleFactorChanged { scale: 1.0 }, a);
    f.app.tick();
    f.turn();
    assert_eq!(f.app.resource::<Atlas>().scale(), 1.0);
    assert_eq!(rescaled(&f).len(), 2);
}

#[test]
fn a_frame_uploads_before_drawing() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 64, 64);
    f.requests();
    repaint(&mut f, win);
    f.send(CALLBACK, ev::DONE, |w| w.uint(0));
    f.turn();
    repaint(&mut f, win);
    f.send(CALLBACK + 1, ev::DONE, |w| w.uint(0));
    f.turn();
    assert!(attaches(&mut f).len() == 1, "both buffers are held now");
    let before = f.app.resource::<Surfaces>().upload_calls();
    // A new page's glyph, rasterized after the last tick: `OnChanged<Atlas>`
    // has not fired, so only the frame itself can upload it.
    {
        let mut atlas = f.app.resource_mut::<Atlas>();
        let inter = atlas
            .add_font(include_bytes!(
                "../../atlas/tests/fixtures/Inter-Regular.ttf"
            ))
            .unwrap();
        let (font, id) = atlas.lookup(&[inter], 'a').unwrap();
        atlas.glyph(font, id, 40);
    }
    f.send(BUF_A, ev::RELEASE, |_| {});
    f.turn();
    assert_eq!(attaches(&mut f), vec![BUF_A], "the released slot was drawn");
    assert!(
        f.app.resource::<Surfaces>().upload_calls() > before,
        "the frame uploaded the dirty glyph"
    );
}

/// What the displayed paint held at each `Frame`, against what the atlas
/// holds for the same glyph then: `(generation, drawn, current)`.
#[derive(Default)]
struct StaleProbe {
    key: Option<(atlas::FontId, u16, NodeId)>,
    frames: Vec<(u64, atlas::AtlasTile, atlas::AtlasTile)>,
}
impl Resource for StaleProbe {}
fn probe_frame(app: &mut App, _: &Frame) {
    let Some((font, id, leaf)) = app.resource::<StaleProbe>().key else {
        return;
    };
    let drawn = match &app.component::<animation::AnimatedPaint>(leaf).unwrap().0 {
        Paint::Monochrome(s) => s[0].tile,
        _ => return,
    };
    let (generation, now) = {
        let mut a = app.resource_mut::<Atlas>();
        (a.generation(), a.glyph(font, id, 40).tile)
    };
    app.resource_mut::<StaleProbe>()
        .frames
        .push((generation, drawn, now));
}

#[test]
fn a_frame_queued_behind_a_rescale_waits_for_the_rescaled_paint() {
    struct Glyphy(atlas::FontId, u16);
    impl Build for Glyphy {
        type Widget = Glyphy;
    }
    impl Widget for Glyphy {
        type Builder = Glyphy;
        fn build(b: Glyphy, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
            s.on::<Rescaled>(me, |ctx: &mut Context<'_, Glyphy>, _| {
                let (font, id) = (ctx.me().0, ctx.me().1);
                let g = ctx.resource_mut::<Atlas>().glyph(font, id, 40);
                ctx.set_paint(Paint::Monochrome(vec![MonochromeSprite::new(
                    g.tile,
                    geometry::Point::ZERO,
                    g.size,
                    Color::WHITE,
                )]));
            });
            b
        }
    }
    let Some((_gpu, mut f)) = fake() else { return };
    f.app.init_resource::<StaleProbe>().system(probe_frame);
    let win = configured(&mut f, 64, 64);
    let (font, id, g) = {
        let mut atlas = f.app.resource_mut::<Atlas>();
        let inter = atlas
            .add_font(include_bytes!(
                "../../atlas/tests/fixtures/Inter-Regular.ttf"
            ))
            .unwrap();
        let (font, id) = atlas.lookup(&[inter], 'a').unwrap();
        (font, id, atlas.glyph(font, id, 40))
    };
    let leaf = f
        .app
        .spawn_with(
            win,
            Glyphy(font, id),
            (
                LayoutStyle::default().size(px(30.0), px(30.0)),
                Paint::Monochrome(vec![MonochromeSprite::new(
                    g.tile,
                    geometry::Point::ZERO,
                    g.size,
                    Color::WHITE,
                )]),
            ),
        )
        .id();
    f.app.resource_mut::<StaleProbe>().key = Some((font, id, leaf));
    f.app.tick();
    f.turn();
    f.send(CALLBACK, ev::DONE, |w| w.uint(1));
    f.turn();
    f.app.tick();
    f.turn();
    assert!(!attaches(&mut f).is_empty(), "the glyph frame was drawn");

    // One read: the scale, then the outstanding callback's `done`. The
    // `done`'s kick queues `Frame` behind the `Emitted` that rebuilds the
    // atlas, so it runs before the `Rescaled` paint reaches the displayed
    // paint; drawing it would sample old tiles from the new pages.
    f.send(SURFACE, ev::PREFERRED_SCALE, |w| w.int(2));
    f.send(CALLBACK + 1, ev::DONE, |w| w.uint(2));
    f.turn();
    assert_eq!(f.app.resource::<Atlas>().generation(), 1, "rebuilt");
    assert!(attaches(&mut f).is_empty(), "the frame waits for the paint");

    // The next tick carries the paint to the displayed paint, and the
    // deferred frame draws with the current tiles.
    f.app.tick();
    f.turn();
    assert!(!attaches(&mut f).is_empty(), "the deferred frame was drawn");
    let &(generation, drawn, now) = f.app.resource::<StaleProbe>().frames.last().unwrap();
    assert_eq!(generation, 1);
    assert_eq!(drawn, now, "the drawn frame uses the rebuilt tiles");
}

// ── removal ────────────────────────────────────────────────────────────

#[test]
fn close_and_closed_are_advice_at_the_window() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 64, 64);
    f.send(TOPLEVEL, ev::CLOSE, |_| {});
    f.turn();
    assert_eq!(log(&f).last().unwrap(), &format!("Close Some({win:?})"));
    assert!(f.app.is_live(win), "presentation removes nothing");
    // The second `fake()` locks the same GPU mutex; drop this one's guard
    // first or the second `fake()` deadlocks against itself.
    drop(f);
    drop(_gpu);

    let Some((_gpu, mut f)) = fake() else { return };
    let root = f.app.root();
    let role = Role::Layer(LayerRole {
        layer: Layer::Bottom,
        anchor: Anchor::BOTTOM,
        exclusive_zone: -1,
        namespace: "wall".into(),
        keyboard_interactivity: KeyboardInteractivity::None,
    });
    let win = f.app.spawn_with(root, window(), (role,)).id();
    f.app.tick();
    f.turn();
    f.send(XDG, ev::LAYER_CLOSED, |_| {});
    f.turn();
    assert_eq!(log(&f).last().unwrap(), &format!("Close Some({win:?})"));
}

#[test]
fn removing_a_window_destroys_its_objects_in_order_and_forgets_them() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = configured(&mut f, 320, 200);
    f.requests();
    f.app.remove(win);
    f.app.tick();
    f.turn();
    let s = shape(&mut f);
    assert_eq!(
        s,
        vec![
            (TOPLEVEL, op::TOPLEVEL_DESTROY),
            (XDG, op::XDG_DESTROY),
            (BUF_A, op::BUFFER_DESTROY),
            (BUF_B, op::BUFFER_DESTROY),
            (SURFACE, op::SURFACE_DESTROY),
        ]
    );
    assert!(f.app.resource::<Surfaces>().surface_of(win).is_none());
    // A late release for a destroyed buffer is ignored.
    f.send(BUF_A, ev::RELEASE, |_| {});
    f.turn();
    assert!(shape(&mut f).is_empty());
}

// ── seat ───────────────────────────────────────────────────────────────

#[test]
fn seat_capabilities_requests_pointer_and_touch_objects_once_each() {
    let Some((_gpu, mut f)) = fake() else { return };
    f.requests();

    f.send(SEAT, ev::CAPABILITIES, |w| {
        w.uint(WlSeatCapability::empty().bits())
    });
    f.turn();
    assert!(f.requests().is_empty(), "no capability bits, no request");

    f.send(SEAT, ev::CAPABILITIES, |w| {
        w.uint(WlSeatCapability::POINTER.bits())
    });
    f.turn();
    assert_eq!(shape(&mut f), vec![(SEAT, op::GET_POINTER)]);

    f.send(SEAT, ev::CAPABILITIES, |w| {
        w.uint(WlSeatCapability::POINTER.bits())
    });
    f.turn();
    assert!(
        f.requests().is_empty(),
        "already has a pointer, no second request"
    );

    f.send(SEAT, ev::CAPABILITIES, |w| {
        w.uint((WlSeatCapability::POINTER | WlSeatCapability::TOUCH).bits())
    });
    f.turn();
    assert_eq!(shape(&mut f), vec![(SEAT, op::GET_TOUCH)]);
}

#[test]
fn pointer_enter_motion_button_leave_produce_contact_input_at_the_focused_window() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = spawn(&mut f, LayoutStyle::default().column());
    f.requests();
    let ptr = pointer(&mut f);

    f.send(ptr, ev::POINTER_ENTER, |w| {
        w.uint(1);
        w.object(ObjectId(SURFACE));
        w.fixed(10.0);
        w.fixed(20.0);
    });
    f.turn();
    f.send(ptr, ev::POINTER_MOTION, |w| {
        w.uint(100);
        w.fixed(11.0);
        w.fixed(21.0);
    });
    f.turn();
    f.send(ptr, ev::POINTER_BUTTON, |w| {
        w.uint(3);
        w.uint(140);
        w.uint(0x110);
        w.uint(WlPointerButtonState::Pressed.into());
    });
    f.turn();
    f.send(ptr, ev::POINTER_BUTTON, |w| {
        w.uint(5);
        w.uint(180);
        w.uint(0x110);
        w.uint(WlPointerButtonState::Released.into());
    });
    f.turn();
    f.send(ptr, ev::POINTER_LEAVE, |w| {
        w.uint(7);
        w.object(ObjectId(SURFACE));
    });
    f.turn();

    assert_eq!(
        log(&f),
        vec![
            format!("{win:?} Mouse Moved 10x20"),
            format!("{win:?} Mouse Moved 11x21"),
            format!("{win:?} Mouse Pressed 11x21"),
            format!("{win:?} Mouse Released 11x21"),
            format!("{win:?} Mouse Cancelled 11x21"),
        ]
    );

    // `Enter` and `Leave` have no `time` on the wire: the seat's last
    // one (0 before any) stands in.
    assert_eq!(
        f.app.resource::<ContactTimes>().0,
        vec![0, 100, 140, 180, 180]
    );
}

#[test]
fn a_button_or_leave_with_no_prior_enter_is_a_no_op() {
    let Some((_gpu, mut f)) = fake() else { return };
    spawn(&mut f, LayoutStyle::default().column());
    f.requests();
    let ptr = pointer(&mut f);

    f.send(ptr, ev::POINTER_BUTTON, |w| {
        w.uint(1);
        w.uint(2);
        w.uint(0x110);
        w.uint(WlPointerButtonState::Pressed.into());
    });
    f.send(ptr, ev::POINTER_LEAVE, |w| {
        w.uint(3);
        w.object(ObjectId(SURFACE));
    });
    f.turn();
    assert!(
        log(&f).is_empty(),
        "no pointer_focus on record, so both are no-ops"
    );
}

#[test]
fn touch_down_motion_up_produce_contact_input_in_order() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = spawn(&mut f, LayoutStyle::default().column());
    f.requests();
    let t = touch(&mut f);

    f.send(t, ev::TOUCH_DOWN, |w| {
        w.uint(1);
        w.uint(100);
        w.object(ObjectId(SURFACE));
        w.int(0);
        w.fixed(5.0);
        w.fixed(6.0);
    });
    f.turn();
    f.send(t, ev::TOUCH_MOTION, |w| {
        w.uint(120);
        w.int(0);
        w.fixed(7.0);
        w.fixed(8.0);
    });
    f.turn();
    f.send(t, ev::TOUCH_UP, |w| {
        w.uint(4);
        w.uint(150);
        w.int(0);
    });
    f.turn();
    // A further Motion for the same, now-released id is a no-op.
    f.send(t, ev::TOUCH_MOTION, |w| {
        w.uint(160);
        w.int(0);
        w.fixed(9.0);
        w.fixed(9.0);
    });
    f.turn();

    assert_eq!(
        log(&f),
        vec![
            format!("{win:?} Touch(0) Moved 5x6"),
            format!("{win:?} Touch(0) Pressed 5x6"),
            format!("{win:?} Touch(0) Moved 7x8"),
            format!("{win:?} Touch(0) Released 7x8"),
        ]
    );
    assert_eq!(f.app.resource::<ContactTimes>().0, vec![100, 100, 120, 150]);
}

#[test]
fn touch_cancel_ends_every_active_touch_at_once() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = spawn(&mut f, LayoutStyle::default().column());
    f.requests();
    let t = touch(&mut f);

    f.send(t, ev::TOUCH_DOWN, |w| {
        w.uint(1);
        w.uint(20);
        w.object(ObjectId(SURFACE));
        w.int(0);
        w.fixed(1.0);
        w.fixed(1.0);
    });
    f.send(t, ev::TOUCH_DOWN, |w| {
        w.uint(3);
        w.uint(40);
        w.object(ObjectId(SURFACE));
        w.int(1);
        w.fixed(2.0);
        w.fixed(2.0);
    });
    f.turn();
    f.send(t, ev::TOUCH_CANCEL, |_| {});
    f.turn();

    let entries = log(&f);
    assert!(entries.contains(&format!("{win:?} Touch(0) Cancelled 1x1")));
    assert!(entries.contains(&format!("{win:?} Touch(1) Cancelled 2x2")));
    assert_eq!(
        entries.iter().filter(|e| e.contains("Cancelled")).count(),
        2
    );

    // `Cancel` has no `time`: both cancels carry the last one seen.
    let times = f.app.resource::<ContactTimes>().0.clone();
    assert_eq!(times[times.len() - 2..], [40, 40]);

    // Nothing left to cancel a second time.
    f.send(t, ev::TOUCH_CANCEL, |_| {});
    f.turn();
    assert_eq!(
        log(&f).iter().filter(|e| e.contains("Cancelled")).count(),
        2,
        "an empty touches map cancels nothing further"
    );
}

#[test]
fn a_motion_or_up_for_an_unknown_touch_id_is_a_no_op() {
    let Some((_gpu, mut f)) = fake() else { return };
    spawn(&mut f, LayoutStyle::default().column());
    f.requests();
    let t = touch(&mut f);

    f.send(t, ev::TOUCH_MOTION, |w| {
        w.uint(1);
        w.int(99);
        w.fixed(1.0);
        w.fixed(1.0);
    });
    f.send(t, ev::TOUCH_UP, |w| {
        w.uint(2);
        w.uint(3);
        w.int(99);
    });
    f.turn();
    assert!(log(&f).is_empty(), "id 99 was never seen in a Down");
}

#[test]
fn a_touch_down_and_up_over_a_leaf_fires_press_release_and_clicked() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = spawn(&mut f, LayoutStyle::default().column());

    struct Leaf;
    impl Build for Leaf {
        type Widget = Leaf;
    }
    impl Widget for Leaf {
        type Builder = Leaf;
        fn build(b: Leaf, _: Handle<Self>, _: &mut Spawner<'_, Self>) -> Self {
            b
        }
    }
    let leaf = f
        .app
        .spawn_with(
            win,
            Leaf,
            (LayoutStyle::default().size(px(10.0), px(10.0)),),
        )
        .id();
    f.app.tick();
    f.turn();

    f.requests();
    let t = touch(&mut f);
    f.send(t, ev::TOUCH_DOWN, |w| {
        w.uint(1);
        w.uint(2);
        w.object(ObjectId(SURFACE));
        w.int(0);
        w.fixed(5.0);
        w.fixed(5.0);
    });
    f.turn();
    f.send(t, ev::TOUCH_UP, |w| {
        w.uint(3);
        w.uint(4);
        w.int(0);
    });
    f.turn();

    // `log_contact` also logs the raw `Moved`/`Pressed`/`Released`
    // `ContactInput` lines into the same `Log`; filter down to the
    // dispatched events this test is actually proving.
    let clicks: Vec<_> = log(&f)
        .into_iter()
        .filter(|e| {
            e.starts_with("Press ") || e.starts_with("Release ") || e.starts_with("Clicked ")
        })
        .collect();
    assert_eq!(
        clicks,
        vec![
            format!("Press Some({leaf:?})"),
            format!("Release Some({leaf:?})"),
            format!("Clicked Some({leaf:?})"),
        ]
    );
}

#[test]
fn showing_a_window_hidden_before_its_first_buffer_draws_without_a_new_configure() {
    let Some((_gpu, mut f)) = fake() else { return };
    let root = f.app.root();
    let win = f
        .app
        .spawn_with(
            root,
            window().layout(LayoutStyle::default().column()),
            (layer_role(),),
        )
        .id();
    {
        let (mut surfaces, mut wl) = f.app.query::<(ResMut<Surfaces>, ResMut<Wayland>)>();
        surfaces.set_mapped(win, false, &mut wl);
    }
    f.app.tick();
    f.turn();
    layer_configure(&mut f, 3);
    f.requests();
    set_mapped(&mut f, win, true);
    f.app.tick();
    f.turn();
    assert_eq!(attaches(&mut f), vec![LAYER_BUF_A]);
}

// ── a transparent clear ──────────────────────────────────────────────────

/// A toplevel cleared to transparent, configured to 320 by 200; returns
/// the fourcc each of its two slots was created with.
fn transparent_slot_formats(f: &mut Fake) -> Vec<u32> {
    let root = f.app.root();
    f.app.spawn(
        root,
        window()
            .title("glass")
            .layout(LayoutStyle::default().column())
            .clear(Color::TRANSPARENT),
    );
    f.app.tick();
    f.turn();
    f.requests();
    f.send(TOPLEVEL, ev::TOPLEVEL_CONFIGURE, |w| {
        w.int(320);
        w.int(200);
        w.array(&[]);
    });
    f.send(XDG, ev::XDG_CONFIGURE, |w| w.uint(1));
    f.turn();
    f.requests()
        .iter()
        .filter(|r| r.opcode == op::PARAMS_CREATE_IMMED)
        .map(|r| {
            let mut a = r.reader();
            let (_buffer, _w, _h) = (a.object(), a.int(), a.int());
            a.uint().unwrap()
        })
        .collect()
}

#[test]
fn a_transparent_window_draws_into_argb_buffers() {
    let Some((_gpu, mut f)) = fake() else { return };
    assert_eq!(transparent_slot_formats(&mut f), vec![ARGB, ARGB]);
}

#[test]
fn a_transparent_window_falls_back_to_xrgb_when_argb_is_not_advertised() {
    let Some((_gpu, mut f)) = fake_advertising(true, false) else {
        return;
    };
    assert_eq!(
        transparent_slot_formats(&mut f),
        vec![XRGB, XRGB],
        "shown opaque rather than not at all"
    );
}

#[test]
fn a_transparent_window_draws_into_argb_buffers_when_no_format_is_listed() {
    let Some((_gpu, mut f)) = fake_advertising(false, false) else {
        return;
    };
    assert_eq!(
        transparent_slot_formats(&mut f),
        vec![ARGB, ARGB],
        "nothing listed rules nothing out, as for XRGB8888"
    );
}

/// A drawn toplevel whose callback and buffer came back: idle, nothing
/// wanted, and its next frame reuses the drawn slot at age 1 (see
/// `a_frame_with_nothing_changed_commits_nothing`).
fn idle(f: &mut Fake) -> NodeId {
    let win = configured(f, 320, 200);
    f.send(CALLBACK, ev::DONE, |w| w.uint(1));
    f.send(BUF_A, ev::RELEASE, |_| {});
    f.turn();
    f.requests();
    win
}

#[test]
fn a_frame_with_nothing_to_draw_commits_nothing() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = idle(&mut f);
    f.app.signal(RequestFrame(win));
    f.app.tick();
    f.turn();
    assert!(log(&f).iter().filter(|l| l.starts_with("Frame")).count() >= 2);
    assert!(shape(&mut f).is_empty());
}

#[test]
fn a_requested_commit_is_sent_bare_on_a_frame_with_nothing_to_draw() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = idle(&mut f);
    f.app.resource_mut::<Surfaces>().request_commit(win);
    f.app.signal(RequestFrame(win));
    f.app.tick();
    f.turn();
    assert_eq!(shape(&mut f), vec![(SURFACE, op::COMMIT)]);

    // Honoured once.
    f.app.signal(RequestFrame(win));
    f.app.tick();
    f.turn();
    assert!(shape(&mut f).is_empty());
}

#[test]
fn a_requested_commit_rides_a_frame_that_draws() {
    let Some((_gpu, mut f)) = fake() else { return };
    let win = idle(&mut f);
    f.app.resource_mut::<Surfaces>().request_commit(win);
    repaint(&mut f, win);
    f.turn();
    let commits = f
        .requests()
        .iter()
        .filter(|r| r.sender == ObjectId(SURFACE) && r.opcode == op::COMMIT)
        .count();
    assert_eq!(commits, 1);
}
