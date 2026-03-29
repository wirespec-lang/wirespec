//! Spec compliance tests for wirespec-sema.
//!
//! These tests verify fixes for 8 spec compliance findings:
//!   1. Reserved identifier list completeness (fill, remaining, in_state, all)
//!   2. Event name reserved-identifier checking
//!   3. State machine type references in state fields (child SM)
//!   4. SmMissingAssignment validation (spec §3.9 rule 2a)
//!   6. bool rejected as wire field type
//!   7. Wildcard transition priority (concrete overrides wildcard)
//!   8. Match exhaustiveness (wildcard required in frames/capsules)

use wirespec_sema::ComplianceProfile;
use wirespec_sema::analyze;
use wirespec_sema::error::ErrorKind;
use wirespec_syntax::parse;

fn check(src: &str) -> Result<wirespec_sema::SemanticModule, wirespec_sema::error::SemaError> {
    analyze(
        &parse(src).unwrap(),
        ComplianceProfile::default(),
        &Default::default(),
    )
}

// ══════════════════════════════════════════════════════════════════════
// Finding 1: Reserved identifiers — expanded list
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_reserved_fill_as_type() {
    let result = check("packet fill { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_reserved_remaining_as_type() {
    let result = check("packet remaining { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_reserved_in_state_as_type() {
    let result = check("packet in_state { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_reserved_all_as_type() {
    let result = check("packet all { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_reserved_bool_as_type() {
    // bool was already reserved; verify it still is
    let result = check("packet bool { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_reserved_src_as_type() {
    let result = check("packet src { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_reserved_dst_as_type() {
    let result = check("packet dst { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_reserved_child_state_changed_as_type() {
    let result = check("packet child_state_changed { x: u8 }");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

// ══════════════════════════════════════════════════════════════════════
// Finding 2: Event name reserved-identifier checking
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_reserved_event_name_child_state_changed_as_trigger() {
    // child_state_changed is a built-in event; referencing it as a bare
    // transition trigger (without custom params) is valid per spec.
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on child_state_changed }
    }"#;
    let result = check(src);
    assert!(result.is_ok());
}

#[test]
fn error_reserved_event_name_child_state_changed_user_defined() {
    // Defining child_state_changed as a user event WITH custom params
    // is still a reserved identifier error.
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on child_state_changed(x: u8) }
    }"#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_reserved_event_name_src() {
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on src }
    }"#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_reserved_event_name_dst() {
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on dst }
    }"#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

#[test]
fn error_reserved_event_name_fill() {
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on fill }
    }"#;
    let result = check(src);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::ReservedIdentifier);
}

// ══════════════════════════════════════════════════════════════════════
// Finding 3: SM type references in state fields
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_sm_field_references_sm_type() {
    let src = r#"
        state machine Child {
            state Init
            state Done [terminal]
            initial Init
            transition Init -> Done { on finish }
        }
        state machine Parent {
            state Active { child: Child }
            state Done [terminal]
            initial Active
            transition Active -> Done { on finish }
        }
    "#;
    let result = check(src);
    assert!(
        result.is_ok(),
        "SM-typed state field should be allowed: {:?}",
        result.err()
    );
    let sem = result.unwrap();
    let parent = &sem.state_machines[1];
    let active_state = &parent.states[0];
    assert_eq!(active_state.fields.len(), 1);
    assert_eq!(active_state.fields[0].name, "child");
    assert!(
        active_state.fields[0].child_sm_id.is_some(),
        "child_sm_id should be populated"
    );
    assert_eq!(
        active_state.fields[0].child_sm_name.as_deref(),
        Some("Child")
    );
}

#[test]
fn error_sm_type_in_packet_field_still_rejected() {
    // SM types should still be rejected in packet fields (wire context)
    let src = r#"
        state machine SM {
            state A state B [terminal] initial A
            transition A -> B { on go }
        }
        packet P { x: SM }
    "#;
    let result = check(src);
    assert!(
        result.is_err(),
        "SM type in packet field should still be rejected"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::TypeMismatch);
}

