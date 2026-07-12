// ---------------------------------------------------------------------------
// DBUS Helpers
// ---------------------------------------------------------------------------

use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
};
use zbus::zvariant::{OwnedValue, Value};

/// Total on-wire length of the D-Bus message at the front of `buf`, or `None`
/// if fewer than the fixed 16-byte prefix have arrived.
///
/// Fixed header layout (D-Bus):
///   byte  0      : endianness
///   bytes 4..8   : u32 body length
///   bytes 12..16 : u32 header-fields array length
/// Total = 16 + align8(fields_len) + body_len.
pub fn dbus_message_len(buf: &[u8]) -> Option<usize> {
    if buf.len() < 16 {
        return None;
    }
    let le = match buf[0] {
        b'l' => true,
        b'B' => false,
        _ => return None, // desync
    };
    let rd = |o: usize| -> u32 {
        let b = [buf[o], buf[o + 1], buf[o + 2], buf[o + 3]];
        if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    let body_len = rd(4) as usize;
    let fields_len = rd(12) as usize;
    let header_len = 16 + fields_len;
    let padded_header = (header_len + 7) & !7; // align body to 8 bytes
    Some(padded_header + body_len)
}

/// Pull the `unix:path=…` (or `abstract=…`) component out of a bus address.
pub fn parse_unix_path(addr: &str) -> Option<String> {
    for transport in addr.split(';') {
        let transport = transport.trim();
        if let Some(rest) = transport.strip_prefix("unix:") {
            for kv in rest.split(',') {
                let mut it = kv.splitn(2, '=');
                match (it.next(), it.next()) {
                    (Some("path"), Some(p)) => return Some(p.to_string()),
                    (Some("abstract"), Some(p)) => return Some(format!("\0{p}")),
                    _ => {}
                }
            }
        }
    }
    None
}

/// Blocking SASL EXTERNAL handshake on a new stream.
pub fn sasl_handshake(stream: &mut UnixStream) -> std::io::Result<bool> {
    stream.write_all(&[0u8])?; // mandatory leading null

    let uid = unsafe { libc::getuid() };
    let uid_hex: String = uid
        .to_string()
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect();
    stream.write_all(format!("AUTH EXTERNAL {uid_hex}\r\n").as_bytes())?;
    read_sasl_line(stream)?; // expect: OK <guid>

    stream.write_all(b"NEGOTIATE_UNIX_FD\r\n")?;
    let unix_fd = read_sasl_line(stream)?.starts_with("AGREE_UNIX_FD");

    stream.write_all(b"BEGIN\r\n")?;
    Ok(unix_fd)
}

/// Read one CRLF-terminated SASL line (blocking, byte-at-a-time; runs once).
fn read_sasl_line(stream: &mut UnixStream) -> std::io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte)?;
        if byte[0] == b'\n' {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            break;
        }
        line.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

// -----------------------------------------------------------------------
// zvariant helpers
// -----------------------------------------------------------------------

/// Extract a typed property from an `a{sv}` map (any `T: TryFrom<OwnedValue>`:
/// u32, u64, bool, String, object paths, …).
pub fn prop<T: TryFrom<OwnedValue>>(props: &HashMap<String, OwnedValue>, key: &str) -> Option<T> {
    props.get(key).and_then(|v| T::try_from(v.clone()).ok())
}

pub fn prop_u32(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u32> {
    prop(props, key)
}

pub fn prop_string(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    prop(props, key)
}

/// Wrap a scalar/string/etc. into an `OwnedValue` (a D-Bus variant `v`)
/// Use for every value kind except `Fd`
pub fn variant<'a, T>(v: T) -> OwnedValue
where
    T: Into<Value<'a>>,
{
    OwnedValue::try_from(v.into()).expect("value -> owned value")
}

/// Non-panicking [`variant`], use when wrapping an `Fd` value
pub fn try_variant<'a, T>(v: T) -> Option<OwnedValue>
where
    T: Into<Value<'a>>,
{
    OwnedValue::try_from(v.into()).ok()
}
