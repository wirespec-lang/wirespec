// crates/wirespec-backend-rust/src/expr.rs
//
// CodecExpr -> Rust expression string conversion.

use wirespec_codec::ir::*;

use crate::names::rust_ident;

/// Context for expression generation: field refs use different prefixes.
pub enum ExprContext {
    /// Parse context: field refs are local variables (bare name).
    Parse,
    /// Serialize context: field refs use `self.`
    Serialize,
}

impl ExprContext {
    fn field_prefix(&self) -> &'static str {
        match self {
            ExprContext::Parse => "",
            ExprContext::Serialize => "self.",
        }
    }
}

fn resolve_field_alias<'a>(name: &str, aliases: &'a [(&'a str, &'a str)]) -> Option<&'a str> {
    aliases
        .iter()
        .find_map(|(from, to)| (*from == name).then_some(*to))
}

fn expr_to_rust_with_aliases(
    expr: &CodecExpr,
    ctx: &ExprContext,
    aliases: &[(&str, &str)],
) -> String {
    match expr {
        CodecExpr::ValueRef { reference } => match reference.kind {
            ValueRefKind::Field | ValueRefKind::Derived => {
                let raw_name = extract_field_name(&reference.value_id);
                if let Some(alias) = resolve_field_alias(raw_name, aliases) {
                    alias.to_string()
                } else {
                    let name = rust_ident(raw_name);
                    let prefix = ctx.field_prefix();
                    format!("{prefix}{name}")
                }
            }
            ValueRefKind::Const => {
                // Constants use UPPER_SNAKE_CASE
                reference.value_id.to_uppercase()
            }
        },
        CodecExpr::Literal { value } => match value {
            LiteralValue::Int(n) => {
                if *n < 0 {
                    format!("({n})")
                } else {
                    format!("{n}")
                }
            }
            LiteralValue::Bool(b) => if *b { "true" } else { "false" }.into(),
            LiteralValue::String(s) => format!("\"{s}\""),
            LiteralValue::Null => "None".into(),
        },
        CodecExpr::Binary { op, left, right } => {
            let l = expr_to_rust_with_aliases(left, ctx, aliases);
            let r = expr_to_rust_with_aliases(right, ctx, aliases);
            let rust_op = match op.as_str() {
                "and" => "&&",
                "or" => "||",
                o => o,
            };
            format!("({l} {rust_op} {r})")
        }
        CodecExpr::Unary { op, operand } => {
            let o = expr_to_rust_with_aliases(operand, ctx, aliases);
            format!("({op}{o})")
        }
        CodecExpr::Coalesce {
            expr: e,
            default: d,
        } => {
            let d_str = expr_to_rust_with_aliases(d, ctx, aliases);
            if let CodecExpr::ValueRef { reference } = e.as_ref() {
                let raw_name = extract_field_name(&reference.value_id);
                if let Some(alias) = resolve_field_alias(raw_name, aliases) {
                    format!("{alias}.unwrap_or({d_str})")
                } else {
                    let name = rust_ident(raw_name);
                    let prefix = ctx.field_prefix();
                    format!("{prefix}{name}.unwrap_or({d_str})")
                }
            } else {
                let e_str = expr_to_rust_with_aliases(e, ctx, aliases);
                format!("{e_str}.unwrap_or({d_str})")
            }
        }
        CodecExpr::Subscript { base, index } => {
            let b = expr_to_rust_with_aliases(base, ctx, aliases);
            let i = expr_to_rust_with_aliases(index, ctx, aliases);
            format!("{b}[{i}]")
        }
        CodecExpr::InState { .. }
        | CodecExpr::StateConstructor { .. }
        | CodecExpr::Fill { .. }
        | CodecExpr::Slice { .. }
        | CodecExpr::All { .. } => unreachable!("SM expression in non-SM context"),
    }
}

/// Convert a CodecExpr to a Rust expression string.
pub fn expr_to_rust(expr: &CodecExpr, ctx: &ExprContext) -> String {
    expr_to_rust_with_aliases(expr, ctx, &[])
}

pub fn expr_to_rust_with_field_aliases(
    expr: &CodecExpr,
    ctx: &ExprContext,
    aliases: &[(&str, &str)],
) -> String {
    expr_to_rust_with_aliases(expr, ctx, aliases)
}

/// Check whether a CodecExpr is known to produce a boolean result.
fn expr_is_boolean(expr: &CodecExpr) -> bool {
    match expr {
        CodecExpr::Binary { op, .. } => matches!(
            op.as_str(),
            "==" | "!=" | "<" | ">" | "<=" | ">=" | "and" | "or" | "&&" | "||"
        ),
        CodecExpr::Unary { op, .. } => op == "!",
        CodecExpr::Literal {
            value: LiteralValue::Bool(_),
        } => true,
        _ => false,
    }
}

/// Convert a CodecExpr to a Rust boolean expression.
/// If the expression is not inherently boolean (e.g. bitwise `&`),
/// wraps it with `!= 0` so Rust's `if` is satisfied.
pub fn expr_to_rust_bool(expr: &CodecExpr, ctx: &ExprContext) -> String {
    let s = expr_to_rust(expr, ctx);
    if expr_is_boolean(expr) {
        s
    } else {
        format!("({s}) != 0")
    }
}

pub fn expr_to_rust_bool_with_field_aliases(
    expr: &CodecExpr,
    ctx: &ExprContext,
    aliases: &[(&str, &str)],
) -> String {
    let s = expr_to_rust_with_aliases(expr, ctx, aliases);
    if expr_is_boolean(expr) {
        s
    } else {
        format!("({s}) != 0")
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
