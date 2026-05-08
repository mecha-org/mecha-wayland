use std::io::{self, Read};

use tracing::trace;

#[allow(dead_code)]
pub fn write_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len_with_null = bytes.len() + 1;
    write_u32(buf, len_with_null as u32);
    buf.extend_from_slice(bytes);
    buf.push(0); // null terminator
    let pad = (4 - (len_with_null % 4)) % 4;
    for _ in 0..pad {
        buf.push(0);
    }
}

pub fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn send_msg(buf: &mut Vec<u8>, object_id: u32, opcode: u16, args: &[u8]) -> io::Result<()> {
    let total_size = (8 + args.len()) as u32;
    trace!(object_id, opcode, bytes = total_size, "→ send");
    let word2 = (total_size << 16) | (opcode as u32);
    buf.extend_from_slice(&object_id.to_le_bytes());
    buf.extend_from_slice(&word2.to_le_bytes());
    buf.extend_from_slice(args);
    Ok(())
}

pub fn recv_msg(stream: &mut impl Read) -> io::Result<(u32, u16, Vec<u8>)> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header)?;
    let object_id = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let word2 = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let total_size = (word2 >> 16) as usize;
    let opcode = (word2 & 0xffff) as u16;
    let body_len = total_size - 8;
    let mut body = vec![0u8; body_len];
    stream.read_exact(&mut body)?;
    trace!(object_id, opcode, bytes = total_size, "← recv");
    Ok((object_id, opcode, body))
}

pub fn read_u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

pub fn read_string(data: &[u8], off: usize) -> (String, usize) {
    let len = read_u32(data, off) as usize; // includes null terminator
    let s = String::from_utf8_lossy(&data[off + 4..off + 4 + len - 1]).to_string();
    let pad = (4 - (len % 4)) % 4;
    (s, off + 4 + len + pad)
}
