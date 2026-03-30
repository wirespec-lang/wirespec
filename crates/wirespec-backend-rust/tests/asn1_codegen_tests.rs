// crates/wirespec-backend-rust/tests/asn1_codegen_tests.rs
//
// Tests for ASN.1 field codegen in the Rust backend.

use wirespec_backend_api::*;
use wirespec_backend_rust::RustBackend;
use wirespec_backend_rust::checksum_binding::RustChecksumBindings;

fn generate_rust(src: &str) -> String {
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem = wirespec_sema::analyze(
        &ast,
        wirespec_sema::ComplianceProfile::default(),
        &Default::default(),
    )
    .unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    let backend = RustBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(RustBackendOptions::default()),
        checksum_bindings: std::sync::Arc::new(RustChecksumBindings),
        is_entry_module: true,
    };
    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();
    lowered.source
}

#[test]
fn asn1_field_generates_rasn_import() {
    let rs = generate_rust(
        r#"
        extern asn1 "s.asn1" { Foo }
        packet P { len: u16, data: asn1(Foo, encoding: uper, length: len) }
    "#,
    );
    assert!(
        rs.contains("use rasn::uper;"),
        "should import rasn::uper, got:\n{}",
        rs
    );
}

#[test]
fn asn1_field_struct_uses_decoded_type() {
    let rs = generate_rust(
        r#"
        extern asn1 "s.asn1" { Foo }
        packet P { len: u16, data: asn1(Foo, encoding: uper, length: len) }
    "#,
    );
    assert!(
        rs.contains("pub data: Foo"),
        "struct field should be decoded type Foo, got:\n{}",
        rs
    );
}

#[test]
fn asn1_field_parse_calls_rasn_decode() {
    let rs = generate_rust(
        r#"
        extern asn1 "s.asn1" { Foo }
        packet P { len: u16, data: asn1(Foo, encoding: uper, length: len) }
    "#,
    );
    assert!(
        rs.contains("uper::decode::<Foo>"),
        "parse should call rasn decode, got:\n{}",
        rs
    );
    assert!(
        rs.contains("Error::Asn1Decode"),
        "parse should map rasn error, got:\n{}",
        rs
    );
}

#[test]
fn asn1_field_serialize_calls_rasn_encode() {
    let rs = generate_rust(
        r#"
        extern asn1 "s.asn1" { Foo }
        packet P { len: u16, data: asn1(Foo, encoding: uper, length: len) }
    "#,
    );
    assert!(
        rs.contains("uper::encode("),
        "serialize should call rasn encode, got:\n{}",
        rs
    );
    assert!(
        rs.contains("Error::Asn1Encode"),
        "serialize should map rasn error, got:\n{}",
        rs
    );
}

#[test]
fn asn1_field_no_lifetime() {
    let rs = generate_rust(
        r#"
        extern asn1 "s.asn1" { Foo }
        packet P { len: u16, data: asn1(Foo, encoding: uper, length: len) }
    "#,
    );
    // Packet with u16 + ASN.1 field should NOT need lifetime
    assert!(
        !rs.contains("struct P<'a>"),
        "ASN.1 fields are owned, no lifetime, got:\n{}",
        rs
    );
}

#[test]
fn asn1_field_generates_type_import() {
    let rs = generate_rust(
        r#"
        extern asn1 "s.asn1" use asn1_types { Foo }
        packet P { len: u16, data: asn1(Foo, encoding: uper, length: len) }
    "#,
    );
    assert!(
        rs.contains("use asn1_types::Foo;"),
        "should import ASN.1 type, got:\n{}",
        rs
    );
}

#[test]
fn asn1_field_no_import_without_use() {
    let rs = generate_rust(
        r#"
        extern asn1 "s.asn1" { Foo }
        packet P { len: u16, data: asn1(Foo, encoding: uper, length: len) }
    "#,
    );
    assert!(
        !rs.contains("use asn1_types::"),
        "should not import without use clause, got:\n{}",
        rs
    );
}

#[test]
fn asn1_field_generates_grouped_import() {
    let rs = generate_rust(
        r#"
        extern asn1 "s.asn1" use asn1_types { Foo, Bar }
        packet P {
            len1: u16,
            data1: asn1(Foo, encoding: uper, length: len1),
            len2: u16,
            data2: asn1(Bar, encoding: uper, length: len2),
        }
    "#,
    );
    assert!(
        rs.contains("use asn1_types::{Bar, Foo};"),
        "should group imports, got:\n{}",
        rs
    );
}

#[test]
fn asn1_serialize_recomputes_length_field() {
    let rs = generate_rust(
        r#"
        extern asn1 "s.asn1" { Foo }
        packet P { len: u16, data: asn1(Foo, encoding: uper, length: len) }
    "#,
    );
    // serialize must NOT write self.len directly — it should write
    // the encoded payload length instead
    assert!(
        !rs.contains("w.write_u16be(self.len)"),
        "serialize should NOT write self.len directly; must recompute from encoded payload.\nGot:\n{}",
        rs
    );
    // The encoded payload should be computed BEFORE the length is written
    let serialize_section = rs.split("fn serialize").nth(1).unwrap_or("");
    let encode_pos = serialize_section
        .find("uper::encode(")
        .expect("should call uper::encode");
    let write_len_pos = serialize_section
        .find("_encoded.len()")
        .expect("should write encoded length");
    assert!(
        encode_pos < write_len_pos,
        "payload must be encoded before length is written.\nGot:\n{}",
        rs
    );
}

#[test]
fn asn1_remaining_parse() {
    let rs = generate_rust(
        r#"
        extern asn1 "s.asn1" { Bar }
        packet P { data: asn1(Bar, encoding: uper, remaining) }
    "#,
    );
    assert!(
        rs.contains("read_remaining()"),
        "remaining should use read_remaining, got:\n{}",
        rs
    );
    assert!(
        rs.contains("uper::decode::<Bar>"),
        "should decode Bar, got:\n{}",
        rs
    );
}
