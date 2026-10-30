//! The attribute table in `program.rs` mirrors `render::Command` byte for
//! byte. This pins the layout it assumes.

use std::mem::{offset_of, size_of};

use render::Command;

#[test]
fn a_command_is_140_bytes_at_the_offsets_the_attributes_use() {
    assert_eq!(size_of::<Command>(), 140);
    assert_eq!(offset_of!(Command, rect), 0);
    assert_eq!(offset_of!(Command, z), 16);
    assert_eq!(offset_of!(Command, color), 20);
    assert_eq!(offset_of!(Command, border_color), 36);
    assert_eq!(offset_of!(Command, background), 52);
    assert_eq!(offset_of!(Command, radii), 68);
    assert_eq!(offset_of!(Command, border), 84);
    assert_eq!(offset_of!(Command, tile), 100);
    assert_eq!(offset_of!(Command, flags), 120);
    assert_eq!(offset_of!(Command, clip), 124);
}
