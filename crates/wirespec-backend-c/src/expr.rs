// crates/wirespec-backend-c/src/expr.rs
//
// CodecExpr -> C expression string conversion.

use wirespec_codec::ir::*;

/// Context for expression generation: field refs use different prefixes.
pub enum ExprContext {
    /// Parse context: field refs use `out->`
    Parse,
    /// Serialize context: field refs use `val->`
    Serialize,
    /// Parse context inside a capsule variant: variant-local fields use
    /// `out->value.{variant}.` while header fields use `out->`.
    CapsuleVariantParse {
        variant_prefix: String,
        header_field_names: Vec<String>,
    },
    /// Serialize context inside a capsule variant: variant-local fields use
    /// `val->value.{variant}.` while header fields use `val->`.
    CapsuleVariantSerialize {
        variant_prefix: String,
        header_field_names: Vec<String>,
    },
}

/// Module prefix for const name resolution (e.g., "quic_frames").
/// Thread-local to avoid threading through all expr_to_c calls.
use std::cell::RefCell;
thread_local! {
    static CONST_PREFIX: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Set the module prefix for const name resolution.
pub fn set_const_prefix(prefix: &str) {
    CONST_PREFIX.with(|p| *p.borrow_mut() = prefix.to_string());
}

impl ExprContext {
    fn resolve_prefix<'a>(&'a self, field_name: &str) -> &'a str {
        match self {
            ExprContext::Parse => "out->",
            ExprContext::Serialize => "val->",
            ExprContext::CapsuleVariantParse {
                variant_prefix,
                header_field_names,
            } => {
                if header_field_names.iter().any(|n| n == field_name) {
                    "out->"
                } else {
                    variant_prefix.as_str()
                }
            }
            ExprContext::CapsuleVariantSerialize {
                variant_prefix,
                header_field_names,
            } => {
                if header_field_names.iter().any(|n| n == field_name) {
                    "val->"
                } else {
                    variant_prefix.as_str()
                }
            }
        }
    }
}

/// Convert a CodecExpr to a C expression string.
pub fn expr_to_c(expr: &CodecExpr, ctx: &ExprContext) -> String {
    match expr {
        CodecExpr::ValueRef { reference } => match reference.kind {
            ValueRefKind::Field | ValueRefKind::Derived => {
                let name = extract_field_name(&reference.value_id);
                let prefix = ctx.resolve_prefix(name);
                format!("{prefix}{name}")
            }
            ValueRefKind::Const => {
                let name_upper = crate::names::to_snake_case(&reference.value_id).to_uppercase();
                CONST_PREFIX.with(|p| {
                    let prefix = p.borrow();
                    if prefix.is_empty() {
                        name_upper.clone()
                    } else {
                        format!("{}_{}", prefix.to_uppercase(), name_upper)
                    }
                })
            }
        },
        CodecExpr::Literal { value } => match value {
            LiteralValue::Int(n) => {
                if *n < 0 {
                    format!("({n})")
                } else if *n > 0xFFFF_FFFF {
                    format!("{n}ULL")
                } else if *n > 0xFFFF {
                    format!("{n}UL")
                } else {
                    format!("{n}")
                }
            }
            LiteralValue::Bool(b) => if *b { "true" } else { "false" }.into(),
            LiteralValue::String(s) => format!("\"{s}\""),
            LiteralValue::Null => "0".into(),
        },
        CodecExpr::Binary { op, left, right } => {
            let l = expr_to_c(left, ctx);
            let r = expr_to_c(right, ctx);
            let c_op = match op.as_str() {
                "and" => "&&",
                "or" => "||",
                o => o,
            };
            format!("({l} {c_op} {r})")
        }
        CodecExpr::Unary { op, operand } => {
            let o = expr_to_c(operand, ctx);
            format!("({op}{o})")
        }
        CodecExpr::Coalesce {
            expr: e,
            default: d,
        } => {
            let e_str = expr_to_c(e, ctx);
            let d_str = expr_to_c(d, ctx);
            // For optional field coalesce: has_X ? X : default
            if let CodecExpr::ValueRef { reference } = e.as_ref() {
                let name = extract_field_name(&reference.value_id);
                let prefix = ctx.resolve_prefix(name);
                format!("({prefix}has_{name} ? {e_str} : {d_str})")
            } else {
                format!("({e_str} ? {e_str} : {d_str})")
            }
        }
        CodecExpr::Subscript { base, index } => {
            let b = expr_to_c(base, ctx);
            let i = expr_to_c(index, ctx);
            format!("{b}[{i}]")
        }
        CodecExpr::InState { .. }
        | CodecExpr::StateConstructor { .. }
        | CodecExpr::Fill { .. }
        | CodecExpr::Slice { .. }
        | CodecExpr::All { .. } => unreachable!("SM expression in non-SM context"),
    }
}

