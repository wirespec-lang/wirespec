// crates/wirespec-backend-tlaplus/src/lib.rs
pub mod emit;
pub mod tlc_result;

use wirespec_sema::ir::SemanticStateMachine;

/// Output of TLA+ generation: spec (.tla) and config (.cfg)
#[derive(Debug)]
pub struct TlaplusOutput {
    pub spec: String,
    pub config: String,
}

/// Generate TLA+ spec and config from a state machine.
///
/// `cli_bound` is an optional override for the model-checking bound.
/// Priority: cli_bound > @verify(bound=N) annotation > default (3).
pub fn generate_tlaplus(
    sm: &SemanticStateMachine,
    cli_bound: Option<u32>,
) -> Result<TlaplusOutput, String> {
    // Reject delegate SMs (Phase 1 limitation)
    for t in &sm.transitions {
        if t.delegate.is_some() {
            return Err(format!(
                "delegate is not yet supported in TLA+ generation (state machine '{}')",
                sm.name
            ));
        }
    }

    let bound = cli_bound.or(sm.verify_bound).unwrap_or(3);

    let spec = emit::emit_spec(sm, bound);
    let config = emit::emit_config(sm, bound);
    Ok(TlaplusOutput { spec, config })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirespec_sema::{ComplianceProfile, analyze};
    use wirespec_syntax::parse;

    // ── helper ──────────────────────────────────────────────────────────────

    /// Parse, analyze, and generate TLA+ for the first state machine in `src`.
    fn tla(src: &str, bound: u32) -> TlaplusOutput {
        let ast = parse(src).unwrap();
        let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
        generate_tlaplus(&sem.state_machines[0], Some(bound)).unwrap()
    }

    // ── smoke / reject ───────────────────────────────────────────────────────

    #[test]
    fn smoke_test_pathstate_generates_tla() {
        let src = r#"
            state machine PathState {
                state Init { path_id: u8 }
                state Active { path_id: u8, rtt: u64 = 0 }
                state Closed [terminal]
                initial Init
                transition Init -> Active {
                    on activate(id: u8)
                    action { dst.path_id = src.path_id; }
                }
                transition Active -> Closed { on close }
                transition * -> Closed { on abort }
            }
        "#;
        let ast = parse(src).unwrap();
        let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
        assert_eq!(sem.state_machines.len(), 1);
        let output = generate_tlaplus(&sem.state_machines[0], Some(3)).unwrap();

        assert!(output.spec.contains("---- MODULE PathState ----"));
        assert!(output.spec.contains("VARIABLE sm"));
        assert!(output.spec.contains("Init =="));
        assert!(output.spec.contains("Next =="));
        assert!(output.spec.contains("NoDeadlock"));
        assert!(output.spec.contains("===="));

        assert!(output.config.contains("INIT Init"));
        assert!(output.config.contains("NEXT Next"));
        assert!(output.config.contains("Bound = 3"));
        assert!(output.config.contains("INVARIANT TypeOK"));

        // Print for manual review
        eprintln!("=== Generated TLA+ ===");
        eprintln!("{}", output.spec);
        eprintln!("=== Generated CFG ===");
        eprintln!("{}", output.config);
    }

    #[test]
    fn reject_delegate_sm() {
        let src = r#"
            state machine Child {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on finish }
            }
            state machine Parent {
                state Running { child: Child }
                state Done [terminal]
                initial Running
                transition Running -> Running {
                    on forward(ev: u8)
                    delegate src.child <- ev
                }
                transition Running -> Done { on stop }
            }
        "#;
        let ast = parse(src).unwrap();
        let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
        let result = generate_tlaplus(&sem.state_machines[1], Some(3));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("delegate"));
    }

    // ── new tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_type_abstraction_bounded_nat() {
        // u8 field should cause BoundedNat to be defined and used in TypeOK
        let src = r#"
            state machine S {
                state A { count: u8 = 0 }
                state B [terminal]
                initial A
                transition A -> B { on done }
            }
        "#;
        let out = tla(src, 3);
        assert!(
            out.spec.contains("BoundedNat == 0..(Bound - 1)"),
            "should define BoundedNat for u8 fields"
        );
        assert!(
            out.spec.contains("sm.count \\in BoundedNat"),
            "count should appear in BoundedNat domain in TypeOK"
        );
    }

    #[test]
    fn test_transition_with_guard() {
        let src = r#"
            state machine S {
                state A { count: u8 = 0 }
                state B [terminal]
                initial A
                transition A -> A {
                    on tick
                    guard src.count < 10
                    action { dst.count = src.count + 1; }
                }
                transition A -> B { on done }
            }
        "#;
        let out = tla(src, 3);
        // Guard `src.count < 10` should translate to `(sm.count < 10)`
        assert!(
            out.spec.contains("(sm.count < 10)"),
            "guard should be translated to TLA+ expression: got\n{}",
            out.spec
        );
    }

    #[test]
    fn test_transition_without_guard() {
        let src = r#"
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on done }
            }
        "#;
        let out = tla(src, 2);
        assert!(
            out.spec.contains("sm.tag = \"A\""),
            "should guard on source state tag"
        );
        assert!(
            out.spec.contains("sm' = MkB"),
            "should transition to destination state MkB"
        );
    }

    #[test]
    fn test_terminal_state_no_outgoing() {
        let src = r#"
            state machine S {
                state A {}
                state Done [terminal]
                initial A
                transition A -> Done { on finish }
            }
        "#;
        let out = tla(src, 2);
        // Done is terminal — no transition should have Done as source state tag
        let tag_done = "sm.tag = \"Done\"";
        assert!(
            !out.spec.contains(tag_done),
            "terminal state Done should not appear as a transition source"
        );
    }

    #[test]
    fn test_cfg_generation() {
        let src = r#"
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
            }
        "#;
        let out = tla(src, 5);
        assert!(out.config.contains("INIT Init"));
        assert!(out.config.contains("NEXT Next"));
        assert!(out.config.contains("Bound = 5"));
        assert!(out.config.contains("INVARIANT TypeOK"));
        assert!(out.config.contains("INVARIANT NoDeadlock"));
    }

    #[test]
    fn test_mk_helper_correct_fields() {
        let src = r#"
            state machine S {
                state Active { path_id: u8, rtt: u64 = 0, cwnd: u64 = 0 }
                state Closed [terminal]
                initial Active
                transition Active -> Closed { on close }
            }
        "#;
        let out = tla(src, 2);
        // path_id has no default → parameter; rtt and cwnd have defaults → inlined
        assert!(
            out.spec.contains("MkActive(path_id_v)"),
            "MkActive should take only path_id as a parameter"
        );
        assert!(
            out.spec.contains("rtt |-> 0"),
            "rtt should use default value 0"
        );
        assert!(
            out.spec.contains("cwnd |-> 0"),
            "cwnd should use default value 0"
        );
    }

    #[test]
    fn test_default_values_in_mk_helper() {
        let src = r#"
            state machine S {
                state A { x: u8 = 42, y: u8 }
                state B [terminal]
                initial A
                transition A -> B { on done }
            }
        "#;
        let out = tla(src, 3);
        // y has no default → parameter; x has default 42 → inlined
        assert!(
            out.spec.contains("MkA(y_v)"),
            "MkA should only take y (x has a default)"
        );
        assert!(
            out.spec.contains("x |-> 42"),
            "x should use default value 42"
        );
    }

    #[test]
    fn test_wildcard_expansion() {
        let src = r#"
            state machine S {
                state A {}
                state B {}
                state Done [terminal]
                initial A
                transition A -> B { on go }
                transition B -> Done { on finish }
                transition * -> Done { on abort }
            }
        "#;
        let out = tla(src, 2);
        // Wildcard * -> Done on abort should expand to AbortFromA and AbortFromB
        assert!(
            out.spec.contains("AbortFromA"),
            "wildcard should expand to AbortFromA transition"
        );
        assert!(
            out.spec.contains("AbortFromB"),
            "wildcard should expand to AbortFromB transition"
        );
    }

    #[test]
    fn test_event_params_existential_quantification() {
        let src = r#"
            state machine S {
                state A { val: u8 = 0 }
                state B [terminal]
                initial A
                transition A -> A {
                    on set_value(v: u8)
                    action { dst.val = v; }
                }
                transition A -> B { on done }
            }
        "#;
        let out = tla(src, 3);
        // Event param v: u8 should produce \E v \in BoundedNat:
        assert!(
            out.spec.contains("\\E v \\in BoundedNat"),
            "event param should be existentially quantified over BoundedNat"
        );
    }

    #[test]
    fn test_no_verify_bound_uses_default() {
        // Verify that the bound passed to generate_tlaplus is reflected in cfg
        let src = r#"
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
            }
        "#;
        let out = tla(src, 3);
        assert!(out.config.contains("Bound = 3"), "bound should be 3 in cfg");

        let out2 = tla(src, 7);
        assert!(
            out2.config.contains("Bound = 7"),
            "bound should be 7 in cfg"
        );
    }

    // ── liveness / Phase 2a ────────────────────────────────────────────────

    #[test]
    fn test_all_reach_closed_generated() {
        let out = tla(
            r#"
            state machine S {
                state A {}
                state Done [terminal]
                initial A
                transition A -> Done { on go }
            }
        "#,
            2,
        );
        assert!(
            out.spec.contains("AllReachClosed"),
            "should generate AllReachClosed"
        );
        assert!(
            out.spec.contains("<>(sm.tag \\in TerminalStates)"),
            "AllReachClosed should use <> operator"
        );
        assert!(
            out.spec.contains("WF_sm(Next)"),
            "Spec should include WF for liveness"
        );
        assert!(
            out.config.contains("PROPERTY AllReachClosed"),
            "cfg should include AllReachClosed property"
        );
    }

    #[test]
    fn test_spec_includes_wf_with_terminal() {
        let out = tla(
            r#"
            state machine S {
                state A {}
                state Done [terminal]
                initial A
                transition A -> Done { on go }
            }
        "#,
            2,
        );
        assert!(
            out.spec.contains("WF_sm(Next)"),
            "Spec should include weak fairness"
        );
    }

    #[test]
    fn test_no_terminal_no_allreachclosed() {
        // SM with no terminal states should not generate AllReachClosed
        let out = tla(
            r#"
            state machine S {
                state A {}
                state B {}
                initial A
                transition A -> B { on go }
                transition B -> A { on back }
            }
        "#,
            2,
        );
        assert!(
            !out.spec.contains("AllReachClosed"),
            "no terminal = no AllReachClosed"
        );
    }

    #[test]
    fn test_multiple_states_union_fields() {
        // Two states with different fields — TypeOK union should cover all fields
        let src = r#"
            state machine S {
                state A { x: u8 }
                state B { y: u16 }
                state Done [terminal]
                initial A
                transition A -> B {
                    on go(val: u16)
                    action { dst.y = val; }
                }
                transition B -> Done { on finish }
            }
        "#;
        let out = tla(src, 2);
        // Both x and y should appear in TypeOK
        assert!(
            out.spec.contains("sm.x \\in BoundedNat"),
            "x (u8) should appear in TypeOK"
        );
        assert!(
            out.spec.contains("sm.y \\in BoundedNat"),
            "y (u16) should appear in TypeOK"
        );
        // Non-belonging fields use NullVal in Mk helpers
        assert!(
            out.spec.contains("y |-> NullVal") || out.spec.contains("x |-> NullVal"),
            "fields not belonging to a state should be NullVal"
        );
    }

    // ── guarded branches / Phase 2b ─────────────────────────────────────

    #[test]
    fn test_guarded_branches_tla_disjunction() {
        let out = tla(
            r#"
            state machine S {
                state A { count: u8 = 0 }
                state B [terminal]
                initial A
                transition A -> A {
                    on tick
                    guard src.count < 3
                    action { dst.count = src.count + 1; }
                }
                transition A -> B {
                    on tick
                    guard src.count >= 3
                }
            }
        "#,
            5,
        );
        // Should generate single Tick action with disjunction
        assert!(
            out.spec.contains("Tick =="),
            "should have single Tick action"
        );
        assert!(
            out.spec.contains("\\/"),
            "should have disjunction for guard branches"
        );
        assert!(out.spec.contains("sm.count < 3"), "should have first guard");
        assert!(
            out.spec.contains("sm.count >= 3"),
            "should have second guard"
        );
    }

    #[test]
    fn test_guard_exclusivity_invariant_generated() {
        let out = tla(
            r#"
            state machine S {
                state A { count: u8 = 0 }
                state B [terminal]
                initial A
                transition A -> A {
                    on tick
                    guard src.count < 3
                    action { dst.count = src.count + 1; }
                }
                transition A -> B {
                    on tick
                    guard src.count >= 3
                }
            }
        "#,
            5,
        );
        assert!(
            out.spec.contains("GuardExclusive"),
            "should generate guard exclusivity invariant"
        );
        assert!(
            out.config.contains("INVARIANT GuardExclusive"),
            "cfg should include exclusivity invariant"
        );
    }

    #[test]
    fn test_no_guard_group_no_exclusivity() {
        let out = tla(
            r#"
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
            }
        "#,
            2,
        );
        assert!(
            !out.spec.contains("GuardExclusive"),
            "no guarded groups = no exclusivity invariant"
        );
    }

    // ── verify declarations / Phase 3 ───────────────────────────────────

    /// Helper that uses the SM's own verify_bound and verify_declarations
    /// (no CLI bound override).
    fn tla_no_bound(src: &str) -> TlaplusOutput {
        let ast = parse(src).unwrap();
        let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
        generate_tlaplus(&sem.state_machines[0], None).unwrap()
    }

    #[test]
    fn test_verify_nodeadlock_controls_generation() {
        // SM with verify NoDeadlock -> generates NoDeadlock
        let out = tla_no_bound(
            r#"
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
                verify NoDeadlock
            }
        "#,
        );
        assert!(
            out.spec.contains("NoDeadlock"),
            "should generate NoDeadlock when verify NoDeadlock is present"
        );
        assert!(
            out.config.contains("INVARIANT NoDeadlock"),
            "cfg should include NoDeadlock invariant"
        );
        // Should NOT generate AllReachClosed (not requested)
        assert!(
            !out.spec.contains("AllReachClosed"),
            "should not generate AllReachClosed when not requested"
        );
        assert!(
            !out.config.contains("PROPERTY AllReachClosed"),
            "cfg should not include AllReachClosed property"
        );
    }

    #[test]
    fn test_verify_only_typeok_when_empty_verify() {
        // SM with NO verify declarations -> legacy behavior (generates everything)
        let out = tla_no_bound(
            r#"
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
            }
        "#,
        );
        // Legacy behavior: NoDeadlock and AllReachClosed
        assert!(
            out.spec.contains("NoDeadlock"),
            "legacy: generates NoDeadlock"
        );
        assert!(
            out.spec.contains("AllReachClosed"),
            "legacy: generates AllReachClosed"
        );
    }

    #[test]
    fn test_verify_nodeadlock_and_allreachclosed() {
        let out = tla_no_bound(
            r#"
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
                verify NoDeadlock
                verify AllReachClosed
            }
        "#,
        );
        assert!(
            out.spec.contains("NoDeadlock"),
            "should generate NoDeadlock"
        );
        assert!(
            out.spec.contains("AllReachClosed"),
            "should generate AllReachClosed"
        );
        assert!(
            out.spec.contains("WF_sm(Next)"),
            "should include WF for AllReachClosed liveness"
        );
        assert!(
            out.config.contains("INVARIANT NoDeadlock"),
            "cfg should include NoDeadlock"
        );
        assert!(
            out.config.contains("PROPERTY AllReachClosed"),
            "cfg should include AllReachClosed"
        );
    }

    #[test]
    fn test_verify_bound_annotation() {
        let out = tla_no_bound(
            r#"
            @verify(bound=5)
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
                verify NoDeadlock
            }
        "#,
        );
        assert!(
            out.config.contains("Bound = 5"),
            "should use bound=5 from annotation"
        );
    }

    #[test]
    fn test_verify_bound_cli_overrides_annotation() {
        let src = r#"
            @verify(bound=5)
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
                verify NoDeadlock
            }
        "#;
        let ast = parse(src).unwrap();
        let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
        let out = generate_tlaplus(&sem.state_machines[0], Some(10)).unwrap();
        assert!(
            out.config.contains("Bound = 10"),
            "CLI bound should override annotation bound"
        );
    }

    #[test]
    fn test_verify_default_bound_when_no_annotation() {
        let out = tla_no_bound(
            r#"
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
                verify NoDeadlock
            }
        "#,
        );
        assert!(
            out.config.contains("Bound = 3"),
            "should default to bound=3 when no annotation"
        );
    }

    #[test]
    fn test_user_property_formula_to_tla() {
        let out = tla_no_bound(
            r#"
            state machine S {
                state Active {}
                state Closing {}
                state Done [terminal]
                initial Active
                transition Active -> Closing { on close }
                transition Closing -> Done { on finish }
                verify property NeverBack: in_state(Closing) -> [] not in_state(Active)
            }
        "#,
        );
        // The property formula should be:
        // NeverBack == (sm.tag = "Closing" => [](~(sm.tag = "Active")))
        assert!(
            out.spec.contains("NeverBack =="),
            "should define NeverBack property: got\n{}",
            out.spec
        );
        assert!(
            out.spec.contains("sm.tag = \"Closing\""),
            "should reference Closing state tag"
        );
        assert!(
            out.spec.contains("sm.tag = \"Active\""),
            "should reference Active state tag"
        );
        assert!(
            out.config.contains("PROPERTY NeverBack"),
            "cfg should include NeverBack property"
        );
        // WF should be present since we have a liveness property
        assert!(
            out.spec.contains("WF_sm(Next)"),
            "should include WF for liveness property"
        );
    }

    #[test]
    fn test_verify_no_wf_when_only_safety() {
        // verify NoDeadlock only — no liveness, no WF
        let out = tla_no_bound(
            r#"
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
                verify NoDeadlock
            }
        "#,
        );
        assert!(
            !out.spec.contains("WF_sm(Next)"),
            "should NOT include WF when only safety properties: got\n{}",
            out.spec
        );
    }

    #[test]
    fn test_verify_leads_to_property() {
        let out = tla_no_bound(
            r#"
            state machine S {
                state A {}
                state B [terminal]
                initial A
                transition A -> B { on go }
                verify property Reach: in_state(A) ~> in_state(B)
            }
        "#,
        );
        assert!(
            out.spec.contains("Reach =="),
            "should define Reach property"
        );
        assert!(
            out.spec.contains("~>"),
            "should contain leads-to operator: got\n{}",
            out.spec
        );
        assert!(
            out.config.contains("PROPERTY Reach"),
            "cfg should include Reach property"
        );
    }

    // ── TLC result parser tests ────────────────────────────────────────────

    #[test]
    fn test_parse_tlc_pass() {
        use tlc_result::*;
        let output = r#"
Model checking completed. No error found.
42 states generated, 12 distinct states found.
        "#;
        match parse_tlc_output(output) {
            TlcResult::Pass {
                states_explored,
                distinct_states,
            } => {
                assert_eq!(states_explored, Some(42));
                assert_eq!(distinct_states, Some(12));
            }
            other => panic!("expected Pass, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tlc_invariant_violation() {
        use tlc_result::*;
        let output = r#"
Error: Invariant NoDeadlock is violated.
Error: The behavior up to this point is:
State 1: <Initial predicate>
/\ sm = [tag |-> "Init", path_id |-> 0, rtt |-> "@@null"]
State 2: <Action>
/\ sm = [tag |-> "Active", path_id |-> 0, rtt |-> 0]
State 3: Stuttering
        "#;
        match parse_tlc_output(output) {
            TlcResult::Fail {
                violated_property,
                counterexample,
            } => {
                assert_eq!(violated_property, "NoDeadlock");
                assert!(counterexample.len() >= 2);
                assert_eq!(counterexample[0].state_tag.as_deref(), Some("Init"));
            }
            other => panic!("expected Fail, got {:?}", other),
        }
    }

    #[test]
    fn test_nullval_hidden() {
        use tlc_result::*;
        let output = r#"
Error: Invariant NoDeadlock is violated.
State 1: <Initial predicate>
/\ sm = [tag |-> "Init", path_id |-> 0, rtt |-> "@@null", cwnd |-> "@@null"]
        "#;
        let result = parse_tlc_output(output);
        if let TlcResult::Fail { counterexample, .. } = result {
            let step = &counterexample[0];
            // NullVal fields should be filtered out
            assert!(
                !step.fields.iter().any(|(_, v)| v.contains("@@null")),
                "NullVal fields should be hidden: {:?}",
                step.fields
            );
        }
    }

    #[test]
    fn test_format_pass() {
        use tlc_result::*;
        let result = TlcResult::Pass {
            states_explored: Some(42),
            distinct_states: Some(12),
        };
        let formatted = format_result(&result, "PathState", 2);
        assert!(formatted.contains("PASS"));
        assert!(formatted.contains("PathState"));
        assert!(formatted.contains("42"));
    }

    #[test]
    fn test_format_fail() {
        use tlc_result::*;
        let result = TlcResult::Fail {
            violated_property: "NoDeadlock".to_string(),
            counterexample: vec![TlcStep {
                step_number: 1,
                state_tag: Some("Init".to_string()),
                fields: vec![("path_id".to_string(), "0".to_string())],
                is_stuttering: false,
            }],
        };
        let formatted = format_result(&result, "PathState", 2);
        assert!(formatted.contains("FAIL"));
        assert!(formatted.contains("NoDeadlock"));
        assert!(formatted.contains("Init"));
    }

    // ── E2E model-checking tests via tla-checker ──────────────────────────

    /// Parse wspec source, generate TLA+, then model-check it with tla-checker.
    fn check_tla(src: &str, bound: u32) -> (tla_checker::checker::CheckResult, TlaplusOutput) {
        use tla_checker::ast::Expr;
        use tla_checker::checker::{CheckerConfig, check};
        use tla_checker::config::{apply_config, parse_cfg};
        use tla_checker::parser::parse as parse_tla;

        let ast = parse(src).unwrap();
        let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
        assert!(!sem.state_machines.is_empty(), "no state machines found");
        let output = generate_tlaplus(&sem.state_machines[0], Some(bound)).unwrap();

        let mut spec = parse_tla(&output.spec)
            .unwrap_or_else(|e| panic!("TLA+ parse error: {:?}\n\nSpec:\n{}", e, output.spec));
        let cfg = parse_cfg(&output.config)
            .unwrap_or_else(|e| panic!("cfg parse error: {:?}\n\nConfig:\n{}", e, output.config));

        let mut domains = tla_checker::ast::Env::new();
        let mut checker_config = CheckerConfig {
            allow_deadlock: true,
            quiet: true,
            ..Default::default()
        };

        apply_config(
            &cfg,
            &mut spec,
            &mut domains,
            &mut checker_config,
            &[],
            &[],
            false,
        )
        .unwrap_or_else(|e| {
            panic!(
                "apply_config failed: {}\n\nSpec:\n{}\n\nConfig:\n{}",
                e, output.spec, output.config
            )
        });

        // tla-checker's check_eventually already wraps properties in <>
        // semantics, so we must unwrap Expr::Eventually here to avoid a
        // "temporal operator <> cannot be evaluated" error at runtime.
        spec.liveness_properties = spec
            .liveness_properties
            .into_iter()
            .map(|p| match p {
                Expr::Eventually(inner) => *inner,
                other => other,
            })
            .collect();

        let result = check(&spec, &domains, &checker_config);
        (result, output)
    }

    #[test]
    fn e2e_pathstate_nodeadlock_pass() {
        let (result, output) = check_tla(
            r#"
            @verify(bound = 2)
            state machine PathState {
                state Init { path_id: u8 }
                state Active { path_id: u8, rtt: u8 = 0 }
                state Closed [terminal]
                initial Init
                transition Init -> Active {
                    on activate(id: u8)
                    action { dst.path_id = src.path_id; }
                }
                transition Active -> Closed { on close }
                transition * -> Closed { on abort }
                verify NoDeadlock
            }
        "#,
            2,
        );
        eprintln!("TLA+:\n{}", output.spec);
        assert!(
            matches!(result, tla_checker::checker::CheckResult::Ok(_)),
            "expected PASS, got {:?}",
            result
        );
    }

    #[test]
    fn e2e_pathstate_allreachclosed_pass() {
        let (result, _) = check_tla(
            r#"
            @verify(bound = 2)
            state machine PathState {
                state Init { path_id: u8 }
                state Active { path_id: u8, rtt: u8 = 0 }
                state Closed [terminal]
                initial Init
                transition Init -> Active {
                    on activate(id: u8)
                    action { dst.path_id = src.path_id; }
                }
                transition Active -> Closed { on close }
                transition * -> Closed { on abort }
                verify AllReachClosed
            }
        "#,
            2,
        );
        assert!(
            matches!(result, tla_checker::checker::CheckResult::Ok(_)),
            "expected PASS for AllReachClosed, got {:?}",
            result
        );
    }

    #[test]
    fn e2e_safety_property_pass() {
        let (result, _) = check_tla(
            r#"
            @verify(bound = 2)
            state machine S {
                state A {}
                state B {}
                state Done [terminal]
                initial A
                transition A -> B { on go }
                transition B -> Done { on finish }
                transition * -> Done { on abort }
                verify property BBeforeDone:
                    in_state(B) -> not in_state(A)
            }
        "#,
            2,
        );
        assert!(
            matches!(result, tla_checker::checker::CheckResult::Ok(_)),
            "expected PASS, got {:?}",
            result
        );
    }

    #[test]
    fn e2e_liveness_leads_to_pass() {
        let (result, _) = check_tla(
            r#"
            @verify(bound = 2)
            state machine S {
                state A {}
                state B {}
                state Done [terminal]
                initial A
                transition A -> B { on go }
                transition B -> Done { on finish }
                verify AllReachClosed
            }
        "#,
            2,
        );
        assert!(
            matches!(result, tla_checker::checker::CheckResult::Ok(_)),
            "expected PASS for liveness, got {:?}",
            result
        );
    }

    #[test]
    fn e2e_exclusive_guards_pass() {
        let (result, _) = check_tla(
            r#"
            @verify(bound = 5)
            state machine RetryMachine {
                state Trying { count: u8 = 0 }
                state Done [terminal]
                initial Trying
                transition Trying -> Trying {
                    on retry
                    guard src.count < 3
                    action { dst.count = src.count + 1; }
                }
                transition Trying -> Done {
                    on retry
                    guard src.count >= 3
                }
                transition * -> Done { on cancel }
                verify NoDeadlock
            }
        "#,
            5,
        );
        assert!(
            matches!(result, tla_checker::checker::CheckResult::Ok(_)),
            "expected PASS for exclusive guards, got {:?}",
            result
        );
    }

    #[test]
    fn e2e_overlapping_guards_fail() {
        let (result, output) = check_tla(
            r#"
            @verify(bound = 5)
            state machine OverlapMachine {
                state Active { level: u8 = 0 }
                state Done [terminal]
                initial Active
                transition Active -> Active {
                    on adjust
                    guard src.level < 3
                    action { dst.level = src.level + 1; }
                }
                transition Active -> Done {
                    on adjust
                    guard src.level < 5
                }
                transition * -> Done { on quit }
                verify NoDeadlock
            }
        "#,
            5,
        );
        eprintln!("TLA+:\n{}", output.spec);
        // GuardExclusive invariant should be violated (level=0,1,2 both guards true)
        assert!(
            matches!(
                result,
                tla_checker::checker::CheckResult::InvariantViolation(..)
            ),
            "expected invariant violation for overlapping guards, got {:?}",
            result
        );
    }

    #[test]
    fn e2e_deadlock_fail_no_escape() {
        let (result, output) = check_tla(
            r#"
            @verify(bound = 3)
            state machine DeadlockMachine {
                state Running { counter: u8 = 0 }
                state Done [terminal]
                initial Running
                transition Running -> Running {
                    on tick
                    guard src.counter < 2
                    action { dst.counter = src.counter + 1; }
                }
                transition Running -> Done {
                    on finish
                    guard src.counter >= 100
                }
                verify NoDeadlock
            }
        "#,
            3,
        );
        eprintln!("TLA+:\n{}", output.spec);
        // counter reaches 2, tick fails (2 < 2), finish fails (2 >= 100)
        // NoDeadlock invariant: Running is not terminal and ENABLED(Next) is false
        assert!(
            matches!(
                result,
                tla_checker::checker::CheckResult::InvariantViolation(..)
                    | tla_checker::checker::CheckResult::Deadlock(..)
            ),
            "expected deadlock/invariant violation, got {:?}",
            result
        );
    }

    #[test]
    fn e2e_unreachable_terminal_fail() {
        let (result, output) = check_tla(
            r#"
            @verify(bound = 2)
            state machine LoopMachine {
                state A { val: u8 = 0 }
                state B { val: u8 = 0 }
                state Done [terminal]
                initial A
                transition A -> B { on go action { dst.val = src.val; } }
                transition B -> A { on back action { dst.val = src.val; } }
                verify AllReachClosed
            }
        "#,
            2,
        );
        eprintln!("TLA+:\n{}", output.spec);
        // A -> B -> A -> B ... forever, never reaches Done
        // AllReachClosed (liveness) should be violated
        assert!(
            matches!(
                result,
                tla_checker::checker::CheckResult::LivenessViolation(..)
            ),
            "expected liveness violation, got {:?}",
            result
        );
    }
}
