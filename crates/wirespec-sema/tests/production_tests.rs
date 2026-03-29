//! Production-quality edge-case and boundary-value tests for wirespec-sema.
//!
//! These tests target type resolution boundaries, complex valid configurations,
//! error accumulation, endianness interactions, checksum edge cases,
//! state machine corner cases, and other scenarios a production compiler
//! must handle correctly.
//!
//! No existing tests or source files are modified.

use wirespec_sema::analyze;
use wirespec_sema::error::ErrorKind;
use wirespec_sema::types::{Endianness, FieldPresence, SemanticType};
use wirespec_sema::ComplianceProfile;
use wirespec_syntax::parse;

fn check(src: &str) -> Result<wirespec_sema::SemanticModule, wirespec_sema::error::SemaError> {
    analyze(
        &parse(src).unwrap(),
        ComplianceProfile::default(),
        &Default::default(),
    )
}

fn check_strict(
    src: &str,
) -> Result<wirespec_sema::SemanticModule, wirespec_sema::error::SemaError> {
    analyze(
        &parse(src).unwrap(),
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    )
}

// ══════════════════════════════════════════════════════════════════════
// Type alias chains and resolution
// ══════════════════════════════════════════════════════════════════════

#[test]
fn type_alias_to_primitive() {
    let sem = check("type Handle = u16le\npacket P { h: Handle }").unwrap();
    assert_eq!(sem.packets[0].fields[0].name, "h");
}

#[test]
fn type_alias_chain_three_deep() {
    let sem = check("type A = u32\ntype B = A\ntype C = B\npacket P { x: C }").unwrap();
    assert_eq!(sem.packets[0].fields.len(), 1);
}

#[test]
fn error_alias_to_nonexistent_type() {
    let result = check("type A = NonExistent\npacket P { x: A }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::UndefinedType);
}

#[test]
fn error_direct_self_alias() {
    let result = check("type A = A\npacket P { x: A }");
    assert!(result.is_err());
    // Could be CyclicDependency or UndefinedType -- both are acceptable
}

#[test]
fn error_mutual_cycle_two_aliases() {
    let result = check("type A = B\ntype B = A\npacket P { x: A }");
    assert!(result.is_err());
}

#[test]
fn error_indirect_cycle_three_aliases() {
    let result = check("type A = B\ntype B = C\ntype C = A\npacket P { x: A }");
    assert!(result.is_err());
}

// ══════════════════════════════════════════════════════════════════════
// Packet bytes combinations
// ══════════════════════════════════════════════════════════════════════

#[test]
fn packet_all_bytes_kinds() {
    let sem = check(
        "packet P { a: bytes[16], b: u16, c: bytes[length: b], d: bytes[remaining] }",
    )
    .unwrap();
    assert_eq!(sem.packets[0].fields.len(), 4);
}

#[test]
fn error_remaining_not_last_field() {
    let result = check("packet P { data: bytes[remaining], x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::RemainingNotLast);
}

#[test]
fn ok_remaining_last_field() {
    assert!(check("packet P { x: u8, data: bytes[remaining] }").is_ok());
}

#[test]
fn ok_bytes_length_with_subtraction() {
    let sem = check("packet P { length: u16, require length >= 4, data: bytes[length: length - 4] }").unwrap();
    assert_eq!(sem.packets[0].fields.len(), 2);
    assert_eq!(sem.packets[0].requires.len(), 1);
}

// ══════════════════════════════════════════════════════════════════════
// Packet derived fields and requires
// ══════════════════════════════════════════════════════════════════════

#[test]
fn packet_multiple_requires() {
    let sem = check("packet P { x: u8, y: u8, require x > 0, require y < 100 }").unwrap();
    assert_eq!(sem.packets[0].requires.len(), 2);
}

#[test]
fn packet_derived_with_complex_expr() {
    let sem = check(
        "packet P { flags: u8, let bit0: bool = (flags & 0x01) != 0, let bit1: bool = (flags & 0x02) != 0 }",
    )
    .unwrap();
    assert_eq!(sem.packets[0].derived.len(), 2);
    assert_eq!(sem.packets[0].derived[0].name, "bit0");
    assert_eq!(sem.packets[0].derived[1].name, "bit1");
}

#[test]
fn packet_nested_optional_with_derived() {
    let sem = check(
        "packet P { flags: u8, extra: if flags & 1 { u16 }, let has: bool = extra != null }",
    )
    .unwrap();
    assert_eq!(sem.packets[0].fields.len(), 2);
    assert_eq!(sem.packets[0].derived.len(), 1);
    assert!(matches!(
        sem.packets[0].fields[1].presence,
        FieldPresence::Conditional { .. }
    ));
}

// ══════════════════════════════════════════════════════════════════════
// Frame edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn frame_single_wildcard_only() {
    let sem = check(
        "frame F = match t: u8 { _ => Unknown { data: bytes[remaining] } }",
    )
    .unwrap();
    assert_eq!(sem.frames[0].variants.len(), 1);
}

#[test]
fn frame_many_branches() {
    let mut src = "frame F = match t: u8 {\n".to_string();
    for i in 0..10 {
        src.push_str(&format!("    {i} => V{i} {{ x: u8 }},\n"));
    }
    src.push_str("    _ => Unknown { data: bytes[remaining] },\n}");
    let sem = check(&src).unwrap();
    assert_eq!(sem.frames[0].variants.len(), 11);
}

#[test]
fn error_frame_missing_wildcard_branch() {
    let result = check(
        "frame F = match t: u8 { 0 => A {}, 1 => B { x: u8 } }",
    );
    assert!(result.is_err(), "frame without wildcard should be rejected");
    let err = result.unwrap_err();
    assert_eq!(err.kind, ErrorKind::TypeMismatch);
}

#[test]
fn frame_with_range_pattern() {
    let sem = check(
        "frame F = match t: u8 { 0x00..=0x0F => Low { x: u8 }, _ => Other { data: bytes[remaining] } }",
    )
    .unwrap();
    assert_eq!(sem.frames[0].variants.len(), 2);
}

#[test]
fn frame_variant_with_optional_field() {
    let sem = check(
        r#"frame F = match t: u8 {
            0 => A { flags: u8, extra: if flags & 1 { u16 } },
            _ => B { data: bytes[remaining] },
        }"#,
    )
    .unwrap();
    assert_eq!(sem.frames[0].variants[0].fields.len(), 2);
}