/// Extract the field name from a value_id.
///
/// IDs may look like `"field_name"` or `"scope:S.field_name"` or `"scope:S.field[0]"`.
/// We want just the final field name portion.
pub fn extract_field_name(id: &str) -> &str {
    // If there is a dot, take everything after the last dot
    let after_dot = if let Some(dot_pos) = id.rfind('.') {
        &id[dot_pos + 1..]
    } else {
        id
    };
    // Strip any trailing bracket subscripts like "[0]"
    if let Some(bracket) = after_dot.find('[') {
        &after_dot[..bracket]
    } else {
        after_dot
    }
}

// ── State machine expression helpers ──

use wirespec_sema::expr::{SemanticExpr, SemanticLiteral, TransitionPeerKind};
use wirespec_sema::ir::SemanticStateMachine;
use wirespec_sema::types::SemanticType;

/// Check if an expression refers to a bytes type by looking up the field/param
/// in the SM's state definitions or event definitions.
fn is_bytes_expr(expr: &SemanticExpr, ctx: &SmExprContext) -> bool {
    if let SemanticExpr::TransitionPeerRef { reference } = expr
        && let Some(field_name) = reference.path.first()
        && let Some(sm) = ctx.sm
    {
        match reference.peer {
            TransitionPeerKind::Src | TransitionPeerKind::Dst => {
                // Look up field in states
                for state in &sm.states {
                    for f in &state.fields {
                        if &f.name == field_name {
                            return matches!(&f.ty, SemanticType::Bytes { .. });
                        }
                    }
                }
            }
            TransitionPeerKind::EventParam => {
                // Look up param in events
                for event in &sm.events {
                    for p in &event.params {
                        if &p.name == field_name {
                            return matches!(&p.ty, SemanticType::Bytes { .. });
                        }
                    }
                }
            }
        }
    }
    false
}

/// Context for state machine transition expression generation.
pub struct SmExprContext<'a> {
    /// snake_case name of the source state, for `sm->{src_state}.field`
    pub src_state_snake: &'a str,
    /// snake_case name of the destination state, for `dst.{dst_state}.field`
    pub dst_state_snake: &'a str,
    /// snake_case name of the event, for `event->{event}.param`
    pub event_snake: &'a str,
    /// The state machine definition, for looking up state fields in StateConstructor etc.
    pub sm: Option<&'a SemanticStateMachine>,
    /// The C prefix (e.g. "test"), for generating tag enum values.
    pub prefix: &'a str,
}