// ══════════════════════════════════════════════════════════════════════
// Finding 6: bool rejected as wire field type
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_bool_wire_field() {
    let result = check("packet P { flag: bool }");
    assert!(
        result.is_err(),
        "bool should be rejected as wire field type"
    );
    let err = result.unwrap_err();
    assert_eq!(err.kind, ErrorKind::TypeMismatch);
    assert!(
        err.msg.contains("semantic type"),
        "error message should mention 'semantic type': {}",
        err.msg
    );
}

#[test]
fn ok_bool_derived_field() {
    let result = check("packet P { x: u8, let flag: bool = x != 0 }");
    assert!(
        result.is_ok(),
        "bool should be valid in derived fields: {:?}",
        result.err()
    );
}

#[test]
fn error_bool_in_frame_wire_field() {
    let src = r#"
        frame F = match tag: u8 {
            0 => A { flag: bool },
            _ => B { data: bytes[remaining] },
        }
    "#;
    let result = check(src);
    assert!(
        result.is_err(),
        "bool in frame wire field should be rejected"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::TypeMismatch);
}

#[test]
fn ok_bool_in_frame_derived_field() {
    let src = r#"
        frame F = match tag: u8 {
            0 => A { x: u8, let flag: bool = x != 0 },
            _ => B { data: bytes[remaining] },
        }
    "#;
    let result = check(src);
    assert!(
        result.is_ok(),
        "bool in frame derived field should be valid: {:?}",
        result.err()
    );
}

// ══════════════════════════════════════════════════════════════════════
// Finding 7: Wildcard transition priority
// ══════════════════════════════════════════════════════════════════════

#[test]
fn ok_wildcard_with_concrete_override() {
    let src = r#"state machine S {
        state A state B state C [terminal] initial A
        transition A -> B { on next }
        transition A -> C { on error }
        transition B -> C { on done }
        transition * -> C { on error }
    }"#;
    // A has concrete "on error", wildcard also has "on error"
    // Concrete should take priority, not duplicate error
    let result = check(src);
    assert!(
        result.is_ok(),
        "concrete should override wildcard: {:?}",
        result.err()
    );
    let sem = result.unwrap();
    let sm = &sem.state_machines[0];
    // A -> C on error (concrete), B -> C on error (wildcard expansion)
    // A -> B on next, B -> C on done
    // Total: 4 transitions
    assert_eq!(
        sm.transitions.len(),
        4,
        "expected 4 transitions, got {:?}",
        sm.transitions
            .iter()
            .map(|t| format!(
                "{} -({})-> {}",
                t.src_state_name, t.event_name, t.dst_state_name
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn ok_wildcard_expands_to_uncovered_states() {
    let src = r#"state machine S {
        state A state B state C [terminal] initial A
        transition A -> B { on next }
        transition B -> C { on done }
        transition * -> C { on error }
    }"#;
    // Neither A nor B has concrete "on error", so wildcard expands to both
    let result = check(src);
    assert!(
        result.is_ok(),
        "wildcard should expand to uncovered states: {:?}",
        result.err()
    );
    let sem = result.unwrap();
    let sm = &sem.state_machines[0];
    // A -> B on next, B -> C on done, A -> C on error (wildcard), B -> C on error (wildcard)
    assert_eq!(sm.transitions.len(), 4);
}

// ══════════════════════════════════════════════════════════════════════
// Finding 8: Match exhaustiveness
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_frame_missing_wildcard() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x01 => B { x: u8 },
        }
    "#;
    let result = check(src);
    assert!(result.is_err(), "frame without wildcard should be rejected");
    let err = result.unwrap_err();
    assert_eq!(err.kind, ErrorKind::TypeMismatch);
    assert!(
        err.msg.contains("exhaustive") || err.msg.contains("wildcard"),
        "error message should mention exhaustiveness: {}",
        err.msg
    );
}

