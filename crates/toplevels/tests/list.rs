use app::prelude::*;
use toplevels::prelude::*;
use wayland::fake::Fake;
use wayland::prelude::*;

const SEAT: u32 = 4;
const MANAGER: u32 = 5;
const A: u32 = 0xff00_0000;
const B: u32 = 0xff00_0001;

const OP_ACTIVATE: u16 = 4;
const OP_CLOSE: u16 = 5;
const OP_DESTROY: u16 = 7;

const EV_TITLE: u16 = 0;
const EV_APP_ID: u16 = 1;
const EV_STATE: u16 = 4;
const EV_DONE: u16 = 5;
const EV_CLOSED: u16 = 6;
const EV_PARENT: u16 = 7;

#[derive(Default)]
struct Changes(u32);
impl Resource for Changes {}

fn count(app: &mut App, _: &ToplevelsChanged) {
    app.resource_mut::<Changes>().0 += 1;
}

/// Turn, then the runner's next tick, which is where the batched signal
/// goes out.
fn settle(f: &mut Fake) {
    f.turn();
    f.app.tick();
}

fn fake_with(globals: &[(&str, u32)]) -> Fake {
    let mut f = Fake::new(globals, |m| m.bind::<WlSeat>());
    f.app.add_module(ToplevelsModule);
    f.app.init_resource::<Changes>().system(count);
    f.turn();
    f.requests();
    f
}

fn fake() -> Fake {
    fake_with(&[("wl_seat", 9), ("zwlr_foreign_toplevel_manager_v1", 3)])
}

fn open(f: &mut Fake, id: u32) {
    f.send(MANAGER, 0, |w| w.new_id(ObjectId(id)));
}
fn title(f: &mut Fake, id: u32, s: &str) {
    f.send(id, EV_TITLE, |w| w.string(s));
}
fn app_id(f: &mut Fake, id: u32, s: &str) {
    f.send(id, EV_APP_ID, |w| w.string(s));
}
fn state(f: &mut Fake, id: u32, activated: bool) {
    f.send(id, EV_STATE, |w| {
        if activated {
            w.array(&2u32.to_ne_bytes());
        } else {
            w.array(&[]);
        }
    });
}
/// A `state` event carrying these raw state values.
fn states(f: &mut Fake, id: u32, values: &[u32]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
    f.send(id, EV_STATE, |w| w.array(&bytes));
}
fn done(f: &mut Fake, id: u32) {
    f.send(id, EV_DONE, |_| {});
}
fn changes(f: &Fake) -> u32 {
    f.app.resource::<Changes>().0
}
fn get(f: &Fake, id: u32) -> ToplevelInfo {
    f.app
        .resource::<Toplevels>()
        .get(ToplevelId::from_raw(id))
        .cloned()
        .expect("toplevel listed")
}

#[test]
fn a_toplevel_appears_after_done() {
    let mut f = fake();
    assert!(f.app.resource::<Toplevels>().available());
    open(&mut f, A);
    title(&mut f, A, "Notes");
    app_id(&mut f, A, "org.mecha.Notes");
    settle(&mut f);
    assert_eq!(f.app.resource::<Toplevels>().iter().count(), 0);
    assert_eq!(changes(&f), 0);
    done(&mut f, A);
    settle(&mut f);
    let t = f.app.resource::<Toplevels>();
    let all: Vec<_> = t.iter().collect();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].title, "Notes");
    assert_eq!(all[0].app_id, "org.mecha.Notes");
    assert!(!all[0].activated);
    assert_eq!(all[0].last_activated, 0);
    assert_eq!(changes(&f), 1);
}

#[test]
fn fullscreen_state_is_reported_and_cleared() {
    // wlr state values: 2 activated, 3 fullscreen.
    let mut f = fake();
    open(&mut f, A);
    states(&mut f, A, &[2, 3]);
    done(&mut f, A);
    settle(&mut f);
    let a = get(&f, A);
    assert!(a.activated && a.fullscreen);
    states(&mut f, A, &[2]);
    done(&mut f, A);
    settle(&mut f);
    let a = get(&f, A);
    assert!(a.activated && !a.fullscreen);
}

#[test]
fn activated_state_stamps_last_activated() {
    let mut f = fake();
    for id in [A, B] {
        open(&mut f, id);
        state(&mut f, id, false);
        done(&mut f, id);
    }
    settle(&mut f);
    state(&mut f, A, true);
    done(&mut f, A);
    settle(&mut f);
    state(&mut f, A, false);
    done(&mut f, A);
    state(&mut f, B, true);
    done(&mut f, B);
    settle(&mut f);
    let (a, b) = (get(&f, A), get(&f, B));
    assert!(b.activated && !a.activated);
    assert!(b.last_activated > a.last_activated);
    assert!(a.last_activated > 0);
}

