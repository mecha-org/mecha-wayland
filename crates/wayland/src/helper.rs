use crate::{ObjectId, RawWaylandEvent};

const HEADER: usize = 8;

pub fn parse_messages(bytes: Vec<u8>) -> Vec<RawWaylandEvent> {
    let mut events = Vec::new();
    let mut offset = 0;
    while offset + HEADER <= bytes.len() {
        let sender_id = u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let word2 = u32::from_ne_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        let size = (word2 >> 16) as usize;
        let opcode = word2 & 0xffff;
        if size < HEADER || offset + size > bytes.len() {
            panic!("partial message");
        }
        let data = bytes[offset + HEADER..offset + size].to_vec();
        events.push(RawWaylandEvent {
            object_id: ObjectId(sender_id),
            opcode,
            data,
        });
        offset += size;
    }
    if offset < bytes.len() {
        panic!("partial message");
    }
    events
}

pub fn encode_string(buf: &mut Vec<u8>, s: &str) {
    let str_len = s.len() + 1; // includes null terminator
    let padded = (str_len + 3) & !3;
    buf.extend_from_slice(&(str_len as u32).to_ne_bytes());
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
    buf.extend(std::iter::repeat(0u8).take(padded - str_len));
}
