#[allow(unused_variables, unused_mut, dead_code, unused_imports)]
pub mod generated;
pub mod manual;

pub use generated::*;
#[cfg(feature = "client")]
pub use manual::client::{WlCallbackEvent, WlDisplayError, WlDisplayEvent, WlRegistryEvent};
#[cfg(feature = "server")]
pub use manual::server::{WlDisplayRequest, WlRegistryRequest};
pub use manual::{WlCallback, WlDisplay, WlRegistry};
