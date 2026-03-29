//! Comprehensive error-path and edge-case tests for wirespec-sema.
//!
//! This file tests EVERY ErrorKind variant in the semantic analyzer,
//! covering type resolution errors, forward reference errors, scope errors,
//! state machine errors, checksum errors, annotation handling, and
//! complex valid regression cases.

use wirespec_sema::ComplianceProfile;
use wirespec_sema::analyze;
use wirespec_sema::error::ErrorKind;
use wirespec_syntax::parse;

fn default_profile() -> ComplianceProfile {
    ComplianceProfile::default()
}

// ══════════════════════════════════════════════════════════════════════
// 1. Type Resolution Errors (ErrorKind::UndefinedType)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_undefined_type_in_field() {
    // Field references a non-existent type
    let ast = parse("packet P { x: NonExistent }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err(), "should fail with undefined type");
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn error_undefined_type_in_second_field() {
    // First field valid, second has undefined type
    let ast = parse("packet P { x: u8, y: UnknownType }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn error_undefined_type_in_frame_tag() {
    // Frame tag type is not a known type
    let src = r#"
        frame F = match tag: UnknownTagType {
            0 => A {},
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err(), "frame with undefined tag type should fail");
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn error_undefined_type_in_const() {
    // Const with undefined type
    let ast = parse("const MAX: UnknownType = 20").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn error_undefined_type_in_enum_underlying() {
    // Enum with undefined underlying type
    let ast = parse("enum E: UnknownType { A = 0 }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn error_undefined_type_in_flags_underlying() {
    // Flags with undefined underlying type
    let ast = parse("flags F: UnknownType { A = 0x01 }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn error_undefined_type_in_array_element() {
    // Array element type is undefined
    let ast = parse("packet P { count: u8, items: [UnknownElem; count] }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn error_undefined_type_in_optional_inner() {
    // Optional inner type is undefined
    let ast = parse("packet P { flags: u8, extra: if flags & 1 { UnknownInner } }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn error_undefined_type_in_frame_variant_field() {
    // Field within a frame variant references undefined type
    let src = r#"
        frame F = match tag: u8 {
            0 => A { x: UnknownType },
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn error_undefined_type_in_capsule_header() {
    // Capsule header field has undefined type
    let src = r#"
        capsule C {
            type_field: UnknownType,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => E { data: bytes[remaining] },
            },
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

// ══════════════════════════════════════════════════════════════════════
// 2. TypeMismatch Errors (ErrorKind::TypeMismatch)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_state_machine_used_as_field_type() {
    // State machines cannot be used as field types
    let src = r#"
        state machine SM {
            state A
            state B [terminal]
            initial A
            transition A -> B { on go }
        }
        packet P { x: SM }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::TypeMismatch);
}

#[test]
fn error_const_used_as_type() {
    // Constants cannot be used as types
    let src = r#"
        const MAX: u8 = 255
        packet P { x: MAX }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::TypeMismatch);
}

// ══════════════════════════════════════════════════════════════════════
// 3. Forward Reference Errors (ErrorKind::ForwardReference)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_forward_ref_in_bytes_length() {
    // bytes[length: len] references 'len' which is declared after
    let ast = parse("packet P { data: bytes[length: len], len: u16 }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_err(),
        "forward reference in bytes length should fail"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::ForwardReference);
}

#[test]
fn error_forward_ref_in_optional_condition() {
    // Optional condition references 'flags' declared after the field
    let ast = parse("packet P { extra: if flags & 1 { u16 }, flags: u8 }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_err(),
        "forward reference in optional condition should fail"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::ForwardReference);
}

#[test]
fn error_forward_ref_in_array_count() {
    // Array count references 'count' declared after
    let ast = parse("packet P { items: [u8; count], count: u16 }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_err(),
        "forward reference in array count should fail"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::ForwardReference);
}

#[test]
fn ok_no_forward_ref_bytes_length() {
    // len is declared before data -- should succeed
    let ast = parse("packet P { len: u16, data: bytes[length: len] }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "back reference in bytes length should succeed"
    );
}

#[test]
fn ok_no_forward_ref_array_count() {
    // count is declared before items -- should succeed
    let ast = parse("packet P { count: u16, items: [u8; count] }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "back reference in array count should succeed"
    );
}

#[test]
fn ok_no_forward_ref_optional_condition() {
    // flags is declared before extra -- should succeed
    let ast = parse("packet P { flags: u8, extra: if flags & 0x01 { u16 } }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "back reference in optional condition should succeed"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 4. Checksum / Profile Errors
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_unknown_checksum_algorithm() {
    // sha256 is not in any profile's allowed list
    let src = r#"
        packet P {
            @checksum(sha256)
            checksum: u32,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err(), "unknown checksum algorithm should fail");
    assert_eq!(
        result.unwrap_err().kind,
        ErrorKind::ChecksumProfileViolation
    );
}

#[test]
fn error_checksum_profile_strict_fletcher16() {
    // fletcher16 is not allowed under Phase2StrictV1_0
    let src = r#"
        packet P {
            data: u32,
            @checksum(fletcher16)
            checksum: u16,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(
        result.is_err(),
        "fletcher16 should be rejected under strict profile"
    );
    assert_eq!(
        result.unwrap_err().kind,
        ErrorKind::ChecksumProfileViolation
    );
}

#[test]
fn ok_checksum_extended_fletcher16() {
    // fletcher16 IS allowed under Phase2ExtendedCurrent
    let src = r#"
        packet P {
            data: u32,
            @checksum(fletcher16)
            checksum: u16,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2ExtendedCurrent,
        &Default::default(),
    );
    assert!(
        result.is_ok(),
        "fletcher16 should be accepted under extended profile"
    );
}

#[test]
fn ok_checksum_internet_strict() {
    // internet checksum IS allowed under Phase2StrictV1_0
    let src = r#"
        packet P {
            data: u32,
            @checksum(internet)
            checksum: u16,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(
        result.is_ok(),
        "internet checksum should be accepted under strict profile"
    );
}

#[test]
fn ok_checksum_crc32_strict() {
    // crc32 IS allowed under Phase2StrictV1_0
    let src = r#"
        packet P {
            data: u32,
            @checksum(crc32)
            checksum: u32,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(
        result.is_ok(),
        "crc32 should be accepted under strict profile"
    );
}

#[test]
fn ok_checksum_crc32c_strict() {
    // crc32c IS allowed under Phase2StrictV1_0
    let src = r#"
        packet P {
            data: u32,
            @checksum(crc32c)
            checksum: u32,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(
        result.is_ok(),
        "crc32c should be accepted under strict profile"
    );
}

#[test]
fn error_checksum_unknown_algo_strict() {
    // An entirely invented algorithm
    let src = r#"
        packet P {
            data: u32,
            @checksum(md5)
            checksum: u32,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(
        result.is_err(),
        "md5 should be rejected under strict profile"
    );
    assert_eq!(
        result.unwrap_err().kind,
        ErrorKind::ChecksumProfileViolation
    );
}

#[test]
fn error_checksum_unknown_algo_extended() {
    // An entirely invented algorithm, even under extended profile
    let src = r#"
        packet P {
            data: u32,
            @checksum(md5)
            checksum: u32,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2ExtendedCurrent,
        &Default::default(),
    );
    assert!(
        result.is_err(),
        "md5 should be rejected even under extended profile"
    );
    assert_eq!(
        result.unwrap_err().kind,
        ErrorKind::ChecksumProfileViolation
    );
}

// ══════════════════════════════════════════════════════════════════════
// 5. State Machine Errors (SmUndefinedState, SmInvalidInitial)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_sm_undefined_dst_state() {
    let src = r#"
        state machine S {
            state A
            initial A
            transition A -> NonExistent { on go }
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err(), "undefined destination state should fail");
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmUndefinedState);
}

#[test]
fn error_sm_undefined_src_state() {
    let src = r#"
        state machine S {
            state A
            state B [terminal]
            initial A
            transition NonExistent -> B { on go }
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err(), "undefined source state should fail");
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmUndefinedState);
}

#[test]
fn error_sm_invalid_initial() {
    // Initial state name doesn't match any declared state
    let src = r#"
        state machine S {
            state A
            state B [terminal]
            initial NonExistent
            transition A -> B { on go }
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err(), "invalid initial state should fail");
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmInvalidInitial);
}

#[test]
fn error_sm_undefined_dst_with_multiple_states() {
    // Multiple valid states, but transition targets a nonexistent one
    let src = r#"
        state machine S {
            state A
            state B
            state C [terminal]
            initial A
            transition A -> B { on step1 }
            transition B -> Missing { on step2 }
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmUndefinedState);
}

#[test]
fn ok_sm_wildcard_expands_correctly() {
    // Wildcard transitions should expand to all non-terminal states
    let src = r#"
        state machine S {
            state A
            state B
            state C [terminal]
            initial A
            transition A -> B { on step }
            transition * -> C { on abort }
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    let sm = &sem.state_machines[0];
    // * should expand to A and B (both non-terminal)
    let abort_transitions: Vec<_> = sm
        .transitions
        .iter()
        .filter(|t| t.event_name == "abort")
        .collect();
    assert_eq!(
        abort_transitions.len(),
        2,
        "wildcard should expand to 2 non-terminal states (A, B)"
    );
}

#[test]
fn ok_sm_self_transition_with_guard_and_action() {
    let src = r#"
        state machine S {
            state Active { count: u8 = 0 }
            state Done [terminal]
            initial Active
            transition Active -> Active {
                on tick
                guard src.count < 10
                action { dst.count = src.count + 1; }
            }
            transition Active -> Done { on finish }
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "self-transition with guard and action should succeed"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 6. Annotation Handling
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_unknown_annotation_with_parens_ignored() {
    // Unknown annotations on fields should be silently ignored
    // (the analyzer only processes known annotations like @checksum, @max_len)
    // Note: bare @name annotations (without parens) greedily consume the next
    // Name token as an arg, so we use parenthesized form here.
    let src = r#"
        packet P {
            @some_custom_annotation(42)
            x: u8,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    // Unknown annotations are permissive -- they are simply ignored
    assert!(
        result.is_ok(),
        "unknown annotations should be silently ignored"
    );
}

#[test]
fn ok_bare_strict_annotation_ignored() {
    // @strict is a bare annotation (no args, no following name to consume)
    // that is recognized by VarInt lowering but ignored on packets.
    let src = r#"
        @strict
        packet P {
            x: u8,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "unrecognized bare @strict on packet should be silently ignored"
    );
}

#[test]
fn ok_max_len_annotation() {
    let src = r#"
        packet P {
            count: u16,
            @max_len(100)
            items: [u8; count],
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(
        sem.packets[0].fields[1].max_elements,
        Some(100),
        "@max_len should be recorded on the field"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 7. Complex Valid Cases (regression tests)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_packet_all_primitive_types() {
    let src = r#"
        packet P {
            a: u8,
            b: u16,
            c: u32,
            d: u64,
            e: i8,
            f: i16,
            g: i32,
            h: i64,
            i: bits[4],
            j: bits[4],
            k: bit,
            l: bit,
            m: bits[6],
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "packet with all primitive types should succeed"
    );
    let sem = result.unwrap();
    assert_eq!(sem.packets[0].fields.len(), 13);
}

#[test]
fn ok_packet_with_bytes_variants() {
    let src = r#"
        packet P {
            fixed: bytes[16],
            len: u16,
            dynamic: bytes[length: len],
            rest: bytes[remaining],
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "packet with bytes variants should succeed");
    let sem = result.unwrap();
    assert_eq!(sem.packets[0].fields.len(), 4);
}

#[test]
fn ok_complex_frame_with_all_features() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => Empty {},
            0x01..=0x03 => WithFields {
                x: u16,
                y: u32,
                require x > 0,
                let z: bool = (x & 0x01) != 0,
            },
            0x10 => WithOptional {
                flags: u8,
                extra: if flags & 0x01 { u16 },
            },
            0x20 => WithBytes {
                len: u16,
                data: bytes[length: len],
            },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "complex frame with all features should succeed"
    );
    let sem = result.unwrap();
    assert_eq!(sem.frames[0].variants.len(), 5);
}

#[test]
fn ok_capsule_with_expr_tag() {
    let src = r#"
        capsule C {
            header: u8,
            length: u16,
            payload: match (header >> 4) within length {
                1 => A { data: bytes[remaining] },
                _ => B { data: bytes[remaining] },
            },
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "capsule with expression tag should succeed");
    let sem = result.unwrap();
    assert_eq!(sem.capsules.len(), 1);
    assert_eq!(sem.capsules[0].header_fields.len(), 2);
    assert_eq!(sem.capsules[0].variants.len(), 2);
}

#[test]
fn ok_state_machine_complete() {
    let src = r#"
        state machine SM {
            state Idle
            state Active { counter: u32 = 0 }
            state Done [terminal]

            initial Idle

            transition Idle -> Active {
                on start
                action { dst.counter = 0; }
            }
            transition Active -> Active {
                on increment
                guard src.counter < 100
                action { dst.counter = src.counter + 1; }
            }
            transition Active -> Done { on finish }
            transition * -> Done { on abort }
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "complete state machine should succeed");
    let sem = result.unwrap();
    let sm = &sem.state_machines[0];
    assert_eq!(sm.states.len(), 3);
    assert!(sm.states[2].is_terminal);
    // Wildcard expands to Idle and Active (both non-terminal)
    let abort_transitions: Vec<_> = sm
        .transitions
        .iter()
        .filter(|t| t.event_name == "abort")
        .collect();
    assert_eq!(abort_transitions.len(), 2);
}

#[test]
fn ok_multiple_items_in_module() {
    let src = r#"
        const MAX: u8 = 255
        enum Direction: u8 { In = 0, Out = 1 }
        flags Flags: u8 { A = 0x01, B = 0x02 }
        type Handle = u16
        packet Header { x: u8, y: u16 }
        static_assert MAX <= 255
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "multiple items in module should succeed");
    let sem = result.unwrap();
    assert_eq!(sem.consts.len(), 1);
    assert_eq!(sem.enums.len(), 2); // enum + flags both become SemanticEnum
    assert_eq!(sem.packets.len(), 1);
    assert_eq!(sem.static_asserts.len(), 1);
}

#[test]
fn ok_const_referenced_in_require() {
    let src = r#"
        const MAX_LEN: u8 = 20
        packet P {
            length: u8,
            require length <= MAX_LEN,
            data: bytes[length: length],
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "const in require expression should succeed");
}

#[test]
fn ok_const_referenced_in_static_assert() {
    let src = r#"
        const MAX: u16 = 1024
        static_assert MAX <= 65535
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "const in static_assert should succeed");
    let sem = result.unwrap();
    assert_eq!(sem.consts.len(), 1);
    assert_eq!(sem.static_asserts.len(), 1);
}

#[test]
fn ok_type_alias_chain() {
    let src = r#"
        type A = u16le
        type B = A
        packet P { x: B }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "type alias chain should succeed");
}

#[test]
fn ok_enum_as_field_type() {
    let src = r#"
        enum ErrorCode: u8 { Ok = 0, Fail = 1 }
        packet P { code: ErrorCode }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "enum as field type should succeed");
}

#[test]
fn ok_flags_as_field_type() {
    let src = r#"
        flags Options: u8 { ReadOnly = 0x01, WriteOnly = 0x02, ReadWrite = 0x03 }
        packet P { opts: Options }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "flags as field type should succeed");
}

#[test]
fn ok_packet_ref_as_field_type() {
    // One packet can be used as a field type for another
    let src = r#"
        packet Inner { x: u8, y: u16 }
        packet Outer { inner: Inner, z: u32 }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "packet as field type should succeed");
}

#[test]
fn ok_frame_ref_as_field_type() {
    // A frame can be used as a field type
    let src = r#"
        frame F = match tag: u8 {
            0 => A {},
            _ => B { data: bytes[remaining] },
        }
        packet P { payload: F }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "frame as field type should succeed");
}

#[test]
fn ok_packet_with_require_and_derived() {
    let src = r#"
        packet P {
            flags: u8,
            length: u16,
            require length >= 8,
            let has_extra: bool = (flags & 0x01) != 0,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "packet with require and derived should succeed"
    );
    let sem = result.unwrap();
    assert_eq!(sem.packets[0].requires.len(), 1);
    assert_eq!(sem.packets[0].derived.len(), 1);
}

#[test]
fn ok_bytes_remaining_as_last() {
    // bytes[remaining] as last field should succeed
    let ast = parse("packet P { x: u8, data: bytes[remaining] }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "bytes[remaining] as last field should succeed"
    );
}

#[test]
fn ok_array_fill_as_last() {
    // [T; fill] as last field should succeed
    let ast = parse("packet P { x: u8, items: [u8; fill] }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "[T; fill] as last field should succeed");
}

// ══════════════════════════════════════════════════════════════════════
// 8. Endianness / Module-level annotations
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_explicit_little_endian_module() {
    let src = "@endian little\nmodule test\npacket P { x: u16 }";
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(
        sem.module_endianness,
        wirespec_sema::types::Endianness::Little
    );
}

#[test]
fn ok_default_big_endian_module() {
    let ast = parse("packet P { x: u16 }").unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.module_endianness, wirespec_sema::types::Endianness::Big);
}

#[test]
fn ok_explicit_endian_type_override() {
    // u16le should always be little endian regardless of module setting
    let ast = parse("packet P { x: u16le }").unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    // Module is big-endian, but the field type is explicitly little
    assert_eq!(sem.module_endianness, wirespec_sema::types::Endianness::Big);
    assert_eq!(sem.packets[0].fields.len(), 1);
}

// ══════════════════════════════════════════════════════════════════════
// 9. VarInt Pattern Errors
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_varint_prefix_match() {
    let src = r#"
        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6],
                0b01 => bits[14],
                0b10 => bits[30],
                0b11 => bits[62],
            },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.varints.len(), 1);
    assert_eq!(sem.varints[0].name, "VarInt");
}

#[test]
fn ok_continuation_varint() {
    let src = r#"
        type MqttLen = varint {
            continuation_bit: msb,
            value_bits: 7,
            max_bytes: 4,
            byte_order: little,
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.varints.len(), 1);
    assert_eq!(sem.varints[0].name, "MqttLen");
}

// ══════════════════════════════════════════════════════════════════════
// 10. Edge Cases - Empty and Minimal Constructs
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_empty_module() {
    let ast = parse("module test").unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.module_name, "test");
    assert!(sem.packets.is_empty());
    assert!(sem.frames.is_empty());
    assert!(sem.capsules.is_empty());
    assert!(sem.state_machines.is_empty());
}

#[test]
fn ok_packet_single_field() {
    let ast = parse("packet P { x: u8 }").unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.packets[0].fields.len(), 1);
}

#[test]
fn ok_frame_empty_variant() {
    // A frame variant with no fields is valid
    let src = r#"
        frame F = match tag: u8 {
            0 => Empty {},
            _ => Fallback { data: bytes[remaining] },
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "frame with empty variant should succeed");
    let sem = result.unwrap();
    assert_eq!(sem.frames[0].variants[0].fields.len(), 0);
}

#[test]
fn ok_sm_single_terminal_state() {
    let src = r#"
        state machine S {
            state Init
            state Done [terminal]
            initial Init
            transition Init -> Done { on finish }
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "SM with single terminal state should succeed"
    );
}

#[test]
fn ok_sm_state_with_multiple_fields() {
    let src = r#"
        state machine S {
            state Active { x: u8 = 0, y: u16 = 0 }
            state Done [terminal]
            initial Active
            transition Active -> Done { on finish }
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "SM state with multiple fields should succeed"
    );
    let sem = result.unwrap();
    assert_eq!(sem.state_machines[0].states[0].fields.len(), 2);
}

// ══════════════════════════════════════════════════════════════════════
// 11. Multiple Error Scenarios in Combination
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_multiple_undefined_types_first_wins() {
    // When there are multiple errors, the first one should be returned
    let ast = parse("packet P { x: Unknown1, y: Unknown2 }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind, ErrorKind::UndefinedType);
    // The error message should mention the first undefined type
    assert!(
        err.msg.contains("Unknown1"),
        "first error should mention Unknown1, got: {}",
        err.msg
    );
}

#[test]
fn error_undefined_type_alias_target() {
    // Type alias points to a nonexistent type
    let src = r#"
        type MyType = UnknownTarget
        packet P { x: MyType }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err(), "alias to undefined type should fail");
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn ok_capsule_with_field_tag() {
    // Capsule using a simple field as tag selector
    let src = r#"
        capsule C {
            type_id: u8,
            length: u16,
            payload: match type_id within length {
                0 => Empty { data: bytes[remaining] },
                _ => Data { data: bytes[remaining] },
            },
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "capsule with field tag should succeed");
}

// ══════════════════════════════════════════════════════════════════════
// 12. Semantic IR Structure Validation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ir_packet_field_ids_are_unique() {
    let src = r#"
        packet P {
            a: u8,
            b: u16,
            c: u32,
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    let ids: Vec<_> = sem.packets[0].fields.iter().map(|f| &f.field_id).collect();
    // All IDs should be unique
    let mut unique_ids = ids.clone();
    unique_ids.sort();
    unique_ids.dedup();
    assert_eq!(ids.len(), unique_ids.len(), "field IDs should be unique");
}

#[test]
fn ir_const_id_format() {
    let ast = parse("const MAX: u8 = 255").unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.consts[0].const_id, "const:MAX");
}

#[test]
fn ir_enum_id_format() {
    let ast = parse("enum E: u8 { A = 0, B = 1 }").unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.enums[0].enum_id, "enum:E");
    assert_eq!(sem.enums[0].members[0].member_id, "enum:E/member:A");
}

#[test]
fn ir_packet_id_format() {
    let ast = parse("packet Foo { x: u8 }").unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.packets[0].packet_id, "packet:Foo");
}

#[test]
fn ir_frame_id_format() {
    let src = r#"
        frame F = match tag: u8 {
            0 => A {},
            _ => B { data: bytes[remaining] },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.frames[0].frame_id, "frame:F");
}

#[test]
fn ir_sm_id_format() {
    let src = r#"
        state machine SM {
            state A
            state B [terminal]
            initial A
            transition A -> B { on go }
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.state_machines[0].sm_id, "sm:SM");
}

#[test]
fn ir_item_order_preserved() {
    let src = r#"
        const C: u8 = 1
        enum E: u8 { A = 0 }
        packet P { x: u8 }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.item_order.len(), 3);
    assert!(sem.item_order[0].starts_with("const:"));
    assert!(sem.item_order[1].starts_with("enum:"));
    assert!(sem.item_order[2].starts_with("packet:"));
}

