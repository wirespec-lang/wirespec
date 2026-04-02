// crates/wirespec-backend-tlaplus/src/emit.rs

use wirespec_sema::expr::*;
use wirespec_sema::ir::*;
use wirespec_sema::types::*;

/// Convert a name to PascalCase for TLA+ action names.
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

/// Emit the complete .tla spec.
pub fn emit_spec(sm: &SemanticStateMachine, _bound: u32) -> String {
    let name = &sm.name;
    let mut out = String::new();

    // Module header
    out.push_str(&format!("---- MODULE {} ----\n", name));
    out.push_str("EXTENDS Integers, TLC\n\n");

    // Bound constant
    out.push_str("\\* Value domains (bounded for model checking)\n");
    out.push_str("CONSTANT Bound\n");
    out.push_str("ASSUME Bound \\in Nat /\\ Bound > 0\n\n");

    // Collect all fields across all states (union)
    let all_fields = collect_all_fields(sm);

    // Value domain definitions
    emit_value_domains(&mut out, &all_fields);

    // Null marker
    out.push_str("NullVal == \"@@null\"\n\n");

    // State tags
    let state_names: Vec<&str> = sm.states.iter().map(|s| s.name.as_str()).collect();
    let terminal_names: Vec<&str> = sm
        .states
        .iter()
        .filter(|s| s.is_terminal)
        .map(|s| s.name.as_str())
        .collect();
    out.push_str(&format!(
        "StateTag == {{{}}}\n",
        state_names
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push_str(&format!(
        "TerminalStates == {{{}}}\n\n",
        terminal_names
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(", ")
    ));

    // State variable
    out.push_str("VARIABLE sm\n\n");

    // TypeOK invariant
    emit_type_ok(&mut out, &all_fields);

    // Mk helpers
    emit_mk_helpers(&mut out, sm, &all_fields);

    // Init predicate
    emit_init(&mut out, sm);

    // Transition actions
    emit_transitions(&mut out, sm);

    // Next
    let action_names = compute_action_names(sm);
    emit_next(&mut out, &action_names);

    // Spec
    out.push_str("\\* Specification\n");
    out.push_str("Spec == Init /\\ [][Next]_sm\n\n");

    // NoDeadlock property
    out.push_str("\\* NoDeadlock: terminal states or transitions enabled\n");
    out.push_str("NoDeadlock == sm.tag \\in TerminalStates \\/ ENABLED(Next)\n\n");

    // Module footer
    out.push_str("====\n");

    out
}

/// Emit the .cfg file.
pub fn emit_config(sm: &SemanticStateMachine, bound: u32) -> String {
    let _ = sm;
    let mut out = String::new();
    out.push_str("SPECIFICATION Spec\n");
    out.push_str(&format!("CONSTANT Bound = {}\n\n", bound));
    out.push_str("INVARIANT TypeOK\n");
    out.push_str("INVARIANT NoDeadlock\n");
    out
}

/// Collect all unique (field_name, field_type) pairs across all states.
fn collect_all_fields(sm: &SemanticStateMachine) -> Vec<(String, SemanticType)> {
    let mut fields: Vec<(String, SemanticType)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for state in &sm.states {
        for field in &state.fields {
            if seen.insert(field.name.clone()) {
                fields.push((field.name.clone(), field.ty.clone()));
            }
        }
    }
    fields
}

/// Emit value domain definitions based on field types.
fn emit_value_domains(out: &mut String, fields: &[(String, SemanticType)]) {
    let mut has_bounded_nat = false;
    let mut has_bounded_int = false;
    let mut sym_bytes_sizes: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();

    for (_, ty) in fields {
        match ty {
            SemanticType::Primitive { wire, .. } => match wire {
                PrimitiveWireType::U8
                | PrimitiveWireType::U16
                | PrimitiveWireType::U24
                | PrimitiveWireType::U32
                | PrimitiveWireType::U64 => {
                    has_bounded_nat = true;
                }
                PrimitiveWireType::I8
                | PrimitiveWireType::I16
                | PrimitiveWireType::I32
                | PrimitiveWireType::I64 => {
                    has_bounded_int = true;
                }
                PrimitiveWireType::Bool | PrimitiveWireType::Bit => {}
            },
            SemanticType::VarIntRef { .. } => {
                has_bounded_nat = true;
            }
            SemanticType::Bytes { fixed_size, .. } => {
                let size = fixed_size.unwrap_or(8);
                sym_bytes_sizes.insert(size);
            }
            SemanticType::Bits { .. }
            | SemanticType::Array { .. }
            | SemanticType::PacketRef { .. }
            | SemanticType::EnumRef { .. }
            | SemanticType::FrameRef { .. }
            | SemanticType::CapsuleRef { .. } => {
                has_bounded_nat = true; // fallback
            }
        }
    }

    if has_bounded_nat {
        out.push_str("BoundedNat == 0..(Bound - 1)\n");
    }
    if has_bounded_int {
        out.push_str("BoundedInt == -(Bound - 1)..(Bound - 1)\n");
    }
    for size in &sym_bytes_sizes {
        out.push_str(&format!(
            "SymBytes{} == {{\"sym_bytes{}_\" \\o ToString(i) : i \\in 0..(Bound - 1)}}\n",
            size, size
        ));
    }
    if has_bounded_nat || has_bounded_int || !sym_bytes_sizes.is_empty() {
        out.push('\n');
    }
}

/// Get the TLA+ value domain for a type.
fn type_to_tla_domain(ty: &SemanticType) -> String {
    match ty {
        SemanticType::Primitive { wire, .. } => match wire {
            PrimitiveWireType::Bool => "BOOLEAN".to_string(),
            PrimitiveWireType::I8
            | PrimitiveWireType::I16
            | PrimitiveWireType::I32
            | PrimitiveWireType::I64 => "BoundedInt".to_string(),
            _ => "BoundedNat".to_string(),
        },
        SemanticType::VarIntRef { .. } => "BoundedNat".to_string(),
        SemanticType::Bytes { fixed_size, .. } => {
            let size = fixed_size.unwrap_or(8);
            format!("SymBytes{}", size)
        }
        _ => "BoundedNat".to_string(),
    }
}

/// Emit TypeOK invariant.
fn emit_type_ok(out: &mut String, fields: &[(String, SemanticType)]) {
    out.push_str("TypeOK ==\n");
    out.push_str("    /\\ sm.tag \\in StateTag\n");
    for (name, ty) in fields {
        let domain = type_to_tla_domain(ty);
        out.push_str(&format!(
            "    /\\ sm.{} \\in {} \\cup {{NullVal}}\n",
            name, domain
        ));
    }
    out.push('\n');
}

/// Emit Mk helper functions for each state.
fn emit_mk_helpers(
    out: &mut String,
    sm: &SemanticStateMachine,
    all_fields: &[(String, SemanticType)],
) {
    for state in &sm.states {
        let mk_name = format!("Mk{}", state.name);

        // Parameters: fields of this state without default values
        let params: Vec<String> = state
            .fields
            .iter()
            .filter(|f| f.default_value.is_none())
            .map(|f| format!("{}_v", f.name))
            .collect();

        let param_str = if params.is_empty() {
            String::new()
        } else {
            format!("({})", params.join(", "))
        };

        out.push_str(&format!("{}{} ==\n", mk_name, param_str));

        // Build record with all fields
        let mut record_parts: Vec<String> = vec![format!("tag |-> \"{}\"", state.name)];
        for (fname, _) in all_fields {
            let state_field = state.fields.iter().find(|f| &f.name == fname);
            let value = if let Some(sf) = state_field {
                if let Some(ref default) = sf.default_value {
                    literal_to_tla(default)
                } else {
                    format!("{}_v", fname)
                }
            } else {
                "NullVal".to_string()
            };
            record_parts.push(format!("{} |-> {}", fname, value));
        }

        out.push_str(&format!("    [{}]\n\n", record_parts.join(", ")));
    }
}

/// Convert a literal to TLA+ value.
fn literal_to_tla(lit: &SemanticLiteral) -> String {
    match lit {
        SemanticLiteral::Int(v) => v.to_string(),
        SemanticLiteral::Bool(true) => "TRUE".to_string(),
        SemanticLiteral::Bool(false) => "FALSE".to_string(),
        SemanticLiteral::String(s) => format!("\"{}\"", s),
        SemanticLiteral::Null => "NullVal".to_string(),
    }
}

/// Emit Init predicate.
fn emit_init(out: &mut String, sm: &SemanticStateMachine) {
    // Find initial state by ID
    let initial_state = sm
        .states
        .iter()
        .find(|s| s.state_id == sm.initial_state_id)
        .expect("initial state not found");

    out.push_str("Init ==\n");

    // Fields without defaults need existential quantification
    let unbound_fields: Vec<&SemanticStateField> = initial_state
        .fields
        .iter()
        .filter(|f| f.default_value.is_none())
        .collect();

    if unbound_fields.is_empty() {
        out.push_str(&format!("    sm = Mk{}\n\n", initial_state.name));
    } else {
        // \E field1 \in Domain1, field2 \in Domain2: sm = Mk...(field1, field2)
        let quantifiers: Vec<String> = unbound_fields
            .iter()
            .map(|f| format!("{}_v \\in {}", f.name, type_to_tla_domain(&f.ty)))
            .collect();
        let args: Vec<String> = initial_state
            .fields
            .iter()
            .filter(|f| f.default_value.is_none())
            .map(|f| format!("{}_v", f.name))
            .collect();
        out.push_str(&format!("    \\E {}:\n", quantifiers.join(", ")));
        out.push_str(&format!(
            "        sm = Mk{}({})\n\n",
            initial_state.name,
            args.join(", ")
        ));
    }
}

/// Compute unique action names for each transition. This must be consistent
/// between emit_transitions and emit_next.
fn compute_action_names(sm: &SemanticStateMachine) -> Vec<String> {
    let mut action_names: Vec<String> = Vec::new();

    for (i, trans) in sm.transitions.iter().enumerate() {
        let event_pascal = to_pascal_case(&trans.event_name);

        // Check if multiple transitions have same event name
        let same_event_count = sm
            .transitions
            .iter()
            .filter(|t| t.event_name == trans.event_name)
            .count();

        let action_name = if same_event_count > 1 {
            format!("{}From{}", event_pascal, trans.src_state_name)
        } else {
            event_pascal.clone()
        };

        // Deduplicate if collision
        let final_name = if action_names.contains(&action_name) {
            format!("{}_{}", action_name, i)
        } else {
            action_name
        };
        action_names.push(final_name);
    }

    action_names
}

/// Emit transition actions.
fn emit_transitions(out: &mut String, sm: &SemanticStateMachine) {
    let action_names = compute_action_names(sm);

    for (i, trans) in sm.transitions.iter().enumerate() {
        let final_name = &action_names[i];

        // Comment
        out.push_str(&format!(
            "\\* transition {} -> {} {{ on {} }}\n",
            trans.src_state_name, trans.dst_state_name, trans.event_name
        ));

        out.push_str(&format!("{} ==\n", final_name));

        // Source state guard
        out.push_str(&format!("    /\\ sm.tag = \"{}\"\n", trans.src_state_name));

        // Event params -> existential quantification
        let event = sm.events.iter().find(|e| e.event_id == trans.event_id);
        let params = event.map(|e| &e.params).cloned().unwrap_or_default();

        let mut indent = "    ";
        if !params.is_empty() {
            let quantifiers: Vec<String> = params
                .iter()
                .map(|p| format!("{} \\in {}", p.name, type_to_tla_domain(&p.ty)))
                .collect();
            out.push_str(&format!("    /\\ \\E {}:\n", quantifiers.join(", ")));
            indent = "        ";
        }

        // Guard
        if let Some(ref guard) = trans.guard {
            let guard_str = expr_to_tla(guard);
            out.push_str(&format!("{}/\\ {}\n", indent, guard_str));
        }

        // Destination state construction
        let dst_state = sm.states.iter().find(|s| s.name == trans.dst_state_name);
        if let Some(dst) = dst_state {
            let mk_args = build_mk_args(dst, &trans.actions);
            if mk_args.is_empty() {
                out.push_str(&format!("{}/\\ sm' = Mk{}\n", indent, dst.name));
            } else {
                out.push_str(&format!(
                    "{}/\\ sm' = Mk{}({})\n",
                    indent,
                    dst.name,
                    mk_args.join(", ")
                ));
            }
        }

        out.push('\n');
    }
}

/// Build Mk helper arguments from transition actions.
fn build_mk_args(dst_state: &SemanticState, actions: &[SemanticAction]) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    for field in &dst_state.fields {
        if field.default_value.is_some() {
            continue; // Will use default in Mk helper
        }
        // Find action that assigns to this field
        let action = actions.iter().find(|a| {
            if let SemanticExpr::TransitionPeerRef { reference } = &a.target {
                reference.peer == TransitionPeerKind::Dst
                    && reference.path.first().map(|p| p.as_str()) == Some(&field.name)
            } else {
                false
            }
        });

        if let Some(action) = action {
            args.push(expr_to_tla(&action.value));
        } else {
            // Field not assigned -- use NullVal as placeholder
            args.push("NullVal".to_string());
        }
    }

    args
}

/// Emit Next relation.
fn emit_next(out: &mut String, action_names: &[String]) {
    out.push_str("\\* Next-state relation\n");
    out.push_str("Next ==\n");

    for name in action_names {
        out.push_str(&format!("    \\/ {}\n", name));
    }
    out.push('\n');
}

/// Convert a SemanticExpr to TLA+ expression string.
fn expr_to_tla(expr: &SemanticExpr) -> String {
    match expr {
        SemanticExpr::Literal { value } => literal_to_tla(value),
        SemanticExpr::Binary { op, left, right } => {
            let l = expr_to_tla(left);
            let r = expr_to_tla(right);
            let tla_op = match op.as_str() {
                "==" => "=",
                "!=" => "/=",
                "and" => "/\\",
                "or" => "\\/",
                _ => op.as_str(), // <, <=, >, >=, +, -, * pass through
            };
            format!("({} {} {})", l, tla_op, r)
        }
        SemanticExpr::Unary { op, operand } => {
            let o = expr_to_tla(operand);
            match op.as_str() {
                "not" | "!" => format!("~({})", o),
                _ => format!("{}({})", op, o),
            }
        }
        SemanticExpr::TransitionPeerRef { reference } => {
            // src.field -> sm.field, dst.field -> sm'.field
            let prefix = match reference.peer {
                TransitionPeerKind::Src => "sm",
                TransitionPeerKind::Dst => "sm'",
                TransitionPeerKind::EventParam => "",
            };
            if reference.path.is_empty() {
                prefix.to_string()
            } else if prefix.is_empty() {
                // Event param -- just the param name
                reference.path.join(".")
            } else {
                format!("{}.{}", prefix, reference.path.join("."))
            }
        }
        SemanticExpr::ValueRef { reference } => reference.value_id.clone(),
        SemanticExpr::InState { state_name, .. } => {
            format!("sm.tag = \"{}\"", state_name)
        }
        SemanticExpr::Coalesce { expr, default } => {
            let e = expr_to_tla(expr);
            let d = expr_to_tla(default);
            format!("IF {} /= NullVal THEN {} ELSE {}", e, e, d)
        }
        SemanticExpr::Subscript { base, index } => {
            let b = expr_to_tla(base);
            let i = expr_to_tla(index);
            format!("{}[{}]", b, i)
        }
        SemanticExpr::StateConstructor {
            state_name, args, ..
        } => {
            if args.is_empty() {
                format!("Mk{}", state_name)
            } else {
                let tla_args: Vec<String> = args.iter().map(expr_to_tla).collect();
                format!("Mk{}({})", state_name, tla_args.join(", "))
            }
        }
        SemanticExpr::Fill { value, count } => {
            let v = expr_to_tla(value);
            let c = expr_to_tla(count);
            format!("[i \\in 1..{} |-> {}]", c, v)
        }
        SemanticExpr::Slice { base, start, end } => {
            let b = expr_to_tla(base);
            let s = expr_to_tla(start);
            let e = expr_to_tla(end);
            format!("SubSeq({}, {} + 1, {})", b, s, e)
        }
        SemanticExpr::All {
            collection,
            state_name,
            ..
        } => {
            let c = expr_to_tla(collection);
            format!(
                "\\A idx \\in DOMAIN {} : {}[idx].tag = \"{}\"",
                c, c, state_name
            )
        }
    }
}
