// crates/wirespec-backend-rust/src/expr.rs
//
// CodecExpr -> Rust expression string conversion.

use wirespec_codec::ir::*;

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

/// Convert a CodecExpr to a Rust expression string.
pub fn expr_to_rust(expr: &CodecExpr, ctx: &ExprContext) -> String {
    match expr {
        CodecExpr::ValueRef { reference } => {
            match reference.kind {
                ValueRefKind::Field | ValueRefKind::Derived => {
                    let name = extract_field_name(&reference.value_id);
                    let prefix = ctx.field_prefix();
                    format!("{prefix}{name}")
                }
                ValueRefKind::Const => {
                    // Constants use UPPER_SNAKE_CASE
                    reference.value_id.to_uppercase()
                }
            }
        }
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
            let l = expr_to_rust(left, ctx);
            let r = expr_to_rust(right, ctx);
            let rust_op = match op.as_str() {
                "and" => "&&",
                "or" => "||",
                o => o,
            };
            format!("({l} {rust_op} {r})")
        }
        CodecExpr::Unary { op, operand } => {
            let o = expr_to_rust(operand, ctx);
            format!("({op}{o})")
        }
        CodecExpr::Coalesce {
            expr: e,
            default: d,
        } => {
            let d_str = expr_to_rust(d, ctx);
            // For optional field coalesce: field.unwrap_or(default)
            if let CodecExpr::ValueRef { reference } = e.as_ref() {
                let name = extract_field_name(&reference.value_id);
                let prefix = ctx.field_prefix();
                format!("{prefix}{name}.unwrap_or({d_str})")
            } else {
                let e_str = expr_to_rust(e, ctx);
                format!("{e_str}.unwrap_or({d_str})")
            }
        }
        CodecExpr::Subscript { base, index } => {
            let b = expr_to_rust(base, ctx);
            let i = expr_to_rust(index, ctx);
            format!("{b}[{i}]")
        }
        CodecExpr::InState { .. }
        | CodecExpr::StateConstructor { .. }
        | CodecExpr::Fill { .. }
        | CodecExpr::Slice { .. }
        | CodecExpr::All { .. } => "/* unsupported expr */".into(),
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