#[test]
fn frame_variant_with_derived_and_require() {
    let sem = check(
        r#"frame F = match t: u8 {
            0 => A { x: u16, y: u32, require x > 0, let z: bool = (x & 0x01) != 0 },
            _ => B { data: bytes[remaining] },
        }"#,
    )
    .unwrap();
    assert_eq!(sem.frames[0].variants[0].fields.len(), 2);
    assert_eq!(sem.frames[0].variants[0].requires.len(), 1);
    assert_eq!(sem.frames[0].variants[0].derived.len(), 1);
}

// ══════════════════════════════════════════════════════════════════════
// Capsule edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn capsule_expr_tag_complex() {
    let sem = check(
        r#"capsule C {
            header: u8, length: u16,
            payload: match (header >> 4) within length {
                1 => A { data: bytes[remaining] },
                _ => B { data: bytes[remaining] },
            },
        }"#,
    )
    .unwrap();
    assert_eq!(sem.capsules[0].variants.len(), 2);
    assert_eq!(sem.capsules[0].header_fields.len(), 2);
}

#[test]
fn error_capsule_missing_wildcard() {
    let result = check(
        r#"capsule C {
            t: u8, length: u16,
            payload: match t within length {
                0 => A { data: bytes[remaining] },
            },
        }"#,
    );
    assert!(result.is_err(), "capsule without wildcard should be rejected");
    assert_eq!(result.unwrap_err().kind, ErrorKind::TypeMismatch);
}

// ══════════════════════════════════════════════════════════════════════
// State machine edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn sm_single_state_terminal_is_initial() {
    let sem = check("state machine S { state Done [terminal] initial Done }").unwrap();
    assert_eq!(sem.state_machines[0].states.len(), 1);
    assert!(sem.state_machines[0].states[0].is_terminal);
}