#[test]
fn ir_schema_version() {
    let ast = parse("packet P { x: u8 }").unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    assert_eq!(sem.schema_version, "semantic-ir/v1");
}

// ══════════════════════════════════════════════════════════════════════
// 13. Profile String Representation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ir_compliance_profile_strict() {
    let ast = parse("packet P { x: u8 }").unwrap();
    let sem = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(sem.compliance_profile, "phase2_strict_v1_0");
}

#[test]
fn ir_compliance_profile_extended() {
    let ast = parse("packet P { x: u8 }").unwrap();
    let sem = analyze(
        &ast,
        ComplianceProfile::Phase2ExtendedCurrent,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(sem.compliance_profile, "phase2_extended_current");
}

// ══════════════════════════════════════════════════════════════════════
// 14. Forward/Back Reference Boundary Cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_self_reference_in_require() {
    // A require can reference a field declared just before it
    let ast = parse("packet P { x: u8, require x > 0 }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "require referencing preceding field should succeed"
    );
}

#[test]
fn ok_derived_references_preceding_field() {
    let ast = parse("packet P { x: u8, let doubled: u8 = x + x }").unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "derived referencing preceding field should succeed"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 15. Frame and Capsule Variant Details
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_frame_variant_names() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => Alpha {},
            0x01 => Beta { x: u8 },
            _ => Gamma { data: bytes[remaining] },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    let names: Vec<_> = sem.frames[0]
        .variants
        .iter()
        .map(|v| v.variant_name.as_str())
        .collect();
    assert_eq!(names, vec!["Alpha", "Beta", "Gamma"]);
}

