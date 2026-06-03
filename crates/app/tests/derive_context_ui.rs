#[test]
fn context_duplicate_field_type_is_rejected() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/context_duplicate_field_type.rs");
}