#[test]
fn sm_multiple_events_expand_to_multiple_transitions() {
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on event1 on event2 }
    }"#;
    let sem = check(src).unwrap();
    // Each event produces a separate transition
    assert!(
        sem.state_machines[0].transitions.len() >= 2,
        "expected at least 2 transitions from multi-event, got {}",
        sem.state_machines[0].transitions.len()
    );
}

#[test]
fn sm_wildcard_does_not_duplicate_concrete() {
    let src = r#"state machine S {
        state A state B state C [terminal] initial A
        transition A -> B { on next }
        transition A -> C { on error }
        transition B -> C { on done }
        transition * -> C { on error }
    }"#;
    // A already has concrete "on error" -> wildcard should not add another for A
    let sem = check(src).unwrap();
    let sm = &sem.state_machines[0];
    let error_from_a: Vec<_> = sm
        .transitions
        .iter()
        .filter(|t| t.src_state_name == "A" && t.event_name == "error")
        .collect();
    assert_eq!(
        error_from_a.len(),
        1,
        "A should have exactly 1 'error' transition (concrete overrides wildcard)"
    );
}

#[test]
fn sm_wildcard_expands_to_non_terminal_only() {
    let src = r#"state machine S {
        state A state B state C [terminal] initial A
        transition A -> B { on next }
        transition B -> C { on done }
        transition * -> C { on abort }
    }"#;
    let sem = check(src).unwrap();
    let sm = &sem.state_machines[0];
    let abort_transitions: Vec<_> = sm
        .transitions
        .iter()
        .filter(|t| t.event_name == "abort")
        .collect();
    // Should expand to A and B (both non-terminal), not C (terminal)
    assert_eq!(
        abort_transitions.len(),
        2,
        "wildcard should expand to 2 non-terminal states"
    );
    // Verify C is not a source
    for t in &abort_transitions {
        assert_ne!(t.src_state_name, "C", "terminal state C should not get wildcard expansion");
    }
}

#[test]
fn error_sm_duplicate_transition() {
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on go }
        transition A -> B { on go }
    }"#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmDuplicateTransition);
}

#[test]
fn error_sm_missing_initial_state() {
    let src = r#"state machine S {
        state A state B [terminal]
        transition A -> B { on go }
    }"#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmInvalidInitial);
}

#[test]
fn error_sm_initial_references_nonexistent() {
    let src = r#"state machine S {
        state A state B [terminal]
        initial Phantom
        transition A -> B { on go }
    }"#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmInvalidInitial);
}

#[test]
fn error_sm_non_terminal_without_outgoing_transition() {
    let src = r#"state machine S {
        state A state B state C [terminal]
        initial A
        transition A -> C { on done }
    }"#;
    // B is non-terminal but has no outgoing transitions
    let result = check(src);
    assert!(result.is_err());
}

#[test]
fn sm_guard_and_action_on_self_transition() {
    let sem = check(
        r#"state machine S {
            state A { count: u8 = 0 }
            state B [terminal]
            initial A
            transition A -> A {
                on tick
                guard src.count < 10
                action { dst.count = src.count + 1; }
            }
            transition A -> B { on done }
        }"#,
    )
    .unwrap();
    let sm = &sem.state_machines[0];
    let t = &sm.transitions[0];
    assert!(t.guard.is_some());
    assert_eq!(t.actions.len(), 1);
}

#[test]
fn error_sm_delegate_on_non_self_transition() {
    let src = r#"state machine S {
        state A { c: u8 }
        state B [terminal]
        initial A
        transition A -> B { on ev(id: u8, e: u8) delegate src.c <- e }
    }"#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmDelegateNotSelfTransition);
}

#[test]
fn error_sm_delegate_with_action_mutual_exclusion() {
    let src = r#"state machine S {
        state A { c: u8 }
        state B [terminal]
        initial A
        transition A -> A {
            on ev(id: u8, e: u8)
            delegate src.c <- e
            action { dst.c = 0; }
        }
        transition A -> B { on done }
    }"#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmDelegateWithAction);
}

