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
