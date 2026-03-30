#![cfg(feature = "asn1")]

use std::sync::Arc;
use wirespec_backend_api::*;
use wirespec_driver::asn1_compile;
use wirespec_driver::pipeline::{Asn1ModuleInfo, Asn1ModuleMap};
use wirespec_sema::ComplianceProfile;

fn test_asn1_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/asn1/test.asn1")
}

fn build_asn1_map() -> Asn1ModuleMap {
    let result = asn1_compile::compile_asn1(&test_asn1_path()).unwrap();
    let mut map = Asn1ModuleMap::default();
    map.modules.insert(
        "test.asn1".to_string(),
        Asn1ModuleInfo {
            module_name: result.module_name,
            source: result.source,
        },
    );
    map
}

#[test]
fn pipeline_auto_resolves_rust_module_from_map() {
    let src = r#"
        extern asn1 "test.asn1" { SimpleMessage }
        packet P { len: u16, data: asn1(SimpleMessage, encoding: uper, length: len) }
    "#;

    let asn1_map = build_asn1_map();

    let codec = wirespec_driver::pipeline::compile_module(
        src,
        ComplianceProfile::default(),
        &Default::default(),
        &asn1_map,
    )
    .unwrap();

    // Verify the hint has the auto-resolved rust_module
    let data_field = &codec.packets[0].fields[1];
    let hint = data_field
        .asn1_hint
        .as_ref()
        .expect("should have asn1_hint");
    assert_eq!(hint.type_name, "SimpleMessage");
    assert_eq!(hint.rust_module.as_deref(), Some("crate::test_module"));
}

#[test]
fn pipeline_empty_map_leaves_rust_module_none() {
    let src = r#"
        extern asn1 "test.asn1" { SimpleMessage }
        packet P { len: u16, data: asn1(SimpleMessage, encoding: uper, length: len) }
    "#;

    let codec = wirespec_driver::pipeline::compile_module(
        src,
        ComplianceProfile::default(),
        &Default::default(),
        &Asn1ModuleMap::default(),
    )
    .unwrap();

    let data_field = &codec.packets[0].fields[1];
    let hint = data_field
        .asn1_hint
        .as_ref()
        .expect("should have asn1_hint");
    assert!(
        hint.rust_module.is_none(),
        "rust_module should be None when map is empty"
    );
}

#[test]
fn pipeline_manual_use_clause_preserved_over_map() {
    let src = r#"
        extern asn1 "test.asn1" use my_custom::path { SimpleMessage }
        packet P { len: u16, data: asn1(SimpleMessage, encoding: uper, length: len) }
    "#;

    let asn1_map = build_asn1_map();

    let codec = wirespec_driver::pipeline::compile_module(
        src,
        ComplianceProfile::default(),
        &Default::default(),
        &asn1_map,
    )
    .unwrap();

    let data_field = &codec.packets[0].fields[1];
    let hint = data_field
        .asn1_hint
        .as_ref()
        .expect("should have asn1_hint");
    // Manual use clause should win over map
    assert_eq!(hint.rust_module.as_deref(), Some("my_custom::path"));
}

#[test]
fn pipeline_auto_resolve_generates_correct_import_in_rust() {
    let src = r#"
        extern asn1 "test.asn1" { SimpleMessage }
        packet P { len: u16, data: asn1(SimpleMessage, encoding: uper, length: len) }
    "#;

    let asn1_map = build_asn1_map();

    let codec = wirespec_driver::pipeline::compile_module(
        src,
        ComplianceProfile::default(),
        &Default::default(),
        &asn1_map,
    )
    .unwrap();

    let backend = wirespec_backend_rust::RustBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(RustBackendOptions {}),
        checksum_bindings: Arc::new(wirespec_backend_rust::checksum_binding::RustChecksumBindings),
        is_entry_module: true,
    };
    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();

    assert!(
        lowered
            .source
            .contains("use crate::test_module::SimpleMessage;"),
        "should auto-generate import from map, got:\n{}",
        lowered.source
    );
}

#[test]
fn pipeline_multiple_extern_asn1_types() {
    let src = r#"
        extern asn1 "test.asn1" { SimpleMessage, AnotherType }
        packet P { len: u16, data: asn1(SimpleMessage, encoding: uper, length: len) }
    "#;

    let asn1_map = build_asn1_map();

    let codec = wirespec_driver::pipeline::compile_module(
        src,
        ComplianceProfile::default(),
        &Default::default(),
        &asn1_map,
    )
    .unwrap();

    // Verify it compiles OK with multiple declared types
    assert_eq!(codec.packets.len(), 1);
}
