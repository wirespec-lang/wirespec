use wirespec_sema::ComplianceProfile;
use wirespec_syntax::parse;

/// Verify the full pipeline (parse → sema → layout → codec) works for ASN.1 fields.
#[test]
fn pipeline_asn1_field_with_length() {
    let src = r#"
        extern asn1 "supl/SUPL.asn1" { SuplPosInit }
        packet SuplMessage {
            version: u8,
            length: u16,
            payload: asn1(SuplPosInit, encoding: uper, length: length),
        }
    "#;
    let ast = parse(src).unwrap();
    let sem =
        wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();

    // Verify sema
    assert_eq!(sem.asn1_externs.len(), 1);
    assert_eq!(sem.asn1_externs[0].path, "supl/SUPL.asn1");
    let payload = &sem.packets[0].fields[2];
    assert!(payload.asn1_hint.is_some());

    // Verify layout
    let layout = wirespec_layout::lower(&sem).unwrap();
    let layout_payload = &layout.packets[0].fields[2];
    assert!(layout_payload.asn1_hint.is_some());

    // Verify codec
    let codec = wirespec_codec::lower(&layout).unwrap();
    let codec_payload = &codec.packets[0].fields[2];
    assert!(codec_payload.asn1_hint.is_some());
    assert_eq!(
        codec_payload.asn1_hint.as_ref().unwrap().type_name,
        "SuplPosInit"
    );
    assert_eq!(codec_payload.asn1_hint.as_ref().unwrap().encoding, "uper");
}

#[test]
fn pipeline_asn1_remaining() {
    let src = r#"
        extern asn1 "s.asn1" { MyType }
        packet P {
            header: u8,
            data: asn1(MyType, encoding: uper, remaining),
        }
    "#;
    let ast = parse(src).unwrap();
    let sem =
        wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let data = &codec.packets[0].fields[1];
    assert!(data.asn1_hint.is_some());
    assert_eq!(data.asn1_hint.as_ref().unwrap().type_name, "MyType");
}

/// Verify C backend treats ASN.1 fields as raw bytes (ignores hint).
#[test]
fn c_backend_ignores_asn1_hint() {
    use std::sync::Arc;
    use wirespec_backend_api::*;

    let src = r#"
        extern asn1 "s.asn1" { Foo }
        packet P { len: u16, data: asn1(Foo, encoding: uper, length: len) }
    "#;
    let ast = parse(src).unwrap();
    let sem =
        wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = wirespec_backend_c::CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions::default()),
        checksum_bindings: Arc::new(wirespec_backend_c::checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };
    let lowered = wirespec_backend_api::Backend::lower(&backend, &codec, &ctx).unwrap();

    // C backend should NOT mention rasn
    assert!(
        !lowered.source_content.contains("rasn"),
        "C backend should not reference rasn"
    );
    assert!(
        !lowered.header_content.contains("rasn"),
        "C header should not reference rasn"
    );
    // Should still have the data field
    assert!(
        lowered.header_content.contains("data"),
        "should have data field in C header"
    );
}

/// Verify Rust backend generates rasn code for ASN.1 fields (end-to-end).
#[test]
fn rust_backend_generates_rasn_code() {
    use std::sync::Arc;
    use wirespec_backend_api::*;

    let src = r#"
        extern asn1 "supl/SUPL.asn1" { SuplPosInit }
        packet SuplMessage {
            version: u8,
            length: u16,
            payload: asn1(SuplPosInit, encoding: uper, length: length),
        }
    "#;
    let ast = parse(src).unwrap();
    let sem =
        wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = wirespec_backend_rust::RustBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(RustBackendOptions::default()),
        checksum_bindings: Arc::new(wirespec_backend_rust::checksum_binding::RustChecksumBindings),
        is_entry_module: true,
    };
    let lowered = wirespec_backend_api::Backend::lower(&backend, &codec, &ctx).unwrap();
    let rs = &lowered.source;

    // Verify key patterns in generated Rust
    assert!(rs.contains("use rasn::uper;"), "should import rasn");
    assert!(
        rs.contains("pub payload: SuplPosInit"),
        "field should be decoded type"
    );
    assert!(rs.contains("uper::decode::<SuplPosInit>"), "should decode");
    assert!(rs.contains("uper::encode("), "should encode");
    assert!(!rs.contains("struct SuplMessage<'a>"), "no lifetime needed");
}