#[test]
fn a_repeated_done_does_not_restamp_or_signal() {
    let mut f = fake();
    open(&mut f, A);
    state(&mut f, A, true);
    done(&mut f, A);
    settle(&mut f);
    let stamp = get(&f, A).last_activated;
    let n = changes(&f);
    state(&mut f, A, true);
    done(&mut f, A);
    settle(&mut f);
    assert_eq!(get(&f, A).last_activated, stamp);
    assert_eq!(changes(&f), n);
}

#[test]
fn parent_is_recorded() {
    let mut f = fake();
    open(&mut f, A);
    done(&mut f, A);
    open(&mut f, B);
    f.send(B, EV_PARENT, |w| w.object_opt(Some(ObjectId(A))));
    done(&mut f, B);
    settle(&mut f);
    assert_eq!(get(&f, A).parent, None);
    assert_eq!(get(&f, B).parent, Some(ToplevelId::from_raw(A)));
}

#[test]
fn closed_toplevel_is_removed() {
    let mut f = fake();
    open(&mut f, A);
    done(&mut f, A);
    settle(&mut f);
    assert_eq!(changes(&f), 1);
    f.send(A, EV_CLOSED, |_| {});
    settle(&mut f);
    assert_eq!(f.app.resource::<Toplevels>().iter().count(), 0);
    assert_eq!(changes(&f), 2);
    f.expect(A, OP_DESTROY);
}

#[test]
fn a_toplevel_closed_before_done_is_destroyed_without_a_signal() {
    let mut f = fake();
    open(&mut f, A);
    f.send(A, EV_CLOSED, |_| {});
    settle(&mut f);
    assert_eq!(changes(&f), 0);
    f.expect(A, OP_DESTROY);
}

#[test]
fn manager_finished_clears_the_list() {
    let mut f = fake();
    open(&mut f, A);
    done(&mut f, A);
    settle(&mut f);
    f.send(MANAGER, 1, |_| {});
    settle(&mut f);
    assert_eq!(f.app.resource::<Toplevels>().iter().count(), 0);
    assert!(!f.app.resource::<Toplevels>().available());
    assert_eq!(changes(&f), 2);
}

#[test]
fn activate_and_close_send_requests() {
    let mut f = fake();
    open(&mut f, A);
    done(&mut f, A);
    settle(&mut f);
    let id = ToplevelId::from_raw(A);
    let seat = *f.app.resource::<WlSeat>();
    {
        let (t, mut wl) = f.app.query::<(Res<Toplevels>, ResMut<Wayland>)>();
        t.activate(&mut wl, seat, id);
        t.close(&mut wl, id);
    }
    settle(&mut f);
    let activate = f.expect(A, OP_ACTIVATE);
    assert_eq!(activate.reader().object(), Some(ObjectId(SEAT)));
    f.expect(A, OP_CLOSE);
}

#[test]
fn missing_manager_is_unavailable() {
    let f = fake_with(&[("wl_seat", 9)]);
    let t = f.app.resource::<Toplevels>();
    assert!(!t.available());
    assert_eq!(t.iter().count(), 0);
}

#[test]
fn a_burst_of_changes_is_one_signal() {
    let mut f = fake();
    open(&mut f, A);
    done(&mut f, A);
    open(&mut f, B);
    done(&mut f, B);
    f.send(A, EV_CLOSED, |_| {});
    settle(&mut f);
    assert_eq!(f.app.resource::<Toplevels>().iter().count(), 1);
    assert_eq!(changes(&f), 1);
}

#[test]
fn activate_and_close_after_closed_send_nothing() {
    let mut f = fake();
    open(&mut f, A);
    done(&mut f, A);
    settle(&mut f);
    f.send(A, EV_CLOSED, |_| {});
    // Handled in the same dispatch as `closed`, before any PostTick.
    f.turn();
    f.expect(A, OP_DESTROY);
    let id = ToplevelId::from_raw(A);
    let seat = *f.app.resource::<WlSeat>();
    {
        let (t, mut wl) = f.app.query::<(Res<Toplevels>, ResMut<Wayland>)>();
        t.activate(&mut wl, seat, id);
        t.close(&mut wl, id);
    }
    settle(&mut f);
    let sent: Vec<_> = f
        .requests()
        .into_iter()
        .filter(|r| r.sender == ObjectId(A))
        .collect();
    assert!(sent.is_empty(), "requests on a destroyed handle: {sent:?}");
}
