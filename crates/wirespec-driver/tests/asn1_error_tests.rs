#![cfg(feature = "asn1")]

use wirespec_driver::asn1_compile;
use wirespec_driver::pipeline::Asn1ModuleMap;
use wirespec_sema::ComplianceProfile;

fn test_asn1_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/asn1/test.asn1")
}

#[test]
fn error_asn1_file_not_found() {
    let result = asn1_compile::compile_asn1(std::path::Path::new("/nonexistent/file.asn1"));
    match result {
        Err(msg) => assert!(
            msg.contains("not found"),
            "error should mention file not found: {}",
            msg
        ),
        Ok(_) => panic!("should have returned an error for nonexistent file"),
    }
}

#[test]
fn error_invalid_asn1_syntax() {
    // Create a temp file with invalid ASN.1 syntax
    let dir = std::env::temp_dir().join("wirespec-asn1-test");
    std::fs::create_dir_all(&dir).unwrap();
    let bad_file = dir.join("bad.asn1");
    std::fs::write(&bad_file, "THIS IS NOT VALID ASN.1").unwrap();

    let err = asn1_compile::compile_asn1(&bad_file);
    assert!(err.is_err(), "should fail on invalid ASN.1");

    std::fs::remove_file(&bad_file).ok();
}

#[test]
fn error_type_not_in_asn1_module() {
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    let err = asn1_compile::validate_types(
        &["NonExistent".to_string()],
        &result.type_names,
        "test.asn1",
    );
    assert!(err.is_err());
    let msg = err.unwrap_err();
    assert!(msg.contains("NonExistent"));
    assert!(msg.contains("test.asn1"));
}

#[test]
fn error_sema_rejects_undeclared_asn1_type() {
    // Type used in asn1() but not declared in any extern asn1 block
    let src = r#"
        packet P { len: u16, data: asn1(Unknown, encoding: uper, length: len) }
    "#;
    let result = wirespec_driver::pipeline::compile_module(
        src,
        ComplianceProfile::default(),
        &Default::default(),
        &Asn1ModuleMap::default(),
    );
    assert!(result.is_err(), "should fail when ASN.1 type not declared");
}

#[test]
fn error_unsupported_encoding_through_pipeline() {
    let src = r#"
        extern asn1 "test.asn1" { SimpleMessage }
        packet P { len: u16, data: asn1(SimpleMessage, encoding: xml, length: len) }
    "#;
    let result = wirespec_driver::pipeline::compile_module(
        src,
        ComplianceProfile::default(),
        &Default::default(),
        &Asn1ModuleMap::default(),
    );
    assert!(result.is_err(), "should fail with unsupported encoding");
}