/// Convert a SemanticExpr (from sema IR) to a C expression string,
/// within the context of a state machine transition's dispatch function.
///
/// - `src.field`  -> `sm->{src_state_snake}.field`
/// - `dst.field`  -> `dst.{dst_state_snake}.field`
/// - `event.param` -> `event->{event_snake}.param`
pub fn sema_expr_to_c(expr: &SemanticExpr, ctx: &SmExprContext) -> String {
    match expr {
        SemanticExpr::TransitionPeerRef { reference } => {
            let path = reference.path.join(".");
            match reference.peer {
                TransitionPeerKind::Src => {
                    format!("sm->{}.{path}", ctx.src_state_snake)
                }
                TransitionPeerKind::Dst => {
                    format!("dst.{}.{path}", ctx.dst_state_snake)
                }
                TransitionPeerKind::EventParam => {
                    format!("event->{}.{path}", ctx.event_snake)
                }
            }
        }
        SemanticExpr::ValueRef { reference } => {
            // In SM context, value refs to consts use uppercase
            match reference.kind {
                wirespec_sema::expr::ValueRefKind::Const => reference.value_id.to_uppercase(),
                _ => {
                    let name = extract_field_name(&reference.value_id);
                    format!("sm->{}.{name}", ctx.src_state_snake)
                }
            }
        }
        SemanticExpr::Literal { value } => match value {
            SemanticLiteral::Int(n) => {
                if *n < 0 {
                    format!("({n})")
                } else if *n > 0xFFFF_FFFF {
                    format!("{n}ULL")
                } else if *n > 0xFFFF {
                    format!("{n}UL")
                } else {
                    format!("{n}")
                }
            }
            SemanticLiteral::Bool(b) => if *b { "true" } else { "false" }.into(),
            SemanticLiteral::String(s) => format!("\"{s}\""),
            SemanticLiteral::Null => "0".into(),
        },
        SemanticExpr::Binary { op, left, right } => {
            let l = sema_expr_to_c(left, ctx);
            let r = sema_expr_to_c(right, ctx);
            // For bytes comparison (== / !=), use memcmp instead of direct ==
            let is_bytes_cmp = (op == "==" || op == "!=") && is_bytes_expr(left, ctx);
            if is_bytes_cmp {
                if op == "==" {
                    format!("({l}.len == {r}.len && memcmp({l}.ptr, {r}.ptr, {l}.len) == 0)")
                } else {
                    format!("({l}.len != {r}.len || memcmp({l}.ptr, {r}.ptr, {l}.len) != 0)")
                }
            } else {
                let c_op = match op.as_str() {
                    "and" => "&&",
                    "or" => "||",
                    o => o,
                };
                format!("({l} {c_op} {r})")
            }
        }
        SemanticExpr::Unary { op, operand } => {
            let o = sema_expr_to_c(operand, ctx);
            format!("({op}{o})")
        }
        // Subscript generates a bare array[index] expression. Bounds checking
        // is done at the statement level in source.rs where these expressions
        // are used (e.g., indexed delegates emit a >= check before indexing).
        SemanticExpr::Subscript { base, index } => {
            let b = sema_expr_to_c(base, ctx);
            let i = sema_expr_to_c(index, ctx);
            format!("{b}[{i}]")
        }
        SemanticExpr::InState {
            expr,
            sm_name,
            state_name,
            ..
        } => {
            let expr_c = sema_expr_to_c(expr, ctx);
            let sm_snake = crate::names::to_snake_case(sm_name);
            let state_snake = crate::names::to_snake_case(state_name);
            let prefix_upper = ctx.prefix.to_uppercase();
            let sm_upper = sm_snake.to_uppercase();
            let state_upper = state_snake.to_uppercase();
            format!("({expr_c}.tag == {prefix_upper}_{sm_upper}_{state_upper})")
        }
        SemanticExpr::StateConstructor {
            sm_name,
            state_name,
            args,
            ..
        } => {
            let sm_snake = crate::names::to_snake_case(sm_name);
            let state_snake = crate::names::to_snake_case(state_name);
            let prefix_upper = ctx.prefix.to_uppercase();
            let sm_upper = sm_snake.to_uppercase();
            let state_upper = state_snake.to_uppercase();
            let sm_type = format!("{prefix}_{sm_snake}_t", prefix = ctx.prefix);
            let tag = format!("{prefix_upper}_{sm_upper}_{state_upper}");

            if args.is_empty() {
                format!("(({sm_type}){{ .tag = {tag} }})")
            } else {
                // Try to get field names from the SM definition
                let field_names: Vec<String> = ctx
                    .sm
                    .and_then(|sm| sm.states.iter().find(|s| &s.name == state_name))
                    .map(|state| {
                        state
                            .fields
                            .iter()
                            .map(|f| crate::names::to_snake_case(&f.name))
                            .collect()
                    })
                    .unwrap_or_default();

                let arg_strs: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let val = sema_expr_to_c(a, ctx);
                        if i < field_names.len() {
                            format!(".{} = {val}", field_names[i])
                        } else {
                            val
                        }
                    })
                    .collect();

                let inits = arg_strs.join(", ");
                format!("(({sm_type}){{ .tag = {tag}, .{state_snake} = {{ {inits} }} }})")
            }
        }
        SemanticExpr::Fill { .. } => {
            // Fill is a statement-level construct. In expression context, generate
            // a placeholder. The actual for-loop is emitted in source.rs action handling.
            "/* fill: see action emission */".into()
        }
        SemanticExpr::Slice { base, start, end } => {
            // Slice is not a standalone C expression; it's used as a collection
            // inside All. Generate a comment noting this.
            let b = sema_expr_to_c(base, ctx);
            let s = sema_expr_to_c(start, ctx);
            let e = sema_expr_to_c(end, ctx);
            format!("/* slice: {b}[{s}..{e}] */")
        }
        SemanticExpr::All {
            collection,
            sm_name,
            state_name,
            ..
        } => {
            // All is a statement-level guard check. In expression context, generate
            // an inline block expression (GCC statement-expression extension).
            let sm_snake = crate::names::to_snake_case(sm_name);
            let state_snake = crate::names::to_snake_case(state_name);
            let prefix_upper = ctx.prefix.to_uppercase();
            let sm_upper = sm_snake.to_uppercase();
            let state_upper = state_snake.to_uppercase();
            let tag = format!("{prefix_upper}_{sm_upper}_{state_upper}");

            // Extract base/start/end from a Slice collection, or iterate the whole thing
            match collection.as_ref() {
                SemanticExpr::Slice { base, start, end } => {
                    let base_c = sema_expr_to_c(base, ctx);
                    let start_c = sema_expr_to_c(start, ctx);
                    let end_c = sema_expr_to_c(end, ctx);
                    format!(
                        "({{ bool _all_ok = true; \
                        for (size_t _aj = (size_t)({start_c}); _aj < (size_t)({end_c}); _aj++) {{ \
                        if ({base_c}[_aj].tag != {tag}) {{ _all_ok = false; break; }} \
                        }} _all_ok; }})"
                    )
                }
                _ => {
                    let coll_c = sema_expr_to_c(collection, ctx);
                    format!("({{ bool _all_ok = true; /* all check on {coll_c} */ _all_ok; }})")
                }
            }
        }
        SemanticExpr::Coalesce {
            expr: e,
            default: d,
        } => {
            let e_str = sema_expr_to_c(e, ctx);
            let d_str = sema_expr_to_c(d, ctx);
            format!("({e_str} ? {e_str} : {d_str})")
        }
    }
}
