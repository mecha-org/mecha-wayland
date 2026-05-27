pub mod ast;
pub mod generator;
pub mod parser;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::path::Path;

pub fn generate_protocol<P: AsRef<Path>>(xml_path: P, out_dir: P) {
    generate_protocols(&[xml_path], out_dir);
}

pub fn generate_protocols<P: AsRef<Path>>(xml_paths: &[P], out_dir: P) {
    let out_dir = out_dir.as_ref();

    std::fs::create_dir_all(out_dir).unwrap_or_else(|e| {
        panic!(
            "Failed to create output directory {}: {}",
            out_dir.display(),
            e
        )
    });

    // Flatten and parse all XML files into a single merged AST
    let merged_items: Vec<_> = xml_paths
        .iter()
        .flat_map(|path| parser::parse_xml(path).items)
        .collect();

    let merged_ast = ast::Protocol {
        name: "merged".to_string(),
        items: merged_items,
    };

    let mut protocols_tokens = TokenStream::new();

    // Generate and write individual interface files
    for interface in merged_ast.interfaces() {
        let file_name = format!("{}.rs", interface.name);
        let file_path = out_dir.join(&file_name);

        let tokens = generator::interface::generate(interface, &merged_ast);

        // Delegate formatting and file I/O to the helper
        write_formatted_tokens(&file_path, tokens, &interface.name);

        // Accumulate module declarations for the master file
        let iface_ident = format_ident!("{}", interface.name);
        let include_path = format!("/{}.rs", interface.name);

        protocols_tokens.extend(quote! {
            pub mod #iface_ident {
                include!(concat!(env!("OUT_DIR"), #include_path));
            }
        });
    }

    // Format and write the master protocols.rs file
    let protocols_path = out_dir.join("protocols.rs");
    write_formatted_tokens(&protocols_path, protocols_tokens, "protocols.rs");
}

/// Helper to parse TokenStream into a syntax tree, format it, and write to disk
fn write_formatted_tokens(path: &Path, tokens: TokenStream, name_for_error: &str) {
    let syntax_tree: syn::File = syn::parse2(tokens).unwrap_or_else(|e| {
        panic!(
            "Failed to parse generated Rust syntax tree for {}: {}",
            name_for_error, e
        )
    });

    let formatted_code = prettyplease::unparse(&syntax_tree);

    std::fs::write(path, formatted_code)
        .unwrap_or_else(|e| panic!("Failed to write generated file {}: {}", path.display(), e));
}
