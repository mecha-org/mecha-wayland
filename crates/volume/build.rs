use anyhow::Result;
use std::{fs::File, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    if let Err(e) = assets::builder::pack_atlas(
        &PathBuf::from(&manifest_dir).join("atlas.toml"),
        &PathBuf::from(&out_dir),
    ) {
        println!("cargo:warning=atlas packing failed: {e:?}");
    }
}