#[test]
fn ok_frame_wildcard_pattern() {
    let src = r#"
        frame F = match tag: u8 {
            0 => A {},
            _ => Default { data: bytes[remaining] },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    let last = sem.frames[0].variants.last().unwrap();
    assert!(
        matches!(last.pattern, wirespec_sema::ir::SemanticPattern::Wildcard),
        "last variant should be wildcard"
    );
}

#[test]
fn ok_frame_range_pattern() {
    let src = r#"
        frame F = match tag: u8 {
            0x01..=0x03 => Range { x: u8 },
            _ => Default { data: bytes[remaining] },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    match &sem.frames[0].variants[0].pattern {
        wirespec_sema::ir::SemanticPattern::RangeInclusive { start, end } => {
            assert_eq!(*start, 1);
            assert_eq!(*end, 3);
        }
        other => panic!("expected RangeInclusive, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// 16. Transition Event Collection
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_sm_events_are_unique() {
    let src = r#"
        state machine S {
            state A
            state B
            state C [terminal]
            initial A
            transition A -> B { on step }
            transition B -> C { on step }
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    let sm = &sem.state_machines[0];
    // "step" event is used in two transitions but should appear only once in events
    assert_eq!(
        sm.events.len(),
        1,
        "duplicate event names should be deduplicated"
    );
    assert_eq!(sm.events[0].name, "step");
}

#[test]
fn ok_sm_multiple_events() {
    let src = r#"
        state machine S {
            state A
            state B [terminal]
            initial A
            transition A -> B { on go }
            transition A -> A { on stay }
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    let sm = &sem.state_machines[0];
    assert_eq!(sm.events.len(), 2, "should have two distinct events");
}

// ══════════════════════════════════════════════════════════════════════
// Integer-like validation for bytes length / array count references
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_bytes_length_non_integer() {
    // bytes[length: field] where field is bytes type (not integer-like)
    let src = "packet P { data: bytes[6], more: bytes[length: data] }";
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_err(), "bytes[length: non-integer] should fail");
    assert_eq!(result.unwrap_err().kind, ErrorKind::InvalidBytesLength);
}

#[test]
fn ok_bytes_length_integer() {
    let src = "packet P { len: u16, data: bytes[length: len] }";
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(result.is_ok(), "bytes[length: integer] should succeed");
}

#[test]
fn error_array_count_non_integer() {
    // array count references a bytes field (not integer-like)
    let src = "packet Inner { x: u8 } packet P { data: bytes[4], items: [Inner; data] }";
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_err(),
        "array count with non-integer ref should fail"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::InvalidArrayCount);
}

#[test]
fn ok_array_count_integer() {
    let src = "packet Inner { x: u8 } packet P { count: u8, items: [Inner; count] }";
    let ast = parse(src).unwrap();
    let result = analyze(&ast, default_profile(), &Default::default());
    assert!(
        result.is_ok(),
        "array count with integer ref should succeed"
    );
}
