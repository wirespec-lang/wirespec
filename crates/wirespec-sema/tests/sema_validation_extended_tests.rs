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

#[test]
fn error_enum_value_overflow_u8() {
    expect_error("enum E: u8 { X = 256 }", ErrorKind::TypeMismatch);
}

#[test]
fn error_enum_value_overflow_u16() {
    expect_error("enum E: u16 { X = 65536 }", ErrorKind::TypeMismatch);
}

#[test]
fn ok_enum_value_max_u8() {
    expect_ok("enum E: u8 { X = 255 }");
}

#[test]
fn ok_enum_value_max_u16() {
    expect_ok("enum E: u16 { X = 65535 }");
}

// ── bytes[length_or_remaining] edge cases ──

#[test]
fn ok_lor_option_u16() {
    expect_ok(
        "packet P { flags: u8, len: if flags & 1 { u16 }, data: bytes[length_or_remaining: len] }",
    );
}

// ── fill array position ──

#[test]
fn error_fill_then_wire_field() {
    expect_error(
        "packet P { items: [u8; fill], extra: u8 }",
        ErrorKind::RemainingNotLast,
    );
}

#[test]
fn ok_fill_then_derived() {
    // let and require can follow fill
    expect_ok("packet P { items: [u8; fill], let count: bool = true }");
}

// ── remaining in frame variant scope ──

#[test]
fn ok_remaining_in_frame_variant() {
    expect_ok(
        "frame F = match tag: u8 { 0 => A { data: bytes[remaining] }, _ => B { data: bytes[remaining] } }",
    );
}

// ── capsule within scope ──

#[test]
fn ok_capsule_basic() {
    expect_ok(
        r#"capsule C {
            type_id: u8,
            length: u16,
            payload: match type_id within length {
                0 => D { data: bytes[remaining] },
                _ => U { data: bytes[remaining] },
            },
        }"#,
    );
}

// ── state machine edge cases ──

#[test]
fn error_sm_duplicate_concrete_transitions() {
    // Two concrete transitions with the same (state, event) = duplicate
    expect_error(
        r#"state machine S {
            state A {}
            state B [terminal]
            initial A
            transition A -> B { on close }
            transition A -> B { on close }
        }"#,
        ErrorKind::SmDuplicateTransition,
    );
}

#[test]
fn ok_sm_wildcard_no_conflict() {
    expect_ok(
        r#"state machine S {
            state A {}
            state B {}
            state C [terminal]
            initial A
            transition A -> B { on go }
            transition * -> C { on close }
        }"#,
    );
}

#[test]
fn error_sm_event_named_src() {
    expect_error(
        r#"state machine S {
            state A {}
            state B [terminal]
            initial A
            transition A -> B { on src }
        }"#,
        ErrorKind::ReservedIdentifier,
    );
}

// ── Checksum in frame variant ──

#[test]
fn ok_checksum_in_frame_variant() {
    expect_ok(
        r#"frame F = match tag: u8 {
            0 => A { data: u32, @checksum(internet) cksum: u16 },
            _ => B { x: u8 },
        }"#,
    );
}

// ── Multiple remaining in same scope ──

#[test]
fn error_double_remaining() {
    expect_error(
        "packet P { a: bytes[remaining], b: bytes[remaining] }",
        ErrorKind::RemainingNotLast,
    );
}

// ── Cross-module type that's not registered ──

#[test]
fn error_undefined_type_in_array_element_nested() {
    expect_error(
        "packet P { count: u8, items: [NoSuchType; count] }",
        ErrorKind::UndefinedType,
    );
}

// ── InvalidEnumUnderlying tests ──

#[test]
fn test_enum_bool_underlying_rejected() {
    // bool is not an integer primitive, so it must be rejected as enum underlying type
    expect_error("enum Foo: bool { A = 0 }", ErrorKind::InvalidEnumUnderlying);
}

#[test]
fn test_enum_bytes_underlying_rejected() {
    // bytes is not a registered primitive type name, so it fails with UndefinedType
    expect_error("enum Foo: bytes { A = 0 }", ErrorKind::UndefinedType);
}

#[test]
fn test_flags_bool_underlying_rejected() {
    // bool is not an integer primitive, so it must be rejected as flags underlying type
    expect_error(
        "flags Foo: bool { A = 1 }",
        ErrorKind::InvalidEnumUnderlying,
    );
}

#[test]
fn test_enum_u24_underlying_accepted() {
    // u24 is a valid integer primitive and should be accepted as enum underlying type
    expect_ok("enum Foo: u24 { A = 0 }");
}
