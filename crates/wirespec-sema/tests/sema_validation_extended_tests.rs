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
fn error_duplicate_enum_member_name() {
    expect_error(
        "enum E: u8 { A = 0, A = 1 }",
        ErrorKind::DuplicateDefinition,
    );
}

#[test]
fn error_duplicate_flags_member_name() {
    expect_error(
        "flags F: u8 { X = 0x01, X = 0x02 }",
        ErrorKind::DuplicateDefinition,
    );
}

#[test]
fn ok_distinct_enum_members() {
    expect_ok("enum E: u8 { A = 0, B = 1, C = 2 }");
}
