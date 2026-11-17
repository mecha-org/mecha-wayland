use app::prelude::*;
use toplevels::prelude::*;
use wayland::fake::Fake;
use wayland::prelude::*;

/// After the seat (4) and the wlr manager (5).
const ACTIVATION: u32 = 6;

const OP_GET_TOKEN: u16 = 1;
const OP_SET_APP_ID: u16 = 1;
const OP_COMMIT: u16 = 3;
const OP_TOKEN_DESTROY: u16 = 4;
const EV_DONE: u16 = 0;

#[derive(Default)]
struct Tokens(Vec<ActivationToken>);
impl Resource for Tokens {}

fn fake_with(globals: &[(&str, u32)]) -> Fake {
    let mut f = Fake::new(globals, |m| m.bind::<WlSeat>());
    f.app.add_module(ToplevelsModule);
    f.app
        .init_resource::<Tokens>()
        .system(|app: &mut App, t: &ActivationToken| {
            app.resource_mut::<Tokens>().0.push(t.clone())
        });
    f.turn();
    f.requests();
    f
}

fn request(f: &mut Fake, app_id: &str) -> Option<TokenRequest> {
    let (t, mut wl) = f.app.query::<(Res<Toplevels>, ResMut<Wayland>)>();
    t.request_activation_token(&mut wl, app_id)
}

#[test]
fn a_token_is_asked_for_an_app_and_comes_back() {
    let mut f = fake_with(&[
        ("wl_seat", 9),
        ("zwlr_foreign_toplevel_manager_v1", 3),
        ("xdg_activation_v1", 1),
    ]);
    assert!(f.app.resource::<Toplevels>().activation_available());
    let asked = request(&mut f, "org.example.Notes").expect("available");
    f.turn();
    let get = f.expect(ACTIVATION, OP_GET_TOKEN);
    let token = get.reader().object().expect("new id");
    let set = f.expect(token.0, OP_SET_APP_ID);
    assert_eq!(set.reader().string().as_deref(), Some("org.example.Notes"));
    f.expect(token.0, OP_COMMIT);

    f.send(token.0, EV_DONE, |w| w.string("tok-1"));
    f.turn();
    f.app.tick();
    let tokens = &f.app.resource::<Tokens>().0;
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].request, asked);
    assert_eq!(tokens[0].token, "tok-1");
    f.turn();
    f.expect(token.0, OP_TOKEN_DESTROY);
}

#[test]
fn without_the_global_there_are_no_tokens() {
    let mut f = fake_with(&[("wl_seat", 9), ("zwlr_foreign_toplevel_manager_v1", 3)]);
    assert!(!f.app.resource::<Toplevels>().activation_available());
    assert!(request(&mut f, "org.example.Notes").is_none());
}
