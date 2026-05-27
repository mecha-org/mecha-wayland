use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    println!("cargo:rerun-if-changed=protocols");

    let out_dir = env::var_os("OUT_DIR").expect("OUT_DIR is not set by Cargo");
    let out_path = PathBuf::from(out_dir);

    let mut xml_paths = Vec::new();

    if let Ok(entries) = fs::read_dir("protocols") {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_file() && path.extension().is_some_and(|ext| ext == "xml") {
                println!("cargo:rerun-if-changed={}", path.display());
                xml_paths.push(path);
            }
        }
    } else {
        panic!("Failed to read 'protocols' directory. Ensure it exists in your crate root.");
    }

    xml_paths.sort();

    wayland_gen::generate_protocols(&xml_paths, out_path);
}
