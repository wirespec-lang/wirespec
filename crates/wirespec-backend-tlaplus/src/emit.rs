// crates/wirespec-backend-tlaplus/src/emit.rs

use wirespec_sema::expr::*;
use wirespec_sema::ir::*;
use wirespec_sema::types::*;

/// Information about a child state machine referenced via a delegate field.
struct ChildSmInfo {
    field_name: String,
    child_sm: SemanticStateMachine,
    is_array: bool,
    array_count: Option<u32>,
}

/// Resolve child SM references from state fields.
fn resolve_child_sms(
    sm: &SemanticStateMachine,
    all_sms: &[SemanticStateMachine],
) -> Vec<ChildSmInfo> {
    let mut result: Vec<ChildSmInfo> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for state in &sm.states {
        for field in &state.fields {
            if let Some(ref child_name) = field.child_sm_name {
                if !seen.insert(field.name.clone()) {
                    continue; // Already processed this field
                }
                if let Some(child_sm) = all_sms.iter().find(|s| &s.name == child_name) {
                    let (is_array, array_count) = match &field.ty {
                        SemanticType::Array { count_expr, .. } => {
                            let count = count_expr.as_ref().and_then(|e| {
                                if let SemanticExpr::Literal {
                                    value: SemanticLiteral::Int(n),
                                } = e.as_ref()
                                {
                                    Some(*n as u32)
                                } else {
                                    None
                                }
                            });
                            (true, count)
                        }
                        _ => (false, None),
                    };
                    result.push(ChildSmInfo {
                        field_name: field.name.clone(),
                        child_sm: child_sm.clone(),
                        is_array,
                        array_count,
                    });
                }
            }
        }
    }
    result
}

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
pub fn emit_spec(
    sm: &SemanticStateMachine,
    _bound: u32,
    all_sms: &[SemanticStateMachine],
) -> String {
    let name = &sm.name;
    let mut out = String::new();

    // Resolve child SMs
    let children = resolve_child_sms(sm, all_sms);

    // Module header
    out.push_str(&format!("---- MODULE {} ----\n", name));
    out.push_str("EXTENDS Integers, Sequences, TLC\n\n");

    // Bound constant
    out.push_str("\\* Value domains (bounded for model checking)\n");
    out.push_str("CONSTANT Bound\n");
    out.push_str("ASSUME Bound \\in Nat /\\ Bound > 0\n\n");

    // Collect all fields across all states (union)
    let all_fields = collect_all_fields(sm, &children);

    // Value domain definitions
    emit_value_domains(&mut out, &all_fields);

    // Null marker
    out.push_str("NullVal == \"@@null\"\n\n");

    // Child SM state tag sets
    for child in &children {
        let child_state_names: Vec<&str> = child
            .child_sm
            .states
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        out.push_str(&format!(
            "{}StateTag == {{{}}}\n",
            child.child_sm.name,
            child_state_names
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !children.is_empty() {
        out.push('\n');
    }

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
    emit_type_ok(&mut out, &all_fields, &children);

    // Mk helpers
    emit_mk_helpers(&mut out, sm, &all_fields, &children);

    // Init predicate
    emit_init(&mut out, sm, &children);

    // Child dispatch operators
    for child in &children {
        emit_child_dispatch(&mut out, child);
    }

    // Transition actions
    emit_transitions(&mut out, sm, &children);

    // Next
    let groups = compute_transition_groups(sm);
    let action_names: Vec<String> = groups.iter().map(|g| g.action_name.clone()).collect();
    emit_next(&mut out, &action_names);

    // Determine if terminal states exist (for liveness)
    let has_terminals = sm.states.iter().any(|s| s.is_terminal);

    // Check which built-in verify declarations are requested
    let has_verify_decls = !sm.verify_declarations.is_empty();
    let wants_nodeadlock = sm
        .verify_declarations
        .iter()
        .any(|v| matches!(v, SemanticVerifyDecl::NoDeadlock));
    let wants_allreachclosed = sm
        .verify_declarations
        .iter()
        .any(|v| matches!(v, SemanticVerifyDecl::AllReachClosed));
    let has_liveness_properties = wants_allreachclosed
        || sm
            .verify_declarations
            .iter()
            .any(|v| matches!(v, SemanticVerifyDecl::Property { .. }));

    // Spec (include WF when liveness properties are present)
    out.push_str("\\* Specification\n");
    if has_verify_decls {
        // With explicit verify declarations, only add WF if liveness properties exist
        if has_liveness_properties {
            out.push_str("Spec == Init /\\ [][Next]_sm /\\ WF_sm(Next)\n\n");
        } else {
            out.push_str("Spec == Init /\\ [][Next]_sm\n\n");
        }
    } else {
        // Legacy behavior: include WF when terminal states exist
        if has_terminals {
            out.push_str("Spec == Init /\\ [][Next]_sm /\\ WF_sm(Next)\n\n");
        } else {
            out.push_str("Spec == Init /\\ [][Next]_sm\n\n");
        }
    }

    // NoDeadlock property (only if explicitly requested or no verify declarations)
    if !has_verify_decls || wants_nodeadlock {
        out.push_str("\\* NoDeadlock: terminal states or transitions enabled\n");
        out.push_str("NoDeadlock == sm.tag \\in TerminalStates \\/ ENABLED(Next)\n\n");
    }

    // Guard exclusivity invariants
    emit_guard_exclusivity(&mut out, sm);

    // AllReachClosed liveness property
    if !has_verify_decls {
        // Legacy behavior: generate when terminal states exist
        if has_terminals {
            out.push_str("\\* AllReachClosed: eventually reach a terminal state\n");
            out.push_str("AllReachClosed == <>(sm.tag \\in TerminalStates)\n\n");
        }
    } else if wants_allreachclosed && has_terminals {
        out.push_str("\\* AllReachClosed: eventually reach a terminal state\n");
        out.push_str("AllReachClosed == <>(sm.tag \\in TerminalStates)\n\n");
    }

    // User-defined verify properties
    for vd in &sm.verify_declarations {
        if let SemanticVerifyDecl::Property { name, formula } = vd {
            let tla_formula = verify_formula_to_tla(formula);
            out.push_str(&format!("\\* User property: {}\n", name));
            out.push_str(&format!("{} == {}\n\n", name, tla_formula));
        }
    }

    // Module footer
    out.push_str("====\n");

    out
}

/// Emit the .cfg file.
pub fn emit_config(
    sm: &SemanticStateMachine,
    bound: u32,
    _all_sms: &[SemanticStateMachine],
) -> String {
    let has_terminals = sm.states.iter().any(|s| s.is_terminal);
    let has_verify_decls = !sm.verify_declarations.is_empty();
    let wants_nodeadlock = sm
        .verify_declarations
        .iter()
        .any(|v| matches!(v, SemanticVerifyDecl::NoDeadlock));
    let wants_allreachclosed = sm
        .verify_declarations
        .iter()
        .any(|v| matches!(v, SemanticVerifyDecl::AllReachClosed));

    let mut out = String::new();
    // Use INIT/NEXT instead of SPECIFICATION to avoid issues with
    // model checkers that cannot evaluate temporal operators (WF/SF)
    // inside the Spec definition during init-state computation.
    // The Spec definition (with WF) is still emitted in the .tla file
    // so parsers can extract fairness constraints for liveness checking.
    out.push_str("INIT Init\n");
    out.push_str("NEXT Next\n");
    out.push_str(&format!("CONSTANT Bound = {}\n\n", bound));
    out.push_str("INVARIANT TypeOK\n");

    if !has_verify_decls || wants_nodeadlock {
        out.push_str("INVARIANT NoDeadlock\n");
    }

    // Guard exclusivity invariants
    let groups = compute_transition_groups(sm);
    for group in &groups {
        if group.indices.len() <= 1 {
            continue;
        }
        let first_trans = &sm.transitions[group.indices[0]];
        let event_pascal = to_pascal_case(&first_trans.event_name);
        let inv_name = format!(
            "GuardExclusive_{}_{}",
            first_trans.src_state_name, event_pascal
        );
        out.push_str(&format!("INVARIANT {}\n", inv_name));
    }

    if !has_verify_decls {
        // Legacy behavior
        if has_terminals {
            out.push_str("\nPROPERTY AllReachClosed\n");
        }
    } else {
        if wants_allreachclosed && has_terminals {
            out.push_str("\nPROPERTY AllReachClosed\n");
        }
        // User-defined properties
        for vd in &sm.verify_declarations {
            if let SemanticVerifyDecl::Property { name, .. } = vd {
                out.push_str(&format!("\nPROPERTY {}\n", name));
            }
        }
    }

    out
}

/// Sentinel type used to mark a field as a child SM state tag (string domain).
/// We use `SemanticType::PacketRef` with a special `packet_id` prefix "child_sm_tag:"
/// to distinguish child SM fields from regular fields in downstream helpers.
const CHILD_SM_TAG_PREFIX: &str = "child_sm_tag:";

/// Sentinel type used to mark a field as an array of child SM state tags.
const CHILD_SM_ARRAY_TAG_PREFIX: &str = "child_sm_array_tag:";

/// Collect all unique (field_name, field_type) pairs across all states.
/// Child SM fields are represented with a special PacketRef marker type.
fn collect_all_fields(
    sm: &SemanticStateMachine,
    children: &[ChildSmInfo],
) -> Vec<(String, SemanticType)> {
    let mut fields: Vec<(String, SemanticType)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for state in &sm.states {
        for field in &state.fields {
            if seen.insert(field.name.clone()) {
                // Check if this field is a child SM field
                if let Some(child) = children.iter().find(|c| c.field_name == field.name) {
                    if child.is_array {
                        // Use array marker
                        fields.push((
                            field.name.clone(),
                            SemanticType::PacketRef {
                                packet_id: format!(
                                    "{}{}:{}",
                                    CHILD_SM_ARRAY_TAG_PREFIX,
                                    child.child_sm.name,
                                    child.array_count.unwrap_or(0)
                                ),
                                name: child.child_sm.name.clone(),
                            },
                        ));
                    } else {
                        // Use scalar marker
                        fields.push((
                            field.name.clone(),
                            SemanticType::PacketRef {
                                packet_id: format!(
                                    "{}{}",
                                    CHILD_SM_TAG_PREFIX, child.child_sm.name
                                ),
                                name: child.child_sm.name.clone(),
                            },
                        ));
                    }
                } else {
                    fields.push((field.name.clone(), field.ty.clone()));
                }
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
fn emit_type_ok(out: &mut String, fields: &[(String, SemanticType)], children: &[ChildSmInfo]) {
    out.push_str("TypeOK ==\n");
    out.push_str("    /\\ sm.tag \\in StateTag\n");
    for (name, ty) in fields {
        if let Some(child) = children.iter().find(|c| c.field_name == *name) {
            if child.is_array {
                // Array of child state tags: validate each element
                let tag_set = format!("{}StateTag", child.child_sm.name);
                out.push_str(&format!(
                    "    /\\ \\A i \\in DOMAIN sm.{} : sm.{}[i] \\in {}\n",
                    name, name, tag_set
                ));
            } else {
                let tag_set = format!("{}StateTag", child.child_sm.name);
                out.push_str(&format!(
                    "    /\\ sm.{} \\in {} \\cup {{NullVal}}\n",
                    name, tag_set
                ));
            }
        } else {
            let domain = type_to_tla_domain(ty);
            out.push_str(&format!(
                "    /\\ sm.{} \\in {} \\cup {{NullVal}}\n",
                name, domain
            ));
        }
    }
    out.push('\n');
}

/// Emit Mk helper functions for each state.
fn emit_mk_helpers(
    out: &mut String,
    sm: &SemanticStateMachine,
    all_fields: &[(String, SemanticType)],
    children: &[ChildSmInfo],
) {
    for state in &sm.states {
        let mk_name = format!("Mk{}", state.name);

        // Parameters: fields of this state without default values and not child SM fields
        let params: Vec<String> = state
            .fields
            .iter()
            .filter(|f| {
                f.default_value.is_none() && !children.iter().any(|c| c.field_name == f.name)
            })
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

            // Check if this is a child SM field
            if let Some(child) = children.iter().find(|c| c.field_name == *fname) {
                if state_field.is_some() {
                    // Field belongs to this state — use child SM initial state
                    let child_initial = child
                        .child_sm
                        .states
                        .iter()
                        .find(|s| s.state_id == child.child_sm.initial_state_id)
                        .map(|s| s.name.as_str())
                        .unwrap_or("UNKNOWN");
                    if child.is_array {
                        let count = child.array_count.unwrap_or(0);
                        record_parts.push(format!(
                            "{} |-> [i \\in 1..{} |-> \"{}\"]",
                            fname, count, child_initial
                        ));
                    } else {
                        record_parts.push(format!("{} |-> \"{}\"", fname, child_initial));
                    }
                } else {
                    record_parts.push(format!("{} |-> NullVal", fname));
                }
                continue;
            }

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
fn emit_init(out: &mut String, sm: &SemanticStateMachine, children: &[ChildSmInfo]) {
    // Find initial state by ID
    let initial_state = sm
        .states
        .iter()
        .find(|s| s.state_id == sm.initial_state_id)
        .expect("initial state not found");

    out.push_str("Init ==\n");

    // Fields without defaults need quantification over their domains
    // Exclude child SM fields (they get their initial state automatically)
    let unbound_fields: Vec<&SemanticStateField> = initial_state
        .fields
        .iter()
        .filter(|f| f.default_value.is_none() && !children.iter().any(|c| c.field_name == f.name))
        .collect();

    if unbound_fields.is_empty() {
        out.push_str(&format!("    sm = Mk{}\n\n", initial_state.name));
    } else {
        // Use sm \in {Mk(v1, v2) : v1 \in D1, v2 \in D2} form.
        // This is equivalent to \E but is compatible with tla-checker's
        // init state inference which recognises `sm \in <set>`.
        let args: Vec<String> = unbound_fields
            .iter()
            .map(|f| format!("{}_v", f.name))
            .collect();
        let quantifiers: Vec<String> = unbound_fields
            .iter()
            .map(|f| format!("{}_v \\in {}", f.name, type_to_tla_domain(&f.ty)))
            .collect();
        out.push_str(&format!(
            "    sm \\in {{Mk{}({}) : {}}}\n\n",
            initial_state.name,
            args.join(", "),
            quantifiers.join(", ")
        ));
    }
}

/// A group of transitions sharing (src_state, event). If the group has
/// multiple members, they must all be guarded (enforced by sema).
struct TransitionGroup {
    /// Indices into sm.transitions
    indices: Vec<usize>,
    /// The action name for this group
    action_name: String,
}

/// Group transitions by (src_state, event) and compute a unique action name
/// for each group. Preserves ordering by first occurrence.
fn compute_transition_groups(sm: &SemanticStateMachine) -> Vec<TransitionGroup> {
    use std::collections::HashMap;

    // Build groups keyed by (src_state, event), preserving order of first occurrence
    let mut group_map: HashMap<(String, String), usize> = HashMap::new();
    let mut groups: Vec<TransitionGroup> = Vec::new();

    for (i, trans) in sm.transitions.iter().enumerate() {
        let key = (trans.src_state_name.clone(), trans.event_name.clone());
        if let Some(&gidx) = group_map.get(&key) {
            groups[gidx].indices.push(i);
        } else {
            let gidx = groups.len();
            group_map.insert(key, gidx);
            groups.push(TransitionGroup {
                indices: vec![i],
                action_name: String::new(), // filled below
            });
        }
    }

    // Compute action names for each group
    // Count how many groups share the same event name (across different src states)
    let mut event_group_count: HashMap<String, usize> = HashMap::new();
    for g in &groups {
        let trans = &sm.transitions[g.indices[0]];
        *event_group_count
            .entry(trans.event_name.clone())
            .or_default() += 1;
    }

    let mut seen_names: Vec<String> = Vec::new();
    for g in &mut groups {
        let trans = &sm.transitions[g.indices[0]];
        let event_pascal = to_pascal_case(&trans.event_name);

        let action_name = if event_group_count
            .get(&trans.event_name)
            .copied()
            .unwrap_or(0)
            > 1
        {
            format!("{}From{}", event_pascal, trans.src_state_name)
        } else {
            event_pascal
        };

        // Deduplicate if collision
        let final_name = if seen_names.contains(&action_name) {
            format!("{}_{}", action_name, g.indices[0])
        } else {
            action_name
        };
        seen_names.push(final_name.clone());
        g.action_name = final_name;
    }

    groups
}

/// Emit the dispatch operator for a child state machine.
/// Maps (child_state, event_ordinal) pairs to new child states.
fn emit_child_dispatch(out: &mut String, child: &ChildSmInfo) {
    let child_sm = &child.child_sm;
    out.push_str(&format!(
        "\\* Dispatch operator for child SM {}\n",
        child_sm.name
    ));
    out.push_str(&format!("{}Dispatch(child_state, ev) ==\n", child_sm.name));

    // Map each transition to a CASE arm, using event ordinal
    // Events are numbered by their order in child_sm.events
    let mut cases: Vec<String> = Vec::new();
    for trans in &child_sm.transitions {
        // Find the event ordinal
        let event_ordinal = child_sm
            .events
            .iter()
            .position(|e| e.event_id == trans.event_id)
            .unwrap_or(0);
        cases.push(format!(
            "child_state = \"{}\" /\\ ev = {} -> \"{}\"",
            trans.src_state_name, event_ordinal, trans.dst_state_name
        ));
    }

    if cases.is_empty() {
        out.push_str("    \"INVALID\"\n\n");
    } else {
        for (i, case) in cases.iter().enumerate() {
            if i == 0 {
                out.push_str(&format!("    CASE {}\n", case));
            } else {
                out.push_str(&format!("      [] {}\n", case));
            }
        }
        out.push_str("      [] OTHER -> \"INVALID\"\n\n");
    }
}

/// Extract the child field name from a delegate target expression.
fn extract_delegate_field_name(target: &SemanticExpr) -> Option<String> {
    match target {
        SemanticExpr::TransitionPeerRef { reference } => {
            if reference.peer == TransitionPeerKind::Src {
                reference.path.first().cloned()
            } else {
                None
            }
        }
        SemanticExpr::Subscript { base, .. } => extract_delegate_field_name(base),
        _ => None,
    }
}

/// Check if a delegate target expression has an index subscript.
fn extract_delegate_index(target: &SemanticExpr) -> Option<Box<SemanticExpr>> {
    if let SemanticExpr::Subscript { index, .. } = target {
        Some(index.clone())
    } else {
        None
    }
}

/// Emit transition actions, handling guarded groups as disjunctions.
fn emit_transitions(out: &mut String, sm: &SemanticStateMachine, children: &[ChildSmInfo]) {
    let groups = compute_transition_groups(sm);

    for group in &groups {
        let first_trans = &sm.transitions[group.indices[0]];

        if group.indices.len() == 1 {
            // Single transition -- emit normally
            let trans = first_trans;

            // Check if this is a delegate transition
            if let Some(ref delegate) = trans.delegate {
                let field_name = extract_delegate_field_name(&delegate.target);
                let index_expr = extract_delegate_index(&delegate.target);
                if let Some(ref fname) = field_name
                    && let Some(child) = children.iter().find(|c| c.field_name == *fname)
                {
                    // Comment
                    out.push_str(&format!(
                        "\\* delegate transition {} -> {} {{ on {} }}\n",
                        trans.src_state_name, trans.dst_state_name, trans.event_name
                    ));

                    out.push_str(&format!("{} ==\n", group.action_name));

                    // Source state guard
                    out.push_str(&format!("    /\\ sm.tag = \"{}\"\n", trans.src_state_name));

                    if child.is_array {
                        // Array child: existentially quantify over index and event
                        let idx_var = if let Some(ref ie) = index_expr {
                            expr_to_tla(ie)
                        } else {
                            "idx".to_string()
                        };
                        out.push_str(&format!(
                            "    /\\ \\E {} \\in 1..Len(sm.{}), ev \\in BoundedNat:\n",
                            idx_var, fname
                        ));
                        out.push_str(&format!(
                            "        /\\ LET new_child == {}Dispatch(sm.{}[{}], ev)\n",
                            child.child_sm.name, fname, idx_var
                        ));
                        out.push_str("           IN /\\ new_child /= \"INVALID\"\n");
                        out.push_str(&format!(
                            "              /\\ sm' = [sm EXCEPT !.{}[{}] = new_child]\n",
                            fname, idx_var
                        ));
                    } else {
                        // Scalar child: existentially quantify over event
                        out.push_str("    /\\ \\E ev \\in BoundedNat:\n");
                        out.push_str(&format!(
                            "        /\\ LET new_child == {}Dispatch(sm.{}, ev)\n",
                            child.child_sm.name, fname
                        ));
                        out.push_str("           IN /\\ new_child /= \"INVALID\"\n");
                        out.push_str(&format!(
                            "              /\\ sm' = [sm EXCEPT !.{} = new_child]\n",
                            fname
                        ));
                    }

                    out.push('\n');
                    continue;
                }
            }

            // Comment
            out.push_str(&format!(
                "\\* transition {} -> {} {{ on {} }}\n",
                trans.src_state_name, trans.dst_state_name, trans.event_name
            ));

            out.push_str(&format!("{} ==\n", group.action_name));

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
            emit_dst_state(out, sm, trans, indent);

            out.push('\n');
        } else {
            // Multiple guarded transitions -- emit as disjunction
            // Comment
            let branch_descs: Vec<String> = group
                .indices
                .iter()
                .map(|&i| {
                    let t = &sm.transitions[i];
                    format!("{} -> {}", t.src_state_name, t.dst_state_name)
                })
                .collect();
            out.push_str(&format!(
                "\\* guarded branches on {}: {}\n",
                first_trans.event_name,
                branch_descs.join(", ")
            ));

            out.push_str(&format!("{} ==\n", group.action_name));

            // Source state guard (shared by all branches)
            out.push_str(&format!(
                "    /\\ sm.tag = \"{}\"\n",
                first_trans.src_state_name
            ));

            // Event params -> existential quantification (shared)
            let event = sm
                .events
                .iter()
                .find(|e| e.event_id == first_trans.event_id);
            let params = event.map(|e| &e.params).cloned().unwrap_or_default();

            let base_indent = if !params.is_empty() {
                let quantifiers: Vec<String> = params
                    .iter()
                    .map(|p| format!("{} \\in {}", p.name, type_to_tla_domain(&p.ty)))
                    .collect();
                out.push_str(&format!("    /\\ \\E {}:\n", quantifiers.join(", ")));
                "        "
            } else {
                "    "
            };

            // Disjunction of guarded branches
            for (branch_idx, &ti) in group.indices.iter().enumerate() {
                let trans = &sm.transitions[ti];
                let disj_prefix = if branch_idx == 0 {
                    format!("{}/\\ \\/ ", base_indent)
                } else {
                    format!("{}   \\/ ", base_indent)
                };

                // Guard (required for all branches in a group)
                if let Some(ref guard) = trans.guard {
                    let guard_str = expr_to_tla(guard);
                    out.push_str(&format!("{}/\\ {}\n", disj_prefix, guard_str));
                }

                // Destination state construction
                let inner_indent = format!("{}      ", base_indent);
                emit_dst_state_with_indent(out, sm, trans, &inner_indent);
            }

            out.push('\n');
        }
    }
}

/// Check if any action targets a field that has a default value in the
/// destination state. When this is true we must emit an inline record
/// instead of using the Mk helper (which bakes in the default).
fn actions_override_defaults(dst_state: &SemanticState, actions: &[SemanticAction]) -> bool {
    actions.iter().any(|action| {
        if let SemanticExpr::TransitionPeerRef { reference } = &action.target
            && reference.peer == TransitionPeerKind::Dst
            && let Some(field_name) = reference.path.first()
            && let Some(field) = dst_state.fields.iter().find(|f| &f.name == field_name)
        {
            field.default_value.is_some()
        } else {
            false
        }
    })
}

/// Build an inline record literal for the destination state, using
/// action values for assigned fields and defaults/NullVal for the rest.
fn build_inline_record(
    dst_state: &SemanticState,
    all_fields: &[(String, SemanticType)],
    actions: &[SemanticAction],
) -> String {
    let mut parts: Vec<String> = vec![format!("tag |-> \"{}\"", dst_state.name)];
    for (fname, _) in all_fields {
        // Check if there's an action that assigns to this field
        let action = actions.iter().find(|a| {
            if let SemanticExpr::TransitionPeerRef { reference } = &a.target {
                reference.peer == TransitionPeerKind::Dst
                    && reference.path.first().map(|p| p.as_str()) == Some(fname)
            } else {
                false
            }
        });

        if let Some(action) = action {
            parts.push(format!("{} |-> {}", fname, expr_to_tla(&action.value)));
        } else if let Some(field) = dst_state.fields.iter().find(|f| &f.name == fname) {
            // Field belongs to this state
            if let Some(ref default) = field.default_value {
                parts.push(format!("{} |-> {}", fname, literal_to_tla(default)));
            } else {
                parts.push(format!("{} |-> NullVal", fname));
            }
        } else {
            // Field doesn't belong to this state
            parts.push(format!("{} |-> NullVal", fname));
        }
    }
    format!("[{}]", parts.join(", "))
}

/// Emit destination state construction for a transition.
fn emit_dst_state(
    out: &mut String,
    sm: &SemanticStateMachine,
    trans: &SemanticTransition,
    indent: &str,
) {
    let dst_state = sm.states.iter().find(|s| s.name == trans.dst_state_name);
    if let Some(dst) = dst_state {
        if actions_override_defaults(dst, &trans.actions) {
            let all_fields = collect_all_fields(sm, &[]);
            let record = build_inline_record(dst, &all_fields, &trans.actions);
            out.push_str(&format!("{}/\\ sm' = {}\n", indent, record));
        } else {
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
    }
}

/// Emit destination state construction with a specific indent string.
fn emit_dst_state_with_indent(
    out: &mut String,
    sm: &SemanticStateMachine,
    trans: &SemanticTransition,
    indent: &str,
) {
    let dst_state = sm.states.iter().find(|s| s.name == trans.dst_state_name);
    if let Some(dst) = dst_state {
        if actions_override_defaults(dst, &trans.actions) {
            let all_fields = collect_all_fields(sm, &[]);
            let record = build_inline_record(dst, &all_fields, &trans.actions);
            out.push_str(&format!("{}/\\ sm' = {}\n", indent, record));
        } else {
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
    }
}

/// Emit guard exclusivity invariants for guarded groups. For each group of
/// guarded transitions sharing (state, event), generate pairwise mutual
/// exclusion invariants.
fn emit_guard_exclusivity(out: &mut String, sm: &SemanticStateMachine) -> Vec<String> {
    let groups = compute_transition_groups(sm);
    let mut invariant_names: Vec<String> = Vec::new();

    for group in &groups {
        if group.indices.len() <= 1 {
            continue; // No exclusivity needed for single transitions
        }

        let first_trans = &sm.transitions[group.indices[0]];
        let event_pascal = to_pascal_case(&first_trans.event_name);
        let inv_name = format!(
            "GuardExclusive_{}_{}",
            first_trans.src_state_name, event_pascal
        );

        out.push_str(&format!(
            "\\* Guard exclusivity for {} in state {}\n",
            first_trans.event_name, first_trans.src_state_name
        ));
        out.push_str(&format!("{} ==\n", inv_name));
        out.push_str(&format!(
            "    sm.tag = \"{}\" =>\n",
            first_trans.src_state_name
        ));

        // Collect guard expressions
        let guards: Vec<String> = group
            .indices
            .iter()
            .map(|&i| {
                let trans = &sm.transitions[i];
                trans
                    .guard
                    .as_ref()
                    .map(expr_to_tla)
                    .unwrap_or_else(|| "TRUE".to_string())
            })
            .collect();

        // Generate pairwise exclusivity: ~(g_i /\ g_j) for all i < j
        let mut pairs: Vec<String> = Vec::new();
        for i in 0..guards.len() {
            for j in (i + 1)..guards.len() {
                pairs.push(format!("~({} /\\ {})", guards[i], guards[j]));
            }
        }

        if pairs.len() == 1 {
            out.push_str(&format!("        {}\n", pairs[0]));
        } else {
            for pair in &pairs {
                out.push_str(&format!("        /\\ {}\n", pair));
            }
        }

        out.push('\n');
        invariant_names.push(inv_name);
    }

    invariant_names
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
                "~>" => "~>",
                _ => op.as_str(), // <, <=, >, >=, +, -, * pass through
            };
            format!("({} {} {})", l, tla_op, r)
        }
        SemanticExpr::Unary { op, operand } => {
            let o = expr_to_tla(operand);
            match op.as_str() {
                "not" | "!" => format!("~({})", o),
                "<>" => format!("<>({})", o),
                "[]" => format!("[]({})", o),
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
        SemanticExpr::InState {
            expr, state_name, ..
        } => {
            // Check if the inner expression refers to a child field
            let field_ref = expr_to_tla(expr);
            if field_ref.starts_with("sm.") && field_ref != "sm" {
                // Child field: e.g., sm.child = "Done"
                format!("{} = \"{}\"", field_ref, state_name)
            } else {
                // Parent state: sm.tag = "StateName"
                format!("sm.tag = \"{}\"", state_name)
            }
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

/// Convert a SemanticVerifyFormula to TLA+ expression string.
fn verify_formula_to_tla(f: &SemanticVerifyFormula) -> String {
    match f {
        SemanticVerifyFormula::InState { state_name } => {
            format!("sm.tag = \"{}\"", state_name)
        }
        SemanticVerifyFormula::Not { inner } => {
            format!("~({})", verify_formula_to_tla(inner))
        }
        SemanticVerifyFormula::And { left, right } => {
            format!(
                "({} /\\ {})",
                verify_formula_to_tla(left),
                verify_formula_to_tla(right)
            )
        }
        SemanticVerifyFormula::Or { left, right } => {
            format!(
                "({} \\/ {})",
                verify_formula_to_tla(left),
                verify_formula_to_tla(right)
            )
        }
        SemanticVerifyFormula::Implies { left, right } => {
            format!(
                "({} => {})",
                verify_formula_to_tla(left),
                verify_formula_to_tla(right)
            )
        }
        SemanticVerifyFormula::Always { inner } => {
            format!("[]({})", verify_formula_to_tla(inner))
        }
        SemanticVerifyFormula::Eventually { inner } => {
            format!("<>({})", verify_formula_to_tla(inner))
        }
        SemanticVerifyFormula::LeadsTo { left, right } => {
            format!(
                "({}) ~> ({})",
                verify_formula_to_tla(left),
                verify_formula_to_tla(right)
            )
        }
        SemanticVerifyFormula::FieldRef { field_name } => {
            format!("sm.{}", field_name)
        }
        SemanticVerifyFormula::Literal { value } => value.to_string(),
        SemanticVerifyFormula::BoolLiteral { value } => {
            if *value { "TRUE" } else { "FALSE" }.to_string()
        }
        SemanticVerifyFormula::Compare { left, op, right } => {
            let tla_op = match op.as_str() {
                "==" => "=",
                "!=" => "/=",
                _ => op.as_str(),
            };
            format!(
                "({} {} {})",
                verify_formula_to_tla(left),
                tla_op,
                verify_formula_to_tla(right)
            )
        }
    }
}
