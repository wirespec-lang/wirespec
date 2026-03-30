#![cfg(feature = "asn1")]

use std::path::PathBuf;
use wirespec_driver::asn1_compile;

fn test_asn1_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/asn1/test.asn1")
}

#[test]
fn compile_asn1_produces_rust_source() {
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    assert!(!result.source.is_empty(), "should produce Rust source");
    assert!(
        result.source.contains("pub struct SimpleMessage"),
        "should contain SimpleMessage"
    );
}

#[test]
fn compile_asn1_extracts_module_name() {
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    assert_eq!(result.module_name, "test_module");
}

#[test]
fn compile_asn1_extracts_type_names() {
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    assert!(result.type_names.contains(&"SimpleMessage".to_string()));
    assert!(result.type_names.contains(&"AnotherType".to_string()));
}

#[test]
fn validate_types_ok() {
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    assert!(
        asn1_compile::validate_types(
            &["SimpleMessage".to_string()],
            &result.type_names,
            "test.asn1"
        )
        .is_ok()
    );
}

#[test]
fn validate_types_missing() {
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    let err =
        asn1_compile::validate_types(&["NoSuchType".to_string()], &result.type_names, "test.asn1")
            .unwrap_err();
    assert!(
        err.contains("NoSuchType"),
        "error should mention missing type: {}",
        err
    );
    assert!(
        err.contains("SimpleMessage"),
        "error should list available types: {}",
        err
    );
}

#[test]
fn compile_asn1_file_not_found() {
    let result = asn1_compile::compile_asn1(&PathBuf::from("nonexistent.asn1"));
    assert!(result.is_err());
}

#[test]
fn compile_asn1_handles_multiple_types() {
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    // Should find both struct types
    assert!(
        result.type_names.len() >= 2,
        "should find at least 2 types, got: {:?}",
        result.type_names
    );
}

#[test]
fn extract_module_name_from_rasn_output() {
    // Verify the compile result has the right module name
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    assert!(
        !result.module_name.is_empty(),
        "module_name should not be empty"
    );
    // ASN.1 module "TestModule" becomes "test_module" in rasn output
    assert_eq!(result.module_name, "test_module");
}

#[test]
fn validate_types_multiple_declared() {
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    // Validate multiple types at once
    assert!(
        asn1_compile::validate_types(
            &["SimpleMessage".to_string(), "AnotherType".to_string()],
            &result.type_names,
            "test.asn1"
        )
        .is_ok()
    );
}

#[test]
fn validate_types_error_lists_available() {
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    let err =
        asn1_compile::validate_types(&["BadType".to_string()], &result.type_names, "test.asn1")
            .unwrap_err();
    // Error message should list available types
    assert!(err.contains("BadType"), "should mention the missing type");
    assert!(err.contains("test.asn1"), "should mention the ASN.1 file");
    for t in &result.type_names {
        assert!(
            err.contains(t),
            "should list available type '{}' in error: {}",
            t,
            err
        );
    }
}
