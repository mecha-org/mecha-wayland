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
    let phosh = fixed_phosh_private();
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
        "protocols/wlr-screencopy-unstable-v1.xml",
        phosh.to_str().expect("utf-8 OUT_DIR"),
    ]);
}

/// Upstream `phosh-private.xml` names `enum="flags"` on `startup_tracker`
/// args, but the enum is called `flag`. The vendored file stays unchanged
/// (opcodes depend on it); the generator reads a copy with that one
/// reference fixed.
fn fixed_phosh_private() -> std::path::PathBuf {
    let xml =
        std::fs::read_to_string("protocols/phosh-private.xml").expect("read phosh-private.xml");
    let fixed = xml.replace(r#"enum="flags""#, r#"enum="flag""#);
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("phosh-private.xml");
    std::fs::write(&out, fixed).expect("write fixed phosh-private.xml");
    out
}
