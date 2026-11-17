//! One message: the fixed header, the header fields, the body and the
//! fds it carries, and how they go on and come off the wire.

use std::collections::VecDeque;
use std::ops::BitOr;
use std::os::fd::OwnedFd;

use crate::fd::UnixFd;
use crate::names::{BusName, ErrorName, InterfaceName, MemberName, ObjectPath};
use crate::signature::{self, Signature};
use crate::types::{Body, FromBody};
use crate::value::Value;
use crate::wire::{DecodeError, Decoder, Encoder};

/// The specification's ceiling on one message, header included.
pub const MAX_MESSAGE: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageKind {
    MethodCall = 1,
    MethodReturn = 2,
    Error = 3,
    Signal = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags(pub u8);

impl Flags {
    pub const NO_REPLY_EXPECTED: Flags = Flags(0x1);
    pub const NO_AUTO_START: Flags = Flags(0x2);
    pub const ALLOW_INTERACTIVE_AUTHORIZATION: Flags = Flags(0x4);

    pub fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for Flags {
    type Output = Flags;
    fn bitor(self, rhs: Flags) -> Flags {
        Flags(self.0 | rhs.0)
    }
}

/// A message, sent or received. The connection assigns a sent message's
/// serial; the bus daemon fills a received one's sender.
#[derive(Debug, Clone)]
pub struct Message {
    kind: MessageKind,
    flags: Flags,
    serial: u32,
    path: Option<ObjectPath>,
    interface: Option<InterfaceName>,
    member: Option<MemberName>,
    error_name: Option<ErrorName>,
    reply_serial: Option<u32>,
    destination: Option<BusName>,
    sender: Option<BusName>,
    signature: Signature,
    body: Vec<u8>,
    big_endian: bool,
    fds: Vec<UnixFd>,
}

impl Message {
    fn empty(kind: MessageKind) -> Message {
        Message {
            kind,
            flags: Flags::default(),
            serial: 0,
            path: None,
            interface: None,
            member: None,
            error_name: None,
            reply_serial: None,
            destination: None,
            sender: None,
            signature: Signature::default(),
            body: Vec::new(),
            big_endian: false,
            fds: Vec::new(),
        }
    }

    pub fn method_call(path: impl Into<ObjectPath>, member: impl Into<MemberName>) -> Message {
        let mut m = Message::empty(MessageKind::MethodCall);
        m.path = Some(path.into());
        m.member = Some(member.into());
        m
    }

    pub fn signal(
        path: impl Into<ObjectPath>,
        interface: impl Into<InterfaceName>,
        member: impl Into<MemberName>,
    ) -> Message {
        let mut m = Message::empty(MessageKind::Signal);
        m.path = Some(path.into());
        m.interface = Some(interface.into());
        m.member = Some(member.into());
        m
    }

    pub fn method_return(reply_serial: u32) -> Message {
        let mut m = Message::empty(MessageKind::MethodReturn);
        m.reply_serial = Some(reply_serial);
        m
    }

    pub fn error(reply_serial: u32, name: impl Into<ErrorName>, text: &str) -> Message {
        let mut m = Message::empty(MessageKind::Error);
        m.reply_serial = Some(reply_serial);
        m.error_name = Some(name.into());
        m.with_body(&(text,))
    }

    pub fn with_interface(mut self, interface: impl Into<InterfaceName>) -> Message {
        self.interface = Some(interface.into());
        self
    }

    pub fn with_destination(mut self, destination: impl Into<BusName>) -> Message {
        self.destination = Some(destination.into());
        self
    }

    pub(crate) fn with_destination_opt(mut self, destination: Option<BusName>) -> Message {
        self.destination = destination;
        self
    }

    /// Set by the bus daemon on real buses; for tests and the fake.
    pub fn with_sender(mut self, sender: impl Into<BusName>) -> Message {
        self.sender = Some(sender.into());
        self
    }

    pub fn with_flags(mut self, flags: Flags) -> Message {
        self.flags = flags;
        self
    }

    pub fn with_body<B: Body>(mut self, body: &B) -> Message {
        let mut e = Encoder::new();
        body.encode_body(&mut e);
        let (bytes, fds) = e.into_parts();
        self.signature = B::body_signature();
        self.body = bytes;
        self.fds = fds;
        self.big_endian = false;
        self
    }

    /// A body of values known only at run time, each encoded by its signature.
    pub fn with_values(mut self, values: &[Value]) -> Message {
        let mut e = Encoder::new();
        let mut signature = String::new();
        for value in values {
            signature.push_str(value.signature().as_str());
            value.encode_raw(&mut e);
        }
        let (bytes, fds) = e.into_parts();
        self.signature = Signature::from_valid(signature);
        self.body = bytes;
        self.fds = fds;
        self.big_endian = false;
        self
    }

    /// A body as given, unchecked: for tests that need a malformed one.
    pub fn with_raw_body(mut self, signature: impl Into<Signature>, body: Vec<u8>) -> Message {
        self.signature = signature.into();
        self.body = body;
        self.big_endian = false;
        self
    }

    pub fn kind(&self) -> MessageKind {
        self.kind
    }

    pub fn serial(&self) -> u32 {
        self.serial
    }

    pub fn flags(&self) -> Flags {
        self.flags
    }

    pub fn path(&self) -> Option<&ObjectPath> {
        self.path.as_ref()
    }

    pub fn interface(&self) -> Option<&InterfaceName> {
        self.interface.as_ref()
    }

    pub fn member(&self) -> Option<&MemberName> {
        self.member.as_ref()
    }

    pub fn error_name(&self) -> Option<&ErrorName> {
        self.error_name.as_ref()
    }

    pub fn reply_serial(&self) -> Option<u32> {
        self.reply_serial
    }

    pub fn destination(&self) -> Option<&BusName> {
        self.destination.as_ref()
    }

    pub fn sender(&self) -> Option<&BusName> {
        self.sender.as_ref()
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    pub fn fds(&self) -> &[UnixFd] {
        &self.fds
    }

    pub fn is_big_endian(&self) -> bool {
        self.big_endian
    }

    /// The body as `T`, a tuple of the arguments.
    pub fn body<T: FromBody>(&self) -> Result<T, DecodeError> {
        let want = T::body_signature();
        if want != self.signature {
            return Err(DecodeError::Mismatch {
                expected: want.as_str().to_owned(),
                found: self.signature.as_str().to_owned(),
            });
        }
        let mut d = Decoder::new(&self.body, self.big_endian, &self.fds);
        let v = T::decode_body(&mut d)?;
        if !d.is_empty() {
            return Err(DecodeError::Trailing(d.remaining()));
        }
        Ok(v)
    }

    /// Every argument, whatever the signature.
    pub fn body_values(&self) -> Result<Vec<Value>, DecodeError> {
        let mut d = Decoder::new(&self.body, self.big_endian, &self.fds);
        let values = signature::split(self.signature.as_str())
            .map(|t| Value::decode_raw(t, &mut d))
            .collect::<Result<Vec<_>, _>>()?;
        if !d.is_empty() {
            return Err(DecodeError::Trailing(d.remaining()));
        }
        Ok(values)
    }

    /// An error's message: its first argument, when that is a string.
    pub fn error_text(&self) -> Option<String> {
        self.string_arg(0)
    }

    /// Argument `n`, when it is a string (match rules' `argN`).
    pub(crate) fn string_arg(&self, n: usize) -> Option<String> {
        match self.body_values().ok()?.into_iter().nth(n)? {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The whole message with `serial`, and the fds to send with it.
    /// For messages built here; a received big-endian message must be
    /// rebuilt with `with_body`, since its body is not little-endian.
    pub fn encode(&self, serial: u32) -> (Vec<u8>, Vec<UnixFd>) {
        assert!(
            !self.big_endian,
            "dbus: a received big-endian message cannot be re-sent as is; rebuild it with with_body"
        );
        let mut e = Encoder::new();
        e.u8(b'l');
        e.u8(self.kind as u8);
        e.u8(self.flags.0);
        e.u8(1);
        e.u32(self.body.len() as u32);
        e.u32(serial);
        e.array(8, |e| {
            if let Some(p) = &self.path {
                field(e, 1, "o", |e| e.str(p.as_str()));
            }
            if let Some(i) = &self.interface {
                field(e, 2, "s", |e| e.str(i.as_str()));
            }
            if let Some(m) = &self.member {
                field(e, 3, "s", |e| e.str(m.as_str()));
            }
            if let Some(n) = &self.error_name {
                field(e, 4, "s", |e| e.str(n.as_str()));
            }
            if let Some(r) = self.reply_serial {
                field(e, 5, "u", |e| e.u32(r));
            }
            if let Some(d) = &self.destination {
                field(e, 6, "s", |e| e.str(d.as_str()));
            }
            if let Some(s) = &self.sender {
                field(e, 7, "s", |e| e.str(s.as_str()));
            }
            if !self.signature.is_empty() {
                field(e, 8, "g", |e| e.signature(self.signature.as_str()));
            }
            if !self.fds.is_empty() {
                field(e, 9, "u", |e| e.u32(self.fds.len() as u32));
            }
        });
        e.align(8);
        let (mut bytes, _) = e.into_parts();
        bytes.extend_from_slice(&self.body);
        (bytes, self.fds.clone())
    }

    /// The length of the message starting `bytes`: `None` until the
    /// fixed header and the whole message are there.
    pub fn frame_len(bytes: &[u8]) -> Result<Option<usize>, DecodeError> {
        if bytes.len() < 16 {
            return Ok(None);
        }
        let big = match bytes[0] {
            b'l' => false,
            b'B' => true,
            _ => return Err(DecodeError::BadHeader("endianness")),
        };
        let word = |at: usize| {
            let w: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
            (if big {
                u32::from_be_bytes(w)
            } else {
                u32::from_le_bytes(w)
            }) as usize
        };
        let body = word(4);
        let fields = word(12);
        let total = (16 + fields).next_multiple_of(8) + body;
        if total > MAX_MESSAGE {
            return Err(DecodeError::TooLarge(total));
        }
        Ok((bytes.len() >= total).then_some(total))
    }

    /// One whole message, as [`Message::frame_len`] measured it, taking
    /// the fds its header declares from the front of `fds`.
    pub fn parse(frame: &[u8], fds: &mut VecDeque<OwnedFd>) -> Result<Message, DecodeError> {
        let kind = match frame.get(1) {
            Some(1) => MessageKind::MethodCall,
            Some(2) => MessageKind::MethodReturn,
            Some(3) => MessageKind::Error,
            Some(4) => MessageKind::Signal,
            _ => return Err(DecodeError::BadHeader("type")),
        };
        if frame.get(3) != Some(&1) {
            return Err(DecodeError::BadHeader("version"));
        }
        let big = frame[0] == b'B';
        let mut d = Decoder::new(frame, big, &[]);
        d.u8()?;
        d.u8()?;
        let flags = Flags(d.u8()?);
        d.u8()?;
        let body_len = d.u32()? as usize;
        let serial = d.u32()?;
        if serial == 0 {
            return Err(DecodeError::BadHeader("serial"));
        }
        let mut m = Message::empty(kind);
        m.flags = flags;
        m.serial = serial;
        m.big_endian = big;
        let mut unix_fds = 0u32;
        d.array(8, |d| {
            d.structure(|d| {
                let code = d.u8()?;
                let sig = d.signature()?;
                let want = |s: &str| {
                    if sig == s {
                        Ok(())
                    } else {
                        Err(DecodeError::BadHeader("header field type"))
                    }
                };
                match code {
                    1 => {
                        want("o")?;
                        m.path = Some(ObjectPath::new(d.str()?)?);
                    }
                    2 => {
                        want("s")?;
                        m.interface = Some(InterfaceName::new(d.str()?)?);
                    }
                    3 => {
                        want("s")?;
                        m.member = Some(MemberName::new(d.str()?)?);
                    }
                    4 => {
                        want("s")?;
                        m.error_name = Some(ErrorName::new(d.str()?)?);
                    }
                    5 => {
                        want("u")?;
                        m.reply_serial = Some(d.u32()?);
                    }
                    6 => {
                        want("s")?;
                        m.destination = Some(BusName::new(d.str()?)?);
                    }
                    7 => {
                        want("s")?;
                        m.sender = Some(BusName::new(d.str()?)?);
                    }
                    8 => {
                        want("g")?;
                        m.signature = Signature::from_valid(d.signature()?.to_owned());
                    }
                    9 => {
                        want("u")?;
                        unix_fds = d.u32()?;
                    }
                    _ => {
                        if !Signature::from_valid(sig.to_owned()).is_single() {
                            return Err(DecodeError::BadVariant(sig.to_owned()));
                        }
                        Value::decode_raw(sig, d)?;
                    }
                }
                Ok(())
            })
        })?;
        d.align(8)?;
        let start = d.pos();
        if frame.len() != start + body_len {
            return Err(DecodeError::BadHeader("length"));
        }
        let complete = match kind {
            MessageKind::MethodCall => m.path.is_some() && m.member.is_some(),
            MessageKind::Signal => m.path.is_some() && m.interface.is_some() && m.member.is_some(),
            MessageKind::Error => m.error_name.is_some() && m.reply_serial.is_some(),
            MessageKind::MethodReturn => m.reply_serial.is_some(),
        };
        if !complete {
            return Err(DecodeError::BadHeader("a required header field is missing"));
        }
        if fds.len() < unix_fds as usize {
            return Err(DecodeError::MissingFds {
                declared: unix_fds,
                available: fds.len(),
            });
        }
        m.fds = fds.drain(..unix_fds as usize).map(UnixFd::new).collect();
        m.body = frame[start..].to_vec();
        Ok(m)
    }
}

/// One header field: `(code, variant)`.
fn field(e: &mut Encoder, code: u8, sig: &str, value: impl FnOnce(&mut Encoder)) {
    e.structure(|e| {
        e.u8(code);
        e.signature(sig);
        value(e);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Message {
        Message::method_call("/org/freedesktop/DBus", "Hello")
            .with_destination("org.freedesktop.DBus")
            .with_interface("org.freedesktop.DBus")
    }

    #[test]
    fn hello_is_byte_exact() {
        let mut want = Vec::new();
        want.extend_from_slice(b"l\x01\x00\x01"); // LE, method call, no flags, v1
        want.extend_from_slice(&0u32.to_le_bytes()); // body length
        want.extend_from_slice(&1u32.to_le_bytes()); // serial
        want.extend_from_slice(&109u32.to_le_bytes()); // header fields length
        want.extend_from_slice(b"\x01\x01o\x00");
        want.extend_from_slice(&21u32.to_le_bytes());
        want.extend_from_slice(b"/org/freedesktop/DBus\x00");
        want.extend_from_slice(&[0; 2]); // 46 -> 48
        want.extend_from_slice(b"\x02\x01s\x00");
        want.extend_from_slice(&20u32.to_le_bytes());
        want.extend_from_slice(b"org.freedesktop.DBus\x00");
        want.extend_from_slice(&[0; 3]); // 77 -> 80
        want.extend_from_slice(b"\x03\x01s\x00");
        want.extend_from_slice(&5u32.to_le_bytes());
        want.extend_from_slice(b"Hello\x00");
        want.extend_from_slice(&[0; 2]); // 94 -> 96
        want.extend_from_slice(b"\x06\x01s\x00");
        want.extend_from_slice(&20u32.to_le_bytes());
        want.extend_from_slice(b"org.freedesktop.DBus\x00");
        want.extend_from_slice(&[0; 3]); // 125 -> 128: header padding
        assert_eq!(want.len(), 128);
        let (bytes, fds) = hello().encode(1);
        assert_eq!(bytes, want);
        assert!(fds.is_empty());
    }

    #[test]
    fn a_big_endian_signal_decodes() {
        let mut m = Vec::new();
        m.extend_from_slice(b"B\x04\x00\x01");
        m.extend_from_slice(&4u32.to_be_bytes());
        m.extend_from_slice(&9u32.to_be_bytes());
        m.extend_from_slice(&55u32.to_be_bytes());
        m.extend_from_slice(b"\x01\x01o\x00");
        m.extend_from_slice(&2u32.to_be_bytes());
        m.extend_from_slice(b"/a\x00");
        m.extend_from_slice(&[0; 5]); // 27 -> 32
        m.extend_from_slice(b"\x02\x01s\x00");
        m.extend_from_slice(&3u32.to_be_bytes());
        m.extend_from_slice(b"a.b\x00");
        m.extend_from_slice(&[0; 4]); // 44 -> 48
        m.extend_from_slice(b"\x03\x01s\x00");
        m.extend_from_slice(&1u32.to_be_bytes());
        m.extend_from_slice(b"C\x00");
        m.extend_from_slice(&[0; 6]); // 58 -> 64
        m.extend_from_slice(b"\x08\x01g\x00\x01u\x00"); // 64 -> 71
        m.push(0); // 71 -> 72: header padding
        m.extend_from_slice(&7u32.to_be_bytes());
        assert_eq!(m.len(), 76);
        assert_eq!(Message::frame_len(&m), Ok(Some(76)));
        let msg = Message::parse(&m, &mut VecDeque::new()).unwrap();
        assert_eq!(msg.kind(), MessageKind::Signal);
        assert_eq!(msg.serial(), 9);
        assert_eq!(msg.path().unwrap().as_str(), "/a");
        assert_eq!(msg.interface().unwrap().as_str(), "a.b");
        assert_eq!(msg.member().unwrap().as_str(), "C");
        assert!(msg.is_big_endian());
        assert_eq!(msg.body::<(u32,)>(), Ok((7,)));
    }

    #[test]
    #[should_panic(expected = "dbus: a received big-endian message")]
    fn a_received_big_endian_message_cannot_be_re_encoded() {
        let mut m = Vec::new();
        m.extend_from_slice(b"B\x04\x00\x01");
        m.extend_from_slice(&4u32.to_be_bytes());
        m.extend_from_slice(&9u32.to_be_bytes());
        m.extend_from_slice(&55u32.to_be_bytes());
        m.extend_from_slice(b"\x01\x01o\x00");
        m.extend_from_slice(&2u32.to_be_bytes());
        m.extend_from_slice(b"/a\x00");
        m.extend_from_slice(&[0; 5]);
        m.extend_from_slice(b"\x02\x01s\x00");
        m.extend_from_slice(&3u32.to_be_bytes());
        m.extend_from_slice(b"a.b\x00");
        m.extend_from_slice(&[0; 4]);
        m.extend_from_slice(b"\x03\x01s\x00");
        m.extend_from_slice(&1u32.to_be_bytes());
        m.extend_from_slice(b"C\x00");
        m.extend_from_slice(&[0; 6]);
        m.extend_from_slice(b"\x08\x01g\x00\x01u\x00");
        m.push(0);
        m.extend_from_slice(&7u32.to_be_bytes());
        let msg = Message::parse(&m, &mut VecDeque::new()).unwrap();
        let _ = msg.encode(1);
    }

    #[test]
    fn a_call_round_trips() {
        let m = Message::method_call("/x", "Do")
            .with_destination(":1.5")
            .with_sender(":1.9")
            .with_interface("a.b")
            .with_flags(Flags::NO_AUTO_START)
            .with_body(&("hi", 3u32));
        let (bytes, _) = m.encode(42);
        assert_eq!(Message::frame_len(&bytes), Ok(Some(bytes.len())));
        let back = Message::parse(&bytes, &mut VecDeque::new()).unwrap();
        assert_eq!(back.kind(), MessageKind::MethodCall);
        assert_eq!(back.serial(), 42);
        assert!(back.flags().contains(Flags::NO_AUTO_START));
        assert_eq!(back.destination().unwrap().as_str(), ":1.5");
        assert_eq!(back.sender().unwrap().as_str(), ":1.9");
        assert_eq!(back.signature(), "su");
        assert_eq!(back.body::<(String, u32)>(), Ok(("hi".to_string(), 3)));
        assert_eq!(
            back.body_values(),
            Ok(vec![Value::Str("hi".into()), Value::U32(3)])
        );
        assert_eq!(back.string_arg(0), Some("hi".to_string()));
        assert_eq!(back.string_arg(1), None);
    }

    #[test]
    fn errors_carry_their_text() {
        let m = Message::error(7, "org.freedesktop.DBus.Error.Failed", "boom");
        let (bytes, _) = m.encode(8);
        let back = Message::parse(&bytes, &mut VecDeque::new()).unwrap();
        assert_eq!(back.kind(), MessageKind::Error);
        assert_eq!(back.reply_serial(), Some(7));
        assert_eq!(
            back.error_name().unwrap().as_str(),
            "org.freedesktop.DBus.Error.Failed"
        );
        assert_eq!(back.error_text(), Some("boom".to_string()));
    }

    #[test]
    fn framing() {
        let (bytes, _) = hello().encode(1);
        assert_eq!(Message::frame_len(&bytes[..15]), Ok(None));
        assert_eq!(Message::frame_len(&bytes[..100]), Ok(None));
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert_eq!(
            Message::frame_len(&bad),
            Err(DecodeError::BadHeader("endianness"))
        );
        let mut huge = bytes.clone();
        huge[4..8].copy_from_slice(&(200u32 << 20).to_le_bytes());
        assert!(matches!(
            Message::frame_len(&huge),
            Err(DecodeError::TooLarge(_))
        ));
    }

    #[test]
    fn a_missing_required_field_is_an_error() {
        let mut m = Message::signal("/a", "a.b", "C");
        m.interface = None;
        let (bytes, _) = m.encode(1);
        assert_eq!(
            Message::parse(&bytes, &mut VecDeque::new()).map(|_| ()),
            Err(DecodeError::BadHeader("a required header field is missing"))
        );
    }

    #[test]
    fn declared_fds_must_arrive() {
        let (r, _w) = std::io::pipe().unwrap();
        let m = Message::method_return(1).with_body(&(UnixFd::new(r.into()),));
        let (bytes, fds) = m.encode(2);
        assert_eq!(fds.len(), 1);
        assert_eq!(
            Message::parse(&bytes, &mut VecDeque::new()).map(|_| ()),
            Err(DecodeError::MissingFds {
                declared: 1,
                available: 0
            })
        );
        let mut queue: VecDeque<OwnedFd> =
            fds.iter().map(|f| f.try_clone_owned().unwrap()).collect();
        let back = Message::parse(&bytes, &mut queue).unwrap();
        assert_eq!(back.fds().len(), 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn body_type_checks() {
        let m = Message::method_return(1).with_body(&(1u32,));
        assert!(matches!(
            m.body::<(String,)>(),
            Err(DecodeError::Mismatch { .. })
        ));
        let raw =
            Message::method_return(1).with_raw_body(Signature::from("as"), vec![100, 0, 0, 0]);
        assert_eq!(raw.body::<(Vec<String>,)>(), Err(DecodeError::Truncated));
    }
}
