#[derive(Default)]
pub struct Ring;

pub enum IoEvent {
    EventOne,
    EventTwo,
}
impl Event for IoEvent {}

macro_rules! ring_module {
    ($exp:expr) => {
        app::module::Module::<crate::ring::Ring>::new().processor(
            |_: &mut crate::ring::Ring, _: &app::Poll| {
                std::thread::sleep(std::time::Duration::from_secs($exp));
                ring::IoEvent::EventOne
            },
        )
    };
}

use app::event::Event;
pub(crate) use ring_module;
