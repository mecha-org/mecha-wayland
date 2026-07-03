#[cfg(feature = "static")]
mod r#static;
#[cfg(feature = "static")]
pub use r#static::*;

#[cfg(all(feature = "dynamic", not(feature = "static")))]
mod dynamic;
#[cfg(all(feature = "dynamic", not(feature = "static")))]
pub use dynamic::*;
