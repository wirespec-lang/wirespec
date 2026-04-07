// crates/wirespec-sema/src/analyzer/validate.rs
//! SM validation helpers and expression resolution.

use super::*;

/// Convert bare NameRef/ValueRef nodes that match event parameter names into
/// `TransitionPeerRef { peer: EventParam }` so the C backend emits
/// `event->{event_snake}.{param}` instead of `sm->{state}.{param}`.
pub(super) fn resolve_event_params(expr: &mut SemanticExpr, param_names: &[String]) {
    match expr {
        SemanticExpr::ValueRef { reference }
            if reference.kind == ValueRefKind::Field
                && param_names.contains(&reference.value_id) =>
        {
            let name = reference.value_id.clone();
            *expr = SemanticExpr::TransitionPeerRef {
                reference: TransitionPeerRef {
                    peer: TransitionPeerKind::EventParam,
                    event_param_id: None,
                    path: vec![name],
                },
            };
        }
        SemanticExpr::Binary { left, right, .. } => {
            resolve_event_params(left, param_names);
            resolve_event_params(right, param_names);
        }
        SemanticExpr::Unary { operand, .. } => {
            resolve_event_params(operand, param_names);
        }
        SemanticExpr::Subscript { base, index } => {
            resolve_event_params(base, param_names);
            resolve_event_params(index, param_names);
        }
        SemanticExpr::StateConstructor { args, .. } => {
            for arg in args {
                resolve_event_params(arg, param_names);
            }
        }
        SemanticExpr::Fill { value, count } => {
            resolve_event_params(value, param_names);
            resolve_event_params(count, param_names);
        }
        SemanticExpr::InState { expr: inner, .. } => {
            resolve_event_params(inner, param_names);
        }
        SemanticExpr::All { collection, .. } => {
            resolve_event_params(collection, param_names);
        }
        SemanticExpr::Slice { base, start, end } => {
            resolve_event_params(base, param_names);
            resolve_event_params(start, param_names);
            resolve_event_params(end, param_names);
        }
        SemanticExpr::Coalesce { expr: e, default } => {
            resolve_event_params(e, param_names);
            resolve_event_params(default, param_names);
        }
        _ => {}
    }
}

/// Resolve empty `sm_name`/`sm_id` in InState/All expressions by looking up
/// the referenced field's child SM type from the source states.
pub(super) fn resolve_guard_sm_names(expr: &mut SemanticExpr, states: &[SemanticState]) {
    match expr {
        SemanticExpr::InState {
            expr: inner,
            sm_id,
            sm_name,
            ..
        } if sm_name.is_empty() => {
            // Try to resolve from the inner expression's field reference
            if let Some(child_sm) = extract_child_sm_name(inner, states) {
                *sm_name = child_sm.clone();
                *sm_id = format!("sm:{child_sm}");
            }
            resolve_guard_sm_names(inner, states);
        }
        SemanticExpr::All {
            collection,
            sm_id,
            sm_name,
            ..
        } if sm_name.is_empty() => {
            // For All, the collection is typically a Slice whose base is a field ref
            let field_expr = match collection.as_ref() {
                SemanticExpr::Slice { base, .. } => Some(base.as_ref()),
                _ => None,
            };
            if let Some(fe) = field_expr
                && let Some(child_sm) = extract_child_sm_name(fe, states)
            {
                *sm_name = child_sm.clone();
                *sm_id = format!("sm:{child_sm}");
            }
            resolve_guard_sm_names(collection, states);
        }
        SemanticExpr::Binary { left, right, .. } => {
            resolve_guard_sm_names(left, states);
            resolve_guard_sm_names(right, states);
        }
        SemanticExpr::Unary { operand, .. } => {
            resolve_guard_sm_names(operand, states);
        }
        _ => {}
    }
}

/// Extract the child SM name from a field reference expression by looking
/// up the field in the state definitions.
fn extract_child_sm_name(expr: &SemanticExpr, states: &[SemanticState]) -> Option<String> {
    if let SemanticExpr::TransitionPeerRef { reference } = expr
        && let Some(field_name) = reference.path.first()
    {
        // Search all states for this field
        for state in states {
            for field in &state.fields {
                if &field.name == field_name {
                    return field.child_sm_name.clone();
                }
            }
        }
    }
    None
}
