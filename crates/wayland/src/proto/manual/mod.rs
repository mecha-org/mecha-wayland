pub mod client;
#[cfg(feature = "server")]
pub mod server;

pub use client::client_module;
#[cfg(feature = "server")]
pub use server::server_dispatch_module;

use crate::Interface;

// ── Read helpers shared by client and server parse functions ──────────────────

pub(crate) fn read_u32(data: &[u8], offset: &mut usize) -> Option<u32> {
    let bytes = data.get(*offset..*offset + 4)?;
    *offset += 4;
    Some(u32::from_ne_bytes(bytes.try_into().unwrap()))
}

pub(crate) fn read_string(data: &[u8], offset: &mut usize) -> Option<String> {
    let len = read_u32(data, offset)? as usize;
    let padded = (len + 3) & !3;
    let raw = data.get(*offset..*offset + padded)?;
    *offset += padded;
    let s = std::str::from_utf8(raw.get(..len.saturating_sub(1))?).ok()?;
    Some(s.to_owned())
}

// ── wl_display ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct WlDisplay;
impl Interface for WlDisplay {
    const NAME: &'static str = "wl_display";
    const VERSION: u32 = 1;
}

// ── wl_callback ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct WlCallback;
impl Interface for WlCallback {
    const NAME: &'static str = "wl_callback";
    const VERSION: u32 = 1;
}

// ── wl_registry ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct WlRegistry;
impl Interface for WlRegistry {
    const NAME: &'static str = "wl_registry";
    const VERSION: u32 = 1;
}