#[test]
fn ok_frame_with_wildcard() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            _ => B { data: bytes[remaining] },
        }
    "#;
    let result = check(src);
    assert!(result.is_ok(), "frame with wildcard should pass");
}

#[test]
fn error_capsule_missing_wildcard() {
    let src = r#"
        capsule C {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                1 => E { data: bytes[remaining] },
            },
        }
    "#;
    let result = check(src);
    assert!(
        result.is_err(),
        "capsule without wildcard should be rejected"
    );
    let err = result.unwrap_err();
    assert_eq!(err.kind, ErrorKind::TypeMismatch);
    assert!(
        err.msg.contains("exhaustive") || err.msg.contains("wildcard"),
        "error message should mention exhaustiveness: {}",
        err.msg
    );
}

#[test]
fn ok_capsule_with_wildcard() {
    let src = r#"
        capsule C {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let result = check(src);
    assert!(result.is_ok(), "capsule with wildcard should pass");
}

// ══════════════════════════════════════════════════════════════════════
// Finding 4: SmMissingAssignment (spec §3.9 rule 2a)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_sm_missing_dst_assignment() {
    let src = r#"state machine S {
        state A
        state B { x: u8 }
        state C [terminal]
        initial A
        transition A -> B { on go }
        transition B -> C { on done }
    }"#;
    // A->B: B.x has no default and no action assigns it
    let result = check(src);
    assert!(
        result.is_err(),
        "should fail: B.x has no default and no action assigns it"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmMissingAssignment);
}

#[test]
fn ok_sm_dst_field_assigned() {
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
    assert!(check(src).is_ok(), "B.x is assigned in action block");
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
    // B.x has default = 0, so no assignment needed
    assert!(
        check(src).is_ok(),
        "B.x has default value, no assignment needed"
    );
}

#[test]
fn ok_sm_delegate_auto_init() {
    let src = r#"state machine S {
        state A { c: u8 }
        state B [terminal]
        initial A
        transition A -> A {
            on ev(id: u8, e: u8)
            delegate src.c <- e
        }
        transition A -> B { on done }
    }"#;
    // Delegate auto-copies src to dst, no manual assignment needed
    assert!(check(src).is_ok(), "delegate auto-copies src to dst");
}

#[test]
fn ok_sm_terminal_no_fields() {
    let src = r#"state machine S {
        state A
        state Done [terminal]
        initial A
        transition A -> Done { on finish }
    }"#;
    // Done has no fields, nothing to assign
    assert!(
        check(src).is_ok(),
        "terminal state with no fields needs no assignment"
    );
}

#[test]
fn error_sm_missing_one_of_multiple_dst_fields() {
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
    assert!(result.is_err(), "should fail: B.y not assigned");
    let err = result.unwrap_err();
    assert_eq!(err.kind, ErrorKind::SmMissingAssignment);
    assert!(
        err.msg.contains("y"),
        "error should mention field 'y': {}",
        err.msg
    );
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
    assert!(check(src).is_ok(), "both B.x and B.y are assigned");
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
    // B.x assigned, B.y has default
    assert!(check(src).is_ok(), "B.x assigned, B.y has default");
}

#[test]
fn error_sm_self_transition_missing_assignment() {
    let src = r#"state machine S {
        state A { x: u8 }
        state B [terminal]
        initial A
        transition A -> A { on tick }
        transition A -> B { on done }
    }"#;
    // A->A: A.x has no default and no action assigns it (not a delegate)
    let result = check(src);
    assert!(
        result.is_err(),
        "self-transition without action should fail if fields have no default"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmMissingAssignment);
}

#[test]
fn ok_sm_self_transition_field_assigned() {
    let src = r#"state machine S {
        state A { x: u8 }
        state B [terminal]
        initial A
        transition A -> A {
            on tick
            action { dst.x = src.x + 1; }
        }
        transition A -> B { on done }
    }"#;
    assert!(
        check(src).is_ok(),
        "self-transition with assignment is valid"
    );
}
