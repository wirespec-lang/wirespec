use wirespec_sema::error::ErrorKind;
use wirespec_sema::{ComplianceProfile, analyze};
use wirespec_syntax::parse;

fn expect_error(src: &str, expected_kind: ErrorKind) {
    let ast = parse(src).unwrap();
    let result = analyze(&ast, ComplianceProfile::default(), &Default::default());
    match result {
        Err(e) => assert_eq!(e.kind, expected_kind, "wrong error kind: {}", e.msg),
        Ok(_) => panic!("expected error {:?}, but analysis succeeded", expected_kind),
    }
}

fn expect_ok(src: &str) {
    let ast = parse(src).unwrap();
    let result = analyze(&ast, ComplianceProfile::default(), &Default::default());
    assert!(
        result.is_ok(),
        "expected success, got error: {:?}",
        result.err()
    );
}

#[test]
fn ok_extern_asn1_with_asn1_field() {
    expect_ok(
        r#"
        extern asn1 "s.asn1" { Foo }
        packet P {
            length: u16,
            payload: asn1(Foo, encoding: uper, length: length),
        }
    "#,
    );
}

#[test]
fn error_asn1_type_not_declared() {
    expect_error(
        r#"
        packet P {
            length: u16,
            payload: asn1(Unknown, encoding: uper, length: length),
        }
        "#,
        ErrorKind::UndefinedAsn1Type,
    );
}

#[test]
fn error_unsupported_encoding() {
    expect_error(
        r#"
        extern asn1 "s.asn1" { Foo }
        packet P {
            length: u16,
            payload: asn1(Foo, encoding: ber, length: length),
        }
        "#,
        ErrorKind::UnsupportedAsn1Encoding,
    );
}

#[test]
fn ok_asn1_remaining() {
    expect_ok(
        r#"
        extern asn1 "s.asn1" { Bar }
        packet P {
            payload: asn1(Bar, encoding: uper, remaining),
        }
    "#,
    );
}

#[test]
fn ok_asn1_field_has_hint_in_sema() {
    let ast = parse(
        r#"
        extern asn1 "path/schema.asn1" { MyType }
        packet P {
            len: u16,
            data: asn1(MyType, encoding: uper, length: len),
        }
    "#,
    )
    .unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.packets.len(), 1);
    let data_field = &sem.packets[0].fields[1];
    assert_eq!(data_field.name, "data");
    let hint = data_field
        .asn1_hint
        .as_ref()
        .expect("should have asn1_hint");
    assert_eq!(hint.type_name, "MyType");
    assert_eq!(hint.encoding, "uper");
    assert_eq!(hint.extern_path, "path/schema.asn1");
}
