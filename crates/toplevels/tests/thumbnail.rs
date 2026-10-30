use app::prelude::*;
use toplevels::prelude::*;
use wayland::fake::Fake;
use wayland::prelude::*;

const PHOSH: u32 = 6;
const LAYER: u32 = 0x77;
const A: u32 = 0xff00_0000;

const OP_SET_PROPERTIES: u16 = 0;
const OP_THUMB_DESTROY: u16 = 1;

/// The opcode of `phosh_private.add_thumbnail_view` as the vendored XML
/// orders its requests.
fn upstream_opcode() -> u16 {
    let xml = include_str!("../../wayland/protocols/phosh-private.xml");
    let start = xml
        .find("<interface name=\"phosh_private\"")
        .expect("phosh_private interface");
    let body = &xml[start..];
    let body = &body[..body.find("</interface>").expect("interface end")];
    body.split("<request name=\"")
        .skip(1)
        .position(|r| r.starts_with("add_thumbnail_view\""))
        .expect("add_thumbnail_view request") as u16
}

fn fake_with(globals: &[(&str, u32)]) -> Fake {
    let mut f = Fake::new(globals, |m| m.bind::<WlSeat>());
    f.app.add_module(ToplevelsModule);
    f.turn();
    f.requests();
    f
}

fn fake() -> Fake {
    fake_with(&[
        ("wl_seat", 9),
        ("zwlr_foreign_toplevel_manager_v1", 3),
        ("phosh_private", 8),
    ])
}

/// Announce toplevel `A` and publish it.
fn open_a(f: &mut Fake) {
    f.send(5, 0, |w| w.new_id(ObjectId(A)));
    f.send(A, 5, |_| {});
    f.turn();
    f.requests();
}

fn layer() -> ZwlrLayerSurfaceV1 {
    ZwlrLayerSurfaceV1(ObjectId(LAYER))
}

#[test]
fn add_thumbnail_sends_request() {
    let mut f = fake();
    open_a(&mut f);
    let t = f.app.resource::<Toplevels>();
    assert!(t.thumbnails_available());
    let thumb = {
        let (t, mut wl) = f.app.query::<(Res<Toplevels>, ResMut<Wayland>)>();
        t.add_thumbnail(&mut wl, ToplevelId::from_raw(A), layer())
            .expect("available")
    };
    f.turn();
    let add = f.expect(PHOSH, upstream_opcode());
    let mut r = add.reader();
    let new_id = r.object().expect("new id");
    assert_eq!(r.object(), Some(ObjectId(A)));
    assert_eq!(r.object(), Some(ObjectId(LAYER)));

    {
        let mut wl = f.app.resource_mut::<Wayland>();
        thumb.set(&mut wl, 12, 34, 0.5, 1.0);
    }
    f.turn();
    let set = f.expect(new_id.0, OP_SET_PROPERTIES);
    let mut r = set.reader();
    assert_eq!(r.int(), Some(12));
    assert_eq!(r.int(), Some(34));
    assert_eq!(r.fixed(), Some(0.5));
    assert_eq!(r.fixed(), Some(1.0));
    // 0.5 and 1.0 in 24.8 fixed point.
    assert_eq!(&set.body[8..], &[128, 0, 0, 0, 0, 1, 0, 0]);

    {
        let mut wl = f.app.resource_mut::<Wayland>();
        thumb.destroy(&mut wl);
    }
    f.turn();
    f.expect(new_id.0, OP_THUMB_DESTROY);
}

#[test]
fn no_phosh_private_means_no_thumbnails() {
    let mut f = fake_with(&[("wl_seat", 9), ("zwlr_foreign_toplevel_manager_v1", 3)]);
    assert!(!f.app.resource::<Toplevels>().thumbnails_available());
    let (t, mut wl) = f.app.query::<(Res<Toplevels>, ResMut<Wayland>)>();
    assert!(
        t.add_thumbnail(&mut wl, ToplevelId::from_raw(A), layer())
            .is_none()
    );
}

#[test]
fn old_phosh_private_is_ignored() {
    let mut f = fake_with(&[
        ("wl_seat", 9),
        ("zwlr_foreign_toplevel_manager_v1", 3),
        ("phosh_private", 7),
    ]);
    assert!(!f.app.resource::<Toplevels>().thumbnails_available());
    let (t, mut wl) = f.app.query::<(Res<Toplevels>, ResMut<Wayland>)>();
    assert!(
        t.add_thumbnail(&mut wl, ToplevelId::from_raw(A), layer())
            .is_none()
    );
}

#[test]
fn add_thumbnail_after_closed_is_none() {
    let mut f = fake();
    open_a(&mut f);
    f.send(A, 6, |_| {});
    f.turn();
    f.requests();
    {
        let (t, mut wl) = f.app.query::<(Res<Toplevels>, ResMut<Wayland>)>();
        assert!(
            t.add_thumbnail(&mut wl, ToplevelId::from_raw(A), layer())
                .is_none()
        );
    }
    f.turn();
    let sent = f.requests();
    assert!(sent.is_empty(), "requests after closed: {sent:?}");
}