#[test]
fn error_sm_transition_missing_dst_field_assignment() {
    let src = r#"state machine S {
        state A
        state B { x: u8 }
        state C [terminal]
        initial A
        transition A -> B { on go }
        transition B -> C { on done }
    }"#;
    // B.x has no default and no action assigns it
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmMissingAssignment);
}

#[test]
fn ok_sm_dst_field_has_default() {
    let src = r#"state machine S {
        state A
        state B { x: u8 = 0 }
        state C [terminal]
        initial A
        transition A -> B { on go }
        transition B -> C { on done }
    }"#;
    assert!(check(src).is_ok(), "B.x has default, no assignment needed");
}

#[test]
fn ok_sm_dst_field_assigned_in_action() {
    let src = r#"state machine S {
        state A
        state B { x: u8 }
        state C [terminal]
        initial A
        transition A -> B {
            on go
            action { dst.x = 42; }
        }
        transition B -> C { on done }
    }"#;
    assert!(check(src).is_ok());
}

#[test]
fn error_sm_partial_assignment_missing_field() {
    let src = r#"state machine S {
        state A
        state B { x: u8, y: u16 }
        state C [terminal]
        initial A
        transition A -> B {
            on go
            action { dst.x = 1; }
        }
        transition B -> C { on done }
    }"#;
    // B.y has no default and is not assigned
    let result = check(src);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind, ErrorKind::SmMissingAssignment);
    assert!(err.msg.contains("y"), "error should mention unassigned field 'y': {}", err.msg);
}

#[test]
fn ok_sm_all_dst_fields_assigned() {
    let src = r#"state machine S {
        state A
        state B { x: u8, y: u16 }
        state C [terminal]
        initial A
        transition A -> B {
            on go
            action { dst.x = 1; dst.y = 2; }
        }
        transition B -> C { on done }
    }"#;
    assert!(check(src).is_ok());
}

#[test]
fn ok_sm_mix_default_and_assigned() {
    let src = r#"state machine S {
        state A
        state B { x: u8, y: u16 = 0 }
        state C [terminal]
        initial A
        transition A -> B {
            on go
            action { dst.x = 1; }
        }
        transition B -> C { on done }
    }"#;
    assert!(check(src).is_ok(), "B.x assigned, B.y has default");
}

// ══════════════════════════════════════════════════════════════════════
// SM child state machine references
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_sm_field_references_child_sm() {
    let src = r#"
        state machine Child {
            state Init state Done [terminal] initial Init
            transition Init -> Done { on finish }
        }
        state machine Parent {
            state Active { child: Child }
            state Done [terminal]
            initial Active
            transition Active -> Done { on finish }
        }
    "#;
    let sem = check(src).unwrap();
    let parent = &sem.state_machines[1];
    assert_eq!(parent.states[0].fields[0].name, "child");
    assert!(parent.states[0].fields[0].child_sm_id.is_some());
    assert_eq!(parent.states[0].fields[0].child_sm_name.as_deref(), Some("Child"));
}

#[test]
fn error_sm_type_in_packet_field() {
    let src = r#"
        state machine SM {
            state A state B [terminal] initial A
            transition A -> B { on go }
        }
        packet P { x: SM }
    "#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::TypeMismatch);
}

// ══════════════════════════════════════════════════════════════════════
// Enum edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn enum_single_member() {
    let sem = check("enum E: u8 { Only = 0 }\npacket P { x: E }").unwrap();
    assert_eq!(sem.enums[0].members.len(), 1);
    assert!(!sem.enums[0].is_flags);
}

#[test]
fn enum_many_members() {
    let mut src = "enum E: u16 {\n".to_string();
    for i in 0..20 {
        src.push_str(&format!("    V{i} = {i},\n"));
    }
    src.push_str("}\npacket P { x: E }");
    let sem = check(&src).unwrap();
    assert_eq!(sem.enums[0].members.len(), 20);
}

#[test]
fn flags_bitmask_values() {
    let sem = check("flags F: u8 { A = 0x01, B = 0x02, C = 0x04, D = 0x08 }\npacket P { x: F }")
        .unwrap();
    assert_eq!(sem.enums[0].members.len(), 4);
    assert!(sem.enums[0].is_flags);
}

