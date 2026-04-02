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

// ── S2: TerminalIrreversibility ──

#[test]
fn error_sm_terminal_has_outgoing_transition() {
    expect_error(
        r#"
        state machine S {
            state Active {}
            state Closed [terminal]
            initial Active
            transition Active -> Closed { on close }
            transition Closed -> Active { on restart }
        }
    "#,
        ErrorKind::SmTerminalHasOutgoing,
    );
}

#[test]
fn ok_sm_transition_to_terminal() {
    expect_ok(
        r#"
        state machine S {
            state Active {}
            state Closed [terminal]
            initial Active
            transition Active -> Closed { on close }
        }
    "#,
    );
}

#[test]
fn ok_sm_wildcard_skips_terminal() {
    expect_ok(
        r#"
        state machine S {
            state A {}
            state B {}
            state Done [terminal]
            initial A
            transition A -> B { on go }
            transition B -> Done { on finish }
            transition * -> Done { on abort }
        }
    "#,
    );
}

#[test]
fn error_sm_terminal_has_outgoing_with_guard() {
    expect_error(
        r#"
        state machine S {
            state Active { count: u8 = 0 }
            state Closed [terminal]
            initial Active
            transition Active -> Closed { on close }
            transition Closed -> Active {
                on restart
                guard src.count < 10
            }
        }
    "#,
        ErrorKind::SmTerminalHasOutgoing,
    );
}

// ── S4: DelegateAcyclicity ──

#[test]
fn error_sm_delegate_direct_cycle() {
    expect_error(
        r#"
        state machine A {
            state Running { child: B }
            state Done [terminal]
            initial Running
            transition Running -> Running {
                on forward(ev: u8)
                delegate src.child <- ev
            }
            transition Running -> Done { on stop }
        }
        state machine B {
            state Running { child: A }
            state Done [terminal]
            initial Running
            transition Running -> Running {
                on forward(ev: u8)
                delegate src.child <- ev
            }
            transition Running -> Done { on stop }
        }
    "#,
        ErrorKind::CyclicDependency,
    );
}

#[test]
fn error_sm_delegate_indirect_cycle() {
    expect_error(
        r#"
        state machine X {
            state S { child: Y }
            state Done [terminal]
            initial S
            transition S -> Done { on stop }
        }
        state machine Y {
            state S { child: Z }
            state Done [terminal]
            initial S
            transition S -> Done { on stop }
        }
        state machine Z {
            state S { child: X }
            state Done [terminal]
            initial S
            transition S -> Done { on stop }
        }
    "#,
        ErrorKind::CyclicDependency,
    );
}

#[test]
fn ok_sm_delegate_chain_no_cycle() {
    expect_ok(
        r#"
        state machine A {
            state S { child: B }
            state Done [terminal]
            initial S
            transition S -> Done { on stop }
        }
        state machine B {
            state S { child: C }
            state Done [terminal]
            initial S
            transition S -> Done { on stop }
        }
        state machine C {
            state S {}
            state Done [terminal]
            initial S
            transition S -> Done { on stop }
        }
    "#,
    );
}

#[test]
fn ok_sm_no_delegate() {
    expect_ok(
        r#"
        state machine S {
            state A {}
            state B [terminal]
            initial A
            transition A -> B { on go }
        }
    "#,
    );
}

#[test]
fn ok_sm_delegate_diamond() {
    expect_ok(
        r#"
        state machine A {
            state S { b: B, c: C }
            state Done [terminal]
            initial S
            transition S -> Done { on stop }
        }
        state machine B {
            state S { d: D }
            state Done [terminal]
            initial S
            transition S -> Done { on stop }
        }
        state machine C {
            state S { d: D }
            state Done [terminal]
            initial S
            transition S -> Done { on stop }
        }
        state machine D {
            state S {}
            state Done [terminal]
            initial S
            transition S -> Done { on stop }
        }
    "#,
    );
}

// ── Warning Infrastructure ──

fn get_warnings(src: &str) -> Vec<wirespec_sema::ir::SemaWarning> {
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    sem.warnings
}

#[test]
fn ok_module_with_no_warnings() {
    let warnings = get_warnings(
        r#"
        state machine S {
            state A {}
            state B [terminal]
            initial A
            transition A -> B { on go }
        }
    "#,
    );
    assert!(
        warnings.is_empty(),
        "expected no warnings, got: {:?}",
        warnings
    );
}

use wirespec_sema::ir::SemaWarningKind;

fn get_warning_kinds(src: &str) -> Vec<SemaWarningKind> {
    get_warnings(src).iter().map(|w| w.kind).collect()
}

// ── S5: StructuralReachability ──

#[test]
fn warning_sm_unreachable_terminal() {
    let kinds = get_warning_kinds(
        r#"
        state machine S {
            state A {}
            state B {}
            state Done [terminal]
            initial A
            transition A -> B { on go }
            transition B -> A { on back }
        }
    "#,
    );
    assert!(
        kinds.contains(&SemaWarningKind::SmUnreachableTerminal),
        "expected SmUnreachableTerminal, got: {:?}",
        kinds
    );
}

#[test]
fn warning_sm_unreachable_from_initial() {
    let kinds = get_warning_kinds(
        r#"
        state machine S {
            state A {}
            state B {}
            state Orphan {}
            state Done [terminal]
            initial A
            transition A -> B { on go }
            transition B -> Done { on finish }
            transition Orphan -> Done { on finish }
        }
    "#,
    );
    assert!(
        kinds.contains(&SemaWarningKind::SmUnreachableFromInitial),
        "expected SmUnreachableFromInitial, got: {:?}",
        kinds
    );
}

#[test]
fn ok_sm_all_states_reach_terminal() {
    let warnings = get_warnings(
        r#"
        state machine S {
            state A {}
            state B {}
            state Done [terminal]
            initial A
            transition A -> B { on go }
            transition B -> Done { on finish }
        }
    "#,
    );
    assert!(
        warnings.is_empty(),
        "expected no warnings, got: {:?}",
        warnings
    );
}

#[test]
fn ok_sm_linear_to_terminal() {
    let warnings = get_warnings(
        r#"
        state machine S {
            state Init {}
            state Active {}
            state Closing {}
            state Closed [terminal]
            initial Init
            transition Init -> Active { on start }
            transition Active -> Closing { on close }
            transition Closing -> Closed { on done }
        }
    "#,
    );
    assert!(warnings.is_empty());
}

#[test]
fn ok_sm_wildcard_ensures_reachability() {
    let warnings = get_warnings(
        r#"
        state machine S {
            state A {}
            state B {}
            state Done [terminal]
            initial A
            transition A -> B { on go }
            transition * -> Done { on abort }
        }
    "#,
    );
    assert!(warnings.is_empty());
}

#[test]
fn warning_sm_cycle_without_exit() {
    let kinds = get_warning_kinds(
        r#"
        state machine S {
            state A {}
            state B {}
            state C {}
            state Done [terminal]
            initial A
            transition A -> B { on x }
            transition B -> C { on y }
            transition C -> A { on z }
        }
    "#,
    );
    assert!(
        kinds.contains(&SemaWarningKind::SmUnreachableTerminal),
        "cycle should warn about unreachable terminal, got: {:?}",
        kinds
    );
}
