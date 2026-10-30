// `ast.rs` is restored unchanged from the old crate and carries fields
// (`frozen`, `since`, `message_type`, ...) this generator does not read;
// `dead_code` here silences those, not the generator's own logic.
#![allow(dead_code)]

#[path = "build/ast.rs"]
mod ast;
#[path = "build/generator.rs"]
mod generator;
#[path = "build/parser.rs"]
mod parser;

fn main() {
    println!("cargo:rerun-if-changed=protocols/");
    println!("cargo:rerun-if-changed=build/");
    generator::generate(&[
        "protocols/wayland.xml",
        "protocols/xdg-shell.xml",
        "protocols/wlr-layer-shell-unstable-v1.xml",
        "protocols/linux-dmabuf-unstable-v1.xml",
        "protocols/ext-session-lock-v1.xml",
        "protocols/virtual-keyboard-unstable-v1.xml",
        "protocols/input-method-unstable-v2.xml",
        "protocols/text-input-unstable-v3.xml",
        "protocols/wlr-foreign-toplevel-management-unstable-v1.xml",
    ]);
}
