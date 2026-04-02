// crates/wirespec-backend-tlaplus/src/lib.rs
pub mod emit;

use wirespec_sema::ir::SemanticStateMachine;

/// Output of TLA+ generation: spec (.tla) and config (.cfg)
#[derive(Debug)]
pub struct TlaplusOutput {
    pub spec: String,
    pub config: String,
}

/// Generate TLA+ spec and config from a state machine.
/// `bound` controls the finite domain size for model checking.
pub fn generate_tlaplus(sm: &SemanticStateMachine, bound: u32) -> Result<TlaplusOutput, String> {
    // Reject delegate SMs (Phase 1 limitation)
    for t in &sm.transitions {
        if t.delegate.is_some() {
            return Err(format!(
                "delegate is not yet supported in TLA+ generation (state machine '{}')",
                sm.name
            ));
        }
    }

    let spec = emit::emit_spec(sm, bound);
    let config = emit::emit_config(sm, bound);
    Ok(TlaplusOutput { spec, config })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirespec_sema::{ComplianceProfile, analyze};
    use wirespec_syntax::parse;

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
        let output = generate_tlaplus(&sem.state_machines[0], 3).unwrap();

        assert!(output.spec.contains("---- MODULE PathState ----"));
        assert!(output.spec.contains("VARIABLE sm"));
        assert!(output.spec.contains("Init =="));
        assert!(output.spec.contains("Next =="));
        assert!(output.spec.contains("NoDeadlock"));
        assert!(output.spec.contains("===="));

        assert!(output.config.contains("SPECIFICATION Spec"));
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
        let result = generate_tlaplus(&sem.state_machines[1], 3);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("delegate"));
    }
}
