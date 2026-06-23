use crate::generator::GenerationType;

mod ast;
mod generator;
mod parser;

fn main() {
    println!("cargo:rerun-if-changed=protocols/wayland.xml");
    generator::generate("protocols/wayland.xml", GenerationType::Client);
}
