//! Work a pass leaves behind runs before the runner blocks, not at the
//! next unrelated wake.

use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use app::prelude::*;
use ring::Ring;
use ring::prelude::*;

/// Written once after its own drain, as a `LayoutDone` handler writes.
#[derive(Default)]
struct Late(bool);
impl Resource for Late {}

/// When the runner started.
struct Started(Instant);
impl Resource for Started {}

/// How long after the start `Late`'s change was heard.
static HEARD_AFTER: Mutex<Option<Duration>> = Mutex::new(None);

/// A `PostTick` system added after `Late`'s drain, so it runs after it.
fn write_late_once(app: &mut App, _: &PostTick) {
    if !app.resource::<Late>().0 {
        app.resource_mut::<Late>().0 = true;
    }
}

fn heard_late(app: &mut App, _: &OnChanged<Late>) {
    *HEARD_AFTER.lock().unwrap() = Some(app.resource::<Started>().0.elapsed());
    app.signal(Stop);
}

#[test]
fn a_write_after_its_drain_is_heard_without_waiting_for_another_wake() {
    let (done, stopped) = mpsc::channel();
    std::thread::spawn(move || {
        let mut app = App::new();
        app.add_module(RingModule::default())
            .init_resource::<Late>()
            .system(write_late_once)
            .system(heard_late);
        app.insert_resource(Started(Instant::now()));
        // The only other wake, as an unrelated event would be on an idle
        // device.
        let _ = app
            .resource_mut::<Ring>()
            .timeout(Duration::from_millis(500));
        app.run();
        let _ = done.send(());
    });
    stopped
        .recv_timeout(Duration::from_secs(5))
        .expect("the runner stopped");
    let heard = HEARD_AFTER.lock().unwrap().expect("the change was heard");
    assert!(
        heard < Duration::from_millis(250),
        "heard {heard:?} after the start: the write waited for the next wake"
    );
}
