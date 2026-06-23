#[allow(unused_variables, unused_mut, dead_code, unused_imports)]
pub mod generated;
pub mod manual;

pub use generated::*;
// Re-export manual types explicitly to avoid conflicting with generated::module().
pub use manual::{WlCallback, WlDisplay, WlRegistry};
#[cfg(feature = "client")]
pub use manual::{WlCallbackEvent, WlDisplayError, WlDisplayEvent, WlRegistryEvent};
#[cfg(feature = "server")]
pub use manual::{WlDisplayRequest, WlRegistryRequest};
