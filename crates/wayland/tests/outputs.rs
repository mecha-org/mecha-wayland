//! `OutputsModule` against the fake compositor.

use app::prelude::*;
use wayland::fake::Fake;
use wayland::prelude::*;

/// One output, registry name 1, version 4.
const GLOBALS: &[(&str, u32)] = &[("wl_output", 4)];
const OUTPUT_GLOBAL: u32 = 1;
const REGISTRY: u32 = 2;

mod op {
    pub const REGISTRY_BIND: u16 = 0;
    pub const OUTPUT_RELEASE: u16 = 0;
}
mod ev {
    pub const GLOBAL: u16 = 0;
    pub const GLOBAL_REMOVE: u16 = 1;
    pub const OUTPUT_DONE: u16 = 2;
}

#[derive(Default)]
struct Log(Vec<String>);
impl Resource for Log {}

fn log_added(app: &mut App, e: &OutputAdded) {
    let s = format!("added {}", e.0.id().0);
    app.resource_mut::<Log>().0.push(s);
}
fn log_removed(app: &mut App, e: &OutputRemoved) {
    let s = format!("removed {}", e.0.id().0);
    app.resource_mut::<Log>().0.push(s);
}

/// The fake with `OutputsModule` installed, and the id the output was
/// bound as.
fn fake() -> (Fake, u32) {
    let mut f = Fake::new(GLOBALS, |m| m);
    f.app
        .init_resource::<Log>()
        .system(log_added)
        .system(log_removed)
        .add_module(OutputsModule);
    f.turn();
    let output = bound_id(&mut f);
    (f, output)
}

/// The id of the next `wl_registry.bind`.
fn bound_id(f: &mut Fake) -> u32 {
    let bind = f.expect(REGISTRY, op::REGISTRY_BIND);
    let mut r = bind.reader();
    r.uint();
    r.string();
    r.uint();
    r.object().unwrap().0
}

fn log(f: &mut Fake) -> Vec<String> {
    std::mem::take(&mut f.app.resource_mut::<Log>().0)
}

fn outputs(f: &Fake) -> Vec<WlOutput> {
    f.app.resource::<Outputs>().iter().collect()
}

#[test]
fn an_output_is_announced_on_its_first_done_only() {
    let (mut f, output) = fake();
    assert!(outputs(&f).is_empty());
    f.send(output, ev::OUTPUT_DONE, |_| {});
    f.turn();
    assert_eq!(log(&mut f), vec![format!("added {output}")]);
    assert_eq!(outputs(&f), vec![WlOutput(ObjectId(output))]);
    f.send(output, ev::OUTPUT_DONE, |_| {});
    f.turn();
    assert!(log(&mut f).is_empty(), "a later done announces nothing");
}

#[test]
fn a_removed_output_is_released_and_reported() {
    let (mut f, output) = fake();
    f.send(output, ev::OUTPUT_DONE, |_| {});
    f.turn();
    log(&mut f);
    f.send(REGISTRY, ev::GLOBAL_REMOVE, |w| w.uint(OUTPUT_GLOBAL));
    f.turn();
    f.expect(output, op::OUTPUT_RELEASE);
    assert_eq!(log(&mut f), vec![format!("removed {output}")]);
    assert!(outputs(&f).is_empty());
}

#[test]
fn an_output_advertised_later_is_bound() {
    let (mut f, _) = fake();
    f.send(REGISTRY, ev::GLOBAL, |w| {
        w.uint(40);
        w.string("wl_output");
        w.uint(4);
    });
    f.turn();
    let later = bound_id(&mut f);
    f.send(later, ev::OUTPUT_DONE, |_| {});
    f.turn();
    assert_eq!(log(&mut f), vec![format!("added {later}")]);
}
