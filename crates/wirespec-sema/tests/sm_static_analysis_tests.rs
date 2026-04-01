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