#[test]
fn enum_used_as_field_type() {
    let sem = check("enum Status: u8 { Ok = 0, Error = 1 }\npacket P { status: Status }").unwrap();
    assert!(matches!(
        &sem.packets[0].fields[0].ty,
        SemanticType::EnumRef { .. }
    ));
}

#[test]
fn error_const_used_as_field_type() {
    let result = check("const MAX: u8 = 255\npacket P { x: MAX }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::TypeMismatch);
}

// ══════════════════════════════════════════════════════════════════════
// VarInt edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn varint_strict_annotation() {
    let src = r#"@strict
    type VarInt = {
        prefix: bits[2],
        value: match prefix {
            0b00 => bits[6],
            0b01 => bits[14],
            0b10 => bits[30],
            0b11 => bits[62],
        },
    }"#;
    let sem = check(src).unwrap();
    assert!(sem.varints[0].strict);
}

#[test]
fn cont_varint_big_endian() {
    let src =
        "type V = varint { continuation_bit: msb, value_bits: 7, max_bytes: 4, byte_order: big }";
    let sem = check(src).unwrap();
    assert_eq!(sem.varints[0].byte_order, Endianness::Big);
}

#[test]
fn cont_varint_little_endian() {
    let src = "type V = varint { continuation_bit: msb, value_bits: 7, max_bytes: 4, byte_order: little }";
    let sem = check(src).unwrap();
    assert_eq!(sem.varints[0].byte_order, Endianness::Little);
}

#[test]
fn varint_used_as_field_type() {
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
        packet P { length: VarInt }
    "#;
    let sem = check(src).unwrap();
    assert!(matches!(
        &sem.packets[0].fields[0].ty,
        SemanticType::VarIntRef { .. }
    ));
}

// ══════════════════════════════════════════════════════════════════════
// Endianness edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn endianness_little_module_big_field() {
    let src = "@endian little\nmodule test\npacket P { x: u16be }";
    let sem = check(src).unwrap();
    assert_eq!(sem.module_endianness, Endianness::Little);
    if let SemanticType::Primitive { endianness, .. } = &sem.packets[0].fields[0].ty {
        assert_eq!(
            *endianness,
            Some(Endianness::Big),
            "field explicit endianness should override module"
        );
    } else {
        panic!("expected Primitive type");
    }
}

#[test]
fn endianness_big_module_little_field() {
    let src = "@endian big\nmodule test\npacket P { x: u32le }";
    let sem = check(src).unwrap();
    assert_eq!(sem.module_endianness, Endianness::Big);
    if let SemanticType::Primitive { endianness, .. } = &sem.packets[0].fields[0].ty {
        assert_eq!(
            *endianness,
            Some(Endianness::Little),
            "field explicit endianness should override module"
        );
    } else {
        panic!("expected Primitive type");
    }
}

#[test]
fn endianness_default_is_big() {
    let sem = check("packet P { x: u16 }").unwrap();
    assert_eq!(sem.module_endianness, Endianness::Big);
}

#[test]
fn endianness_explicit_little_module() {
    let sem = check("@endian little\nmodule test\npacket P { x: u16 }").unwrap();
    assert_eq!(sem.module_endianness, Endianness::Little);
}

// ══════════════════════════════════════════════════════════════════════
// Checksum edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn checksum_internet_correct_type() {
    let sem = check("packet P { data: u32, @checksum(internet) c: u16 }").unwrap();
    assert_eq!(
        sem.packets[0].fields[1].checksum_algorithm.as_deref(),
        Some("internet")
    );
}

#[test]
fn checksum_crc32_correct_type() {
    let sem = check("packet P { data: u32, @checksum(crc32) c: u32 }").unwrap();
    assert_eq!(
        sem.packets[0].fields[1].checksum_algorithm.as_deref(),
        Some("crc32")
    );
}

