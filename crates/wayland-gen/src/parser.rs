use crate::ast::Protocol;
use std::path::Path;

pub fn parse_xml<P: AsRef<Path>>(path: P) -> Protocol {
    let path_ref = path.as_ref();

    let xml_content = std::fs::read_to_string(path_ref)
        .unwrap_or_else(|e| panic!("Failed to read XML file '{}': {}", path_ref.display(), e));

    quick_xml::de::from_str(&xml_content).unwrap_or_else(|e| {
        panic!(
            "Failed to parse Wayland XML '{}': {}",
            path_ref.display(),
            e
        )
    })
}
