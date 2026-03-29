//! Semantic validation tests for Tasks 1-5, 7.
//!
//! Each test exercises a specific validation rule added to the analyzer.

use wirespec_sema::ComplianceProfile;
use wirespec_sema::analyze;
use wirespec_sema::error::ErrorKind;
use wirespec_syntax::parse;

fn default_profile() -> ComplianceProfile {
    ComplianceProfile::default()
}

fn check(src: &str) -> Result<wirespec_sema::SemanticModule, wirespec_sema::error::SemaError> {
    analyze(&parse(src).unwrap(), default_profile(), &Default::default())
}

// ══════════════════════════════════════════════════════════════════════
// Task 1: SM duplicate transition detection
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_sm_duplicate_transition() {
    let src = r#"state machine S {
        state A
        state B [terminal]
        initial A
        transition A -> B { on go }
        transition A -> B { on go }
    }"#;
    let result = check(src);
    assert!(result.is_err(), "should fail with duplicate transition");
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmDuplicateTransition);
}

#[test]
fn ok_sm_different_events() {
    let src = r#"state machine S {
        state A
        state B [terminal]
        initial A
        transition A -> B { on go }
        transition A -> B { on stop }
    }"#;
    assert!(check(src).is_ok(), "different events should be allowed");
}

#[test]
fn ok_sm_same_event_different_src() {
    let src = r#"state machine S {
        state A
        state B
        state C [terminal]
        initial A
        transition A -> B { on go }
        transition B -> C { on go }
    }"#;
    assert!(
        check(src).is_ok(),
        "same event from different states should be allowed"
    );
}

// ══════════════════════════════════════════════════════════════════════
// Task 2: SM delegate only on self-transitions
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_sm_delegate_not_self() {
    let src = r#"state machine S {
        state A { c: u8 }
        state B [terminal]
        initial A
        transition A -> B { on ev(id: u8, e: u8) delegate src.c <- e }
    }"#;
    let result = check(src);
    assert!(
        result.is_err(),
        "delegate on non-self-transition should fail"
    );
    assert_eq!(
        result.unwrap_err().kind,
        ErrorKind::SmDelegateNotSelfTransition
    );
}

#[test]
fn ok_sm_delegate_self() {
    let src = r#"state machine S {
        state A { c: u8 }
        state B [terminal]
        initial A
        transition A -> A { on ev(id: u8, e: u8) delegate src.c <- e }
        transition A -> B { on done }
    }"#;
    assert!(
        check(src).is_ok(),
        "delegate on self-transition should be allowed"
    );
}

// ══════════════════════════════════════════════════════════════════════
// Task 3: SM delegate + action mutual exclusion
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_sm_delegate_with_action() {
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
    assert!(
        result.is_err(),
        "delegate + action should be mutually exclusive"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmDelegateWithAction);
}

// ══════════════════════════════════════════════════════════════════════
// Task 4: SM invalid initial state
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_sm_invalid_initial() {
    let src = r#"state machine S {
        state A
        state B [terminal]
        initial NonExistent
        transition A -> B { on go }
    }"#;
    let result = check(src);
    assert!(
        result.is_err(),
        "initial state that doesn't exist should fail"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmInvalidInitial);
}

#[test]
fn error_sm_missing_initial() {
    let src = r#"state machine S {
        state A
        state B [terminal]
        transition A -> B { on go }
    }"#;
    let result = check(src);
    assert!(
        result.is_err(),
        "missing initial state declaration should fail"
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::SmInvalidInitial);
}

// ══════════════════════════════════════════════════════════════════════
// Task 5: Recursive type alias detection
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_recursive_type_alias() {
    let src = "type A = B\ntype B = C\ntype C = A\npacket P { x: A }";
    let result = check(src);
    assert!(result.is_err(), "recursive alias chain should fail");
}

#[test]
fn error_direct_cyclic_alias() {
    let src = "type A = A\npacket P { x: A }";
    let result = check(src);
    assert!(result.is_err(), "direct self-referencing alias should fail");
}

// ══════════════════════════════════════════════════════════════════════
// Task 7: bytes[length_or_remaining:] optional check
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_lor_non_optional() {
    let src = "packet P { len: u16, data: bytes[length_or_remaining: len] }";
    let result = check(src);
    assert!(
        result.is_err(),
        "LOR referencing non-optional field should fail"
    );
    assert_eq!(
        result.unwrap_err().kind,
        ErrorKind::InvalidLengthOrRemaining
    );
}

#[test]
fn ok_lor_optional() {
    let src =
        "packet P { flags: u8, len: if flags & 1 { u16 }, data: bytes[length_or_remaining: len] }";
    let result = check(src);
    assert!(
        result.is_ok(),
        "LOR referencing optional field should pass: {:?}",
        result.err()
    );
}

// ══════════════════════════════════════════════════════════════════════
// SM exhaustiveness: non-terminal states must have outgoing transitions
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_sm_non_terminal_no_transitions() {
    let src = r#"state machine S {
        state A
        state B
        state C [terminal]
        initial A
        transition A -> C { on done }
    }"#;
    // B is non-terminal but has no outgoing transitions
    assert!(check(src).is_err());
}

#[test]
fn ok_sm_wildcard_covers_all() {
    let src = r#"state machine S {
        state A
        state B
        state C [terminal]
        initial A
        transition A -> C { on done }
        transition * -> C { on abort }
    }"#;
    // Wildcard covers B
    assert!(check(src).is_ok());
}

#[test]
fn ok_sm_all_non_terminal_have_transitions() {
    let src = r#"state machine S {
        state A
        state B
        state C [terminal]
        initial A
        transition A -> B { on next }
        transition B -> C { on done }
    }"#;
    assert!(check(src).is_ok());
}