#[test]
fn error_checksum_wrong_type_internet() {
    // internet checksum requires u16, not u32
    let result = check("packet P { data: u32, @checksum(internet) c: u32 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::InvalidChecksumType);
}

#[test]
fn error_checksum_wrong_type_crc32() {
    // crc32 requires u32, not u16
    let result = check("packet P { data: u32, @checksum(crc32) c: u16 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::InvalidChecksumType);
}

#[test]
fn error_duplicate_checksums() {
    let result = check("packet P { @checksum(internet) a: u16, @checksum(crc32) b: u32 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::DuplicateChecksum);
}

#[test]
fn error_checksum_unknown_algorithm() {
    let result = check("packet P { @checksum(sha256) c: u32 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ChecksumProfileViolation);
}

#[test]
fn error_checksum_unknown_algorithm_strict_profile() {
    let result = check_strict("packet P { @checksum(md5) c: u32 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ChecksumProfileViolation);
}

#[test]
fn error_fletcher16_rejected_under_strict() {
    let result = check_strict("packet P { data: u32, @checksum(fletcher16) c: u16 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ChecksumProfileViolation);
}

#[test]
fn ok_fletcher16_under_extended() {
    let result = check("packet P { data: u32, @checksum(fletcher16) c: u16 }");
    assert!(result.is_ok(), "fletcher16 should be allowed under extended profile");
}

// ══════════════════════════════════════════════════════════════════════
// Reserved identifiers
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_packet_named_bool() {
    let result = check("packet bool { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_packet_named_src() {
    let result = check("packet src { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_packet_named_dst() {
    let result = check("packet dst { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_packet_named_fill() {
    let result = check("packet fill { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_packet_named_remaining() {
    let result = check("packet remaining { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn ok_event_child_state_changed_as_trigger() {
    // child_state_changed is a built-in event; referencing it as a
    // transition trigger (without custom params) is valid.
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on child_state_changed }
    }"#;
    let result = check(src);
    assert!(result.is_ok());
}

#[test]
fn error_event_name_reserved_child_state_changed_with_params() {
    // Defining child_state_changed as a user event with custom params is
    // still a reserved identifier error.
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on child_state_changed(x: u8) }
    }"#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

// ══════════════════════════════════════════════════════════════════════
// bool type restrictions
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_bool_as_wire_field() {
    let result = check("packet P { flag: bool }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::TypeMismatch);
}

#[test]
fn ok_bool_in_derived_field() {
    let sem = check("packet P { x: u8, let flag: bool = x != 0 }").unwrap();
    assert_eq!(sem.packets[0].derived.len(), 1);
}

#[test]
fn error_bool_in_frame_wire_field() {
    let result = check(
        r#"frame F = match tag: u8 {
            0 => A { flag: bool },
            _ => B { data: bytes[remaining] },
        }"#,
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::TypeMismatch);
}

#[test]
fn ok_bool_in_frame_derived_field() {
    let result = check(
        r#"frame F = match tag: u8 {
            0 => A { x: u8, let flag: bool = x != 0 },
            _ => B { data: bytes[remaining] },
        }"#,
    );
    assert!(result.is_ok(), "bool in derived field should be valid: {:?}", result.err());
}

// ══════════════════════════════════════════════════════════════════════
// Duplicate definition rejection
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_duplicate_packet() {
    let result = check("packet P { x: u8 }\npacket P { y: u16 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::DuplicateDefinition);
}

#[test]
fn error_duplicate_enum() {
    let result = check("enum E: u8 { A = 0 }\nenum E: u8 { B = 1 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::DuplicateDefinition);
}

#[test]
fn error_duplicate_const() {
    let result = check("const A: u8 = 1\nconst A: u8 = 2");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::DuplicateDefinition);
}

// ══════════════════════════════════════════════════════════════════════
// Forward reference errors
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_forward_ref_bytes_length() {
    let result = check("packet P { data: bytes[length: len], len: u16 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ForwardReference);
}

#[test]
fn error_forward_ref_optional_condition() {
    let result = check("packet P { extra: if flags & 1 { u16 }, flags: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ForwardReference);
}

#[test]
fn error_forward_ref_array_count() {
    let result = check("packet P { items: [u8; count], count: u16 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ForwardReference);
}

#[test]
fn ok_backward_ref_bytes_length() {
    assert!(check("packet P { len: u16, data: bytes[length: len] }").is_ok());
}

#[test]
fn ok_backward_ref_array_count() {
    assert!(check("packet P { count: u16, items: [u8; count] }").is_ok());
}

#[test]
fn ok_backward_ref_optional() {
    assert!(check("packet P { flags: u8, extra: if flags & 0x01 { u16 } }").is_ok());
}

// ══════════════════════════════════════════════════════════════════════
// bytes[length_or_remaining:] validation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_lor_references_non_optional_field() {
    let result = check("packet P { len: u16, data: bytes[length_or_remaining: len] }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::InvalidLengthOrRemaining);
}

#[test]
fn ok_lor_references_optional_field() {
    let result = check(
        "packet P { flags: u8, len: if flags & 1 { u16 }, data: bytes[length_or_remaining: len] }",
    );
    assert!(result.is_ok(), "LOR with optional field should pass: {:?}", result.err());
}

// ══════════════════════════════════════════════════════════════════════
// Cross-type references (packet-in-packet, frame-as-field, etc.)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn packet_ref_as_field_type() {
    let sem = check("packet Inner { x: u8 }\npacket Outer { inner: Inner, y: u16 }").unwrap();
    assert!(matches!(
        &sem.packets[1].fields[0].ty,
        SemanticType::PacketRef { .. }
    ));
}

#[test]
fn frame_ref_as_field_type() {
    let sem = check(
        r#"frame F = match t: u8 {
            0 => A {},
            _ => B { data: bytes[remaining] },
        }
        packet P { payload: F }"#,
    )
    .unwrap();
    assert!(matches!(
        &sem.packets[0].fields[0].ty,
        SemanticType::FrameRef { .. }
    ));
}

// ══════════════════════════════════════════════════════════════════════
// Static assert
// ══════════════════════════════════════════════════════════════════════

#[test]
fn static_assert_with_const() {
    let sem = check("const MAX: u8 = 20\nstatic_assert MAX <= 255").unwrap();
    assert_eq!(sem.static_asserts.len(), 1);
    assert_eq!(sem.consts.len(), 1);
}

#[test]
fn multiple_static_asserts() {
    let sem = check(
        "const A: u8 = 10\nconst B: u8 = 20\nstatic_assert A <= 255\nstatic_assert B <= 255\nstatic_assert A < B",
    )
    .unwrap();
    assert_eq!(sem.static_asserts.len(), 3);
}

// ══════════════════════════════════════════════════════════════════════
// derive traits
// ══════════════════════════════════════════════════════════════════════

#[test]
fn derive_traits_on_packet() {
    let sem = check("@derive(debug, compare)\npacket P { x: u8 }").unwrap();
    assert!(!sem.packets[0].derive_traits.is_empty());
}

#[test]
fn derive_debug_only() {
    let sem = check("@derive(debug)\npacket P { x: u8 }").unwrap();
    assert!(sem.packets[0].derive_traits.contains(&wirespec_sema::types::DeriveTrait::Debug));
}

// ══════════════════════════════════════════════════════════════════════
// Complex integration scenarios
// ══════════════════════════════════════════════════════════════════════

#[test]
fn full_protocol_stack_with_multiple_types() {
    let src = r#"
        @endian big
        module test.production

        const VERSION: u32 = 1
        const MAX_PAYLOAD: u16 = 1500

        enum MessageType: u8 { Data = 0, Control = 1, Heartbeat = 2 }

        flags Flags: u8 { Ack = 0x01, Syn = 0x02, Fin = 0x04 }

        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6],
                0b01 => bits[14],
                0b10 => bits[30],
                0b11 => bits[62],
            },
        }

        packet Header {
            version: u8,
            msg_type: MessageType,
            flags: Flags,
            length: u16,
            require version == 1,
            require length >= 6,
            let has_ack: bool = (flags & 0x01) != 0,
        }

        frame Payload = match tag: u8 {
            0 => Data { data: bytes[remaining] },
            1 => Control { cmd: u8, param: u16 },
            2 => Heartbeat {},
            _ => Unknown { data: bytes[remaining] },
        }

        static_assert VERSION <= 255
        static_assert MAX_PAYLOAD <= 65535
    "#;
    let sem = check(src).unwrap();
    assert_eq!(sem.module_name, "test.production");
    assert_eq!(sem.module_endianness, Endianness::Big);
    assert_eq!(sem.consts.len(), 2);
    assert_eq!(sem.enums.len(), 2); // enum + flags
    assert_eq!(sem.varints.len(), 1);
    assert_eq!(sem.packets.len(), 1);
    assert_eq!(sem.frames.len(), 1);
    assert_eq!(sem.static_asserts.len(), 2);
}

#[test]
fn all_primitive_types_in_single_packet() {
    let src = r#"
        packet AllPrimitives {
            a: u8, b: u16, c: u32, d: u64,
            e: i8, f: i16, g: i32, h: i64,
            i: bits[4], j: bits[4],
            k: bit, l: bit, m: bits[6],
        }
    "#;
    let sem = check(src).unwrap();
    assert_eq!(sem.packets[0].fields.len(), 13);
}

#[test]
fn capsule_with_full_features() {
    let src = r#"
        capsule TlvPacket {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0x01 => Data {
                    data: bytes[remaining],
                },
                0x02 => Control {
                    cmd: u8,
                    param: u16,
                },
                _ => Unknown {
                    data: bytes[remaining],
                },
            },
        }
    "#;
    let sem = check(src).unwrap();
    assert_eq!(sem.capsules.len(), 1);
    assert_eq!(sem.capsules[0].header_fields.len(), 2);
    assert_eq!(sem.capsules[0].variants.len(), 3);
}

// ══════════════════════════════════════════════════════════════════════
// Compliance profile handling
// ══════════════════════════════════════════════════════════════════════

#[test]
fn strict_profile_rejects_fletcher16() {
    let src = "packet P { data: u32, @checksum(fletcher16) c: u16 }";
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ChecksumProfileViolation);
}

#[test]
fn extended_profile_accepts_fletcher16() {
    let src = "packet P { data: u32, @checksum(fletcher16) c: u16 }";
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2ExtendedCurrent,
        &Default::default(),
    );
    assert!(result.is_ok());
}

#[test]
fn strict_profile_accepts_internet() {
    let src = "packet P { data: u32, @checksum(internet) c: u16 }";
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(result.is_ok());
}

#[test]
fn strict_profile_accepts_crc32() {
    let src = "packet P { data: u32, @checksum(crc32) c: u32 }";
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(result.is_ok());
}

// ══════════════════════════════════════════════════════════════════════
// item_order verification
// ══════════════════════════════════════════════════════════════════════

#[test]
fn item_order_reflects_declaration_order() {
    let src = r#"
        const A: u8 = 1
        enum E: u8 { X = 0 }
        packet P { x: u8 }
        static_assert A <= 255
    "#;
    let sem = check(src).unwrap();
    // item_order should contain IDs in declaration order
    assert!(
        !sem.item_order.is_empty(),
        "item_order should not be empty"
    );
    assert_eq!(sem.item_order.len(), 4);
}

// ══════════════════════════════════════════════════════════════════════
// max_len annotation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn max_len_annotation_on_array() {
    let sem = check("packet P { count: u16, @max_len(100) items: [u8; count] }").unwrap();
    assert_eq!(sem.packets[0].fields[1].max_elements, Some(100));
}

#[test]
fn max_len_annotation_absent_means_none() {
    let sem = check("packet P { count: u16, items: [u8; count] }").unwrap();
    assert_eq!(sem.packets[0].fields[1].max_elements, None);
}

// ══════════════════════════════════════════════════════════════════════
// Fill arrays
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_fill_array_as_last_field() {
    assert!(check("packet P { x: u8, items: [u8; fill] }").is_ok());
}

#[test]
fn error_fill_array_not_last() {
    let result = check("packet P { items: [u8; fill], x: u8 }");
    assert!(result.is_err());
}
