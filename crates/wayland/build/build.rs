mod ast;
mod generator;
mod parser;

fn main() {
    println!("cargo:rerun-if-changed=protocols/");
    generator::generate(&[
        "protocols/wayland.xml",
        "protocols/xdg-shell.xml",
        "protocols/wlr-layer-shell-unstable-v1.xml",
        "protocols/ext-session-lock-v1.xml",
        "protocols/linux-dmabuf-unstable-v1.xml",
        "protocols/virtual-keyboard-unstable-v1.xml",
        "protocols/input-method-unstable-v2.xml",
        "protocols/text-input-unstable-v3.xml",
    ]);
}
