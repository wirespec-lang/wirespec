// crates/wirespec-backend-c/src/parse_emit.rs
//
// Per-field parse code generation (cursor reads, bitgroups, conditionals, arrays).

use wirespec_codec::ir::*;

use crate::expr::{ExprContext, expr_to_c};
use crate::names::*;
use crate::type_map::*;

/// Bundles the "threading" parameters used throughout field-parse emission,
/// keeping individual function signatures under the clippy arg-count limit.
struct FieldParseCtx<'a> {
    prefix: &'a str,
    indent: &'a str,
    struct_prefix: &'a str,
    expr_ctx: &'a ExprContext,
}

/// Returns true if the wire type is a signed integer that needs a cast
/// when passed to the unsigned cursor-read / write functions.
fn needs_signed_cast(wt: &WireType) -> bool {
    matches!(
        wt,
        WireType::I8 | WireType::I16 | WireType::I32 | WireType::I64
    )
}

/// Returns the unsigned C type corresponding to a signed wire type.
fn unsigned_c_type(wt: &WireType) -> &'static str {
    match wt {
        WireType::I8 => "uint8_t",
        WireType::I16 => "uint16_t",
        WireType::I32 => "uint32_t",
        WireType::I64 => "uint64_t",
        _ => unreachable!(),
    }
}

/// Emit parse body for a list of items (fields, derived, requires).
///
/// `expr_ctx` controls how field references are resolved in expressions.
/// For top-level structs, pass `None` (uses `ExprContext::Parse`).
/// For variant fields, pass a `CapsuleVariantParse` context.
pub fn emit_parse_items(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    prefix: &str,
    indent: &str,
    struct_prefix: &str,
) {
    emit_parse_items_with_ctx(
        out,
        fields,
        items,
        prefix,
        indent,
        struct_prefix,
        &ExprContext::Parse,
    );
}

/// Like `emit_parse_items` but with an explicit `ExprContext` for expression evaluation.
pub fn emit_parse_items_with_ctx(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    prefix: &str,
    indent: &str,
    struct_prefix: &str,
    expr_ctx: &ExprContext,
) {
    // Track which bitgroups we've already emitted
    let mut emitted_bitgroups: Vec<u32> = Vec::new();
    let ctx = FieldParseCtx {
        prefix,
        indent,
        struct_prefix,
        expr_ctx,
    };

    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    emit_field_parse_with_ctx(out, f, fields, &ctx, &mut emitted_bitgroups);
                }
            }
            CodecItem::Derived(d) => {
                let expr_str = expr_to_c(&d.expr, expr_ctx);
                out.push_str(&format!(
                    "{indent}{struct_prefix}{} = {expr_str};\n",
                    d.name
                ));
            }
            CodecItem::Require(r) => {
                let expr_str = expr_to_c(&r.expr, expr_ctx);
                out.push_str(&format!(
                    "{indent}if (!({expr_str})) return WIRESPEC_ERR_CONSTRAINT;\n"
                ));
            }
        }
    }
}

fn emit_field_parse(
    out: &mut String,
    f: &CodecField,
    all_fields: &[CodecField],
    prefix: &str,
    indent: &str,
    struct_prefix: &str,
    emitted_bitgroups: &mut Vec<u32>,
) {
    let ctx = FieldParseCtx {
        prefix,
        indent,
        struct_prefix,
        expr_ctx: &ExprContext::Parse,
    };
    emit_field_parse_with_ctx(out, f, all_fields, &ctx, emitted_bitgroups);
}

fn emit_field_parse_with_ctx(
    out: &mut String,
    f: &CodecField,
    all_fields: &[CodecField],
    ctx: &FieldParseCtx<'_>,
    emitted_bitgroups: &mut Vec<u32>,
) {
    let prefix = ctx.prefix;
    let indent = ctx.indent;
    let struct_prefix = ctx.struct_prefix;
    let expr_ctx = ctx.expr_ctx;
    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let read_fn = cursor_read_fn(&f.wire_type, f.endianness);
            if needs_signed_cast(&f.wire_type) {
                let utype = unsigned_c_type(&f.wire_type);
                out.push_str(&format!(
                    "{indent}r = {read_fn}(cur, ({utype} *)&{struct_prefix}{});\n",
                    f.name
                ));
            } else {
                out.push_str(&format!(
                    "{indent}r = {read_fn}(cur, &{struct_prefix}{});\n",
                    f.name
                ));
            }
            out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
        }
        FieldStrategy::BitGroup => {
            if let Some(ref bg) = f.bitgroup_member
                && !emitted_bitgroups.contains(&bg.group_id)
            {
                emitted_bitgroups.push(bg.group_id);
                emit_bitgroup_parse(out, f, all_fields, prefix, indent, struct_prefix, bg);
            }
            // If already emitted, this field was handled as part of the group
        }
        FieldStrategy::BytesFixed => {
            if let Some(BytesSpec::Fixed { size }) = &f.bytes_spec {
                out.push_str(&format!(
                    "{indent}r = wirespec_cursor_read_bytes(cur, {size}, &{struct_prefix}{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
            }
        }
        FieldStrategy::BytesLength => {
            if let Some(BytesSpec::Length { ref expr }) = f.bytes_spec {
                let len_expr = expr_to_c(expr, expr_ctx);
                out.push_str(&format!(
                    "{indent}r = wirespec_cursor_read_bytes(cur, (size_t){len_expr}, &{struct_prefix}{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
            }
        }
        FieldStrategy::BytesRemaining => {
            out.push_str(&format!(
                "{indent}{struct_prefix}{}.ptr = cur->base + cur->pos;\n",
                f.name
            ));
            out.push_str(&format!(
                "{indent}{struct_prefix}{}.len = wirespec_cursor_remaining(cur);\n",
                f.name
            ));
            out.push_str(&format!("{indent}cur->pos = cur->len;\n"));
        }
        FieldStrategy::BytesLor => {
            if let Some(BytesSpec::LengthOrRemaining { ref expr }) = f.bytes_spec {
                // Check if the length field is available (has_X)
                let len_expr = expr_to_c(expr, expr_ctx);
                // We need to figure out which field is the length source
                // For LOR, the condition is typically based on whether a field has a value
                // The expr gives us the length expression
                let value_id = get_value_ref_id(expr);
                let field_name = crate::expr::extract_field_name(&value_id);
                out.push_str(&format!(
                    "{indent}if ({struct_prefix}has_{field_name}) {{\n"
                ));
                out.push_str(&format!(
                    "{indent}    r = wirespec_cursor_read_bytes(cur, (size_t){len_expr}, &{struct_prefix}{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
                out.push_str(&format!("{indent}}} else {{\n"));
                out.push_str(&format!(
                    "{indent}    {struct_prefix}{}.ptr = cur->base + cur->pos;\n",
                    f.name
                ));
                out.push_str(&format!(
                    "{indent}    {struct_prefix}{}.len = wirespec_cursor_remaining(cur);\n",
                    f.name
                ));
                out.push_str(&format!("{indent}    cur->pos = cur->len;\n"));
                out.push_str(&format!("{indent}}}\n"));
            }
        }
        FieldStrategy::Conditional => {
            if let Some(ref cond) = f.condition {
                let cond_str = expr_to_c(cond, expr_ctx);
                out.push_str(&format!("{indent}{struct_prefix}has_{} = false;\n", f.name));
                out.push_str(&format!("{indent}if ({cond_str}) {{\n"));

                // Parse the inner field
                let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
                // Check if the inner type is a struct reference
                if f.ref_type_name.is_some()
                    && matches!(
                        inner_wt,
                        WireType::Struct(_) | WireType::VarInt | WireType::ContVarInt
                    )
                {
                    let ref_name = f.ref_type_name.as_ref().unwrap();
                    let parse_fn = c_func_name(prefix, ref_name, "parse_cursor");
                    out.push_str(&format!(
                        "{indent}    r = {parse_fn}(cur, &{struct_prefix}{});\n",
                        f.name
                    ));
                } else {
                    let read_fn = cursor_read_fn(inner_wt, f.endianness);
                    out.push_str(&format!(
                        "{indent}    r = {read_fn}(cur, &{struct_prefix}{});\n",
                        f.name
                    ));
                }
                out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
                out.push_str(&format!(
                    "{indent}    {struct_prefix}has_{} = true;\n",
                    f.name
                ));
                out.push_str(&format!("{indent}}}\n"));
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                emit_array_parse_with_ctx(out, f, arr, prefix, indent, struct_prefix, expr_ctx);
            }
        }
        FieldStrategy::Struct => {
            if let Some(ref ref_name) = f.ref_type_name {
                let parse_fn = c_func_name(prefix, ref_name, "parse_cursor");
                out.push_str(&format!(
                    "{indent}r = {parse_fn}(cur, &{struct_prefix}{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
            }
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = f.ref_type_name {
                let parse_fn = c_func_name(prefix, ref_name, "parse_cursor");
                out.push_str(&format!(
                    "{indent}r = {parse_fn}(cur, &{struct_prefix}{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
            }
        }
    }
}

fn emit_bitgroup_parse(
    out: &mut String,
    _first_field: &CodecField,
    all_fields: &[CodecField],
    _prefix: &str,
    indent: &str,
    struct_prefix: &str,
    bg: &BitgroupMember,
) {
    let group_id = bg.group_id;
    let total_bits = bg.total_bits;
    let container_type = bitgroup_c_type(total_bits);
    let read_fn = bitgroup_read_fn(total_bits, bg.group_endianness);
    let var_name = format!("_bg{group_id}");

    out.push_str(&format!("{indent}{{\n"));
    out.push_str(&format!("{indent}    {container_type} {var_name};\n"));
    out.push_str(&format!("{indent}    r = {read_fn}(cur, &{var_name});\n"));
    out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));

    // Extract all fields in this group
    for f in all_fields {
        if let Some(ref mbg) = f.bitgroup_member
            && mbg.group_id == group_id
        {
            let mask = (1u64 << mbg.member_width_bits) - 1;
            let shift = mbg.member_offset_bits;
            let ctype = wire_type_to_c(&f.wire_type, "");
            // Use the simplest unsigned type for the cast
            let cast_type = if ctype.contains("int") {
                &ctype
            } else {
                "uint8_t"
            };
            out.push_str(&format!(
                    "{indent}    {struct_prefix}{} = ({cast_type})(({var_name} >> {shift}) & 0x{mask:x});\n",
                    f.name
                ));
        }
    }

    out.push_str(&format!("{indent}}}\n"));
}

fn emit_array_parse_with_ctx(
    out: &mut String,
    f: &CodecField,
    arr: &ArraySpec,
    prefix: &str,
    indent: &str,
    struct_prefix: &str,
    expr_ctx: &ExprContext,
) {
    if let Some(ref count_expr) = arr.count_expr {
        let count_str = expr_to_c(count_expr, expr_ctx);
        let max_elems = f.max_elements.unwrap_or(256);

        out.push_str(&format!(
            "{indent}{struct_prefix}{}_count = (uint32_t){count_str};\n",
            f.name
        ));
        out.push_str(&format!(
            "{indent}if ({struct_prefix}{}_count > {max_elems}) return WIRESPEC_ERR_CAPACITY;\n",
            f.name
        ));
        out.push_str(&format!(
            "{indent}for (uint32_t _i = 0; _i < {struct_prefix}{}_count; _i++) {{\n",
            f.name
        ));

        // Parse each element
        match arr.element_strategy {
            FieldStrategy::Primitive => {
                let read_fn = cursor_read_fn(&arr.element_wire_type, None);
                out.push_str(&format!(
                    "{indent}    r = {read_fn}(cur, &{struct_prefix}{}[_i]);\n",
                    f.name
                ));
                out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
            }
            FieldStrategy::Struct => {
                if let Some(ref ref_name) = arr.element_ref_type_name {
                    let parse_fn = c_func_name(prefix, ref_name, "parse_cursor");
                    out.push_str(&format!(
                        "{indent}    r = {parse_fn}(cur, &{struct_prefix}{}[_i]);\n",
                        f.name
                    ));
                    out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
                }
            }
            _ => {
                out.push_str(&format!(
                    "{indent}    /* unsupported array element strategy */\n"
                ));
            }
        }

        out.push_str(&format!("{indent}}}\n"));
    } else {
        // [T; fill] — parse until cursor exhausted
        let max_elems = f.max_elements.unwrap_or(256);

        // Determine the cursor variable to use
        let cur_var = if let Some(ref within_expr) = arr.within_expr {
            // [T; fill] within expr — create a sub-cursor bounded by the expression
            let within_c = expr_to_c(within_expr, expr_ctx);
            out.push_str(&format!("{indent}{{\n"));
            out.push_str(&format!("{indent}    wirespec_cursor_t _arr_sub;\n"));
            out.push_str(&format!(
                "{indent}    r = wirespec_cursor_sub(cur, (size_t){within_c}, &_arr_sub);\n"
            ));
            out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
            "&_arr_sub".to_string()
        } else {
            "cur".to_string()
        };

        out.push_str(&format!(
            "{indent}    {struct_prefix}{}_count = 0;\n",
            f.name
        ));
        out.push_str(&format!(
            "{indent}    while (wirespec_cursor_remaining({cur_var}) > 0) {{\n",
        ));
        out.push_str(&format!(
            "{indent}        if ({struct_prefix}{}_count >= {max_elems}) return WIRESPEC_ERR_CAPACITY;\n",
            f.name
        ));

        match arr.element_strategy {
            FieldStrategy::Primitive => {
                let read_fn = cursor_read_fn(&arr.element_wire_type, None);
                out.push_str(&format!(
                    "{indent}        r = {read_fn}({cur_var}, &{struct_prefix}{}[{struct_prefix}{}_count]);\n",
                    f.name, f.name
                ));
                out.push_str(&format!(
                    "{indent}        if (r != WIRESPEC_OK) return r;\n"
                ));
            }
            FieldStrategy::Struct => {
                if let Some(ref ref_name) = arr.element_ref_type_name {
                    let parse_fn = c_func_name(prefix, ref_name, "parse_cursor");
                    out.push_str(&format!(
                        "{indent}        r = {parse_fn}({cur_var}, &{struct_prefix}{}[{struct_prefix}{}_count]);\n",
                        f.name, f.name
                    ));
                    out.push_str(&format!(
                        "{indent}        if (r != WIRESPEC_OK) return r;\n"
                    ));
                }
            }
            _ => {
                out.push_str(&format!(
                    "{indent}        /* unsupported fill array element strategy */\n"
                ));
            }
        }

        out.push_str(&format!(
            "{indent}        {struct_prefix}{}_count++;\n",
            f.name
        ));
        out.push_str(&format!("{indent}    }}\n"));

        if arr.within_expr.is_some() {
            out.push_str(&format!("{indent}}}\n"));
        }
    }
}

/// Extract value_id from a CodecExpr if it's a ValueRef, or return empty string.
fn get_value_ref_id(expr: &CodecExpr) -> String {
    match expr {
        CodecExpr::ValueRef { reference } => reference.value_id.clone(),
        _ => String::new(),
    }
}

/// Emit parse for frame variants (the switch dispatch).
pub fn emit_frame_parse_body(out: &mut String, frame: &CodecFrame, prefix: &str, indent: &str) {
    let struct_prefix = "out->";

    // Read the tag field
    let tag_read_fn = cursor_read_fn(&frame.tag.wire_type, frame.tag.endianness);

    match &frame.tag.wire_type {
        WireType::VarInt | WireType::ContVarInt => {
            if let Some(ref ref_name) = frame.tag.ref_type_name {
                let parse_fn = c_func_name(prefix, ref_name, "parse_cursor");
                out.push_str(&format!("{indent}uint64_t _tag_val;\n"));
                out.push_str(&format!("{indent}r = {parse_fn}(cur, &_tag_val);\n"));
                out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
            } else {
                // Fallback: read as u8 if no ref_type_name (should not happen in practice)
                out.push_str(&format!("{indent}uint64_t _tag_val;\n"));
                out.push_str(&format!(
                    "{indent}r = wirespec_cursor_read_u8(cur, (uint8_t *)&_tag_val);\n"
                ));
                out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
            }
        }
        _ => {
            // Primitive tag
            let tag_ctype = wire_type_to_c(&frame.tag.wire_type, prefix);
            out.push_str(&format!("{indent}{tag_ctype} _tag_val;\n"));
            out.push_str(&format!("{indent}r = {tag_read_fn}(cur, &_tag_val);\n"));
            out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
        }
    }

    // Store raw tag value
    out.push_str(&format!("{indent}{struct_prefix}frame_type = _tag_val;\n"));

    // Switch on tag value
    out.push_str(&format!("{indent}switch (_tag_val) {{\n"));
    let mut has_wildcard = false;
    for variant in &frame.variants {
        if matches!(variant.pattern, VariantPattern::Wildcard) {
            has_wildcard = true;
        }
        emit_variant_case(out, variant, frame, prefix, indent, struct_prefix);
    }
    if !has_wildcard {
        out.push_str(&format!("{indent}    default:\n"));
        out.push_str(&format!(
            "{indent}        return WIRESPEC_ERR_INVALID_TAG;\n"
        ));
    }
    out.push_str(&format!("{indent}}}\n"));
}

fn emit_variant_case(
    out: &mut String,
    variant: &CodecVariantScope,
    frame: &CodecFrame,
    prefix: &str,
    indent: &str,
    struct_prefix: &str,
) {
    let tag_val = c_frame_tag_value(prefix, &frame.name, &variant.name);
    let vname = to_snake_case(&variant.name);
    let inner_indent = format!("{indent}        ");

    match &variant.pattern {
        VariantPattern::Exact { value } => {
            out.push_str(&format!("{indent}    case {value}:\n"));
        }
        VariantPattern::RangeInclusive { start, end } => {
            let count = end.saturating_sub(*start).saturating_add(1);
            if count > 4096 {
                panic!(
                    "range pattern {start}..={end} too large ({count} values) for switch expansion"
                );
            }
            for v in *start..=*end {
                out.push_str(&format!("{indent}    case {v}:\n"));
            }
        }
        VariantPattern::Wildcard => {
            out.push_str(&format!("{indent}    default:\n"));
        }
    }

    out.push_str(&format!("{inner_indent}{struct_prefix}tag = {tag_val};\n"));

    // Parse variant fields into the union member
    let variant_prefix = format!("{struct_prefix}value.{vname}.");

    // Build ExprContext: frame tag field uses `out->` prefix,
    // variant-local fields use `out->value.{variant}.` prefix.
    let header_field_names = vec![frame.tag.field_name.clone()];
    let expr_ctx = ExprContext::CapsuleVariantParse {
        variant_prefix: variant_prefix.clone(),
        header_field_names,
    };

    emit_parse_items_with_ctx(
        out,
        &variant.fields,
        &variant.items,
        prefix,
        &inner_indent,
        &variant_prefix,
        &expr_ctx,
    );

    out.push_str(&format!("{inner_indent}break;\n"));
}

/// Emit parse for capsule (header fields + sub-cursor dispatch).
pub fn emit_capsule_parse_body(
    out: &mut String,
    capsule: &CodecCapsule,
    prefix: &str,
    indent: &str,
) {
    let struct_prefix = "out->";

    // Parse header fields
    let mut emitted_bitgroups: Vec<u32> = Vec::new();
    for item in &capsule.header_items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = capsule
                    .header_fields
                    .iter()
                    .find(|f| &f.field_id == field_id)
                {
                    emit_field_parse(
                        out,
                        f,
                        &capsule.header_fields,
                        prefix,
                        indent,
                        struct_prefix,
                        &mut emitted_bitgroups,
                    );
                }
            }
            CodecItem::Derived(d) => {
                let expr_str = expr_to_c(&d.expr, &ExprContext::Parse);
                out.push_str(&format!(
                    "{indent}{struct_prefix}{} = {expr_str};\n",
                    d.name
                ));
            }
            CodecItem::Require(r) => {
                let expr_str = expr_to_c(&r.expr, &ExprContext::Parse);
                out.push_str(&format!(
                    "{indent}if (!({expr_str})) return WIRESPEC_ERR_CONSTRAINT;\n"
                ));
            }
        }
    }

    // Create sub-cursor for "within" field (always uses the within_field, NOT the tag expr)
    let within_expr = format!("(size_t)({struct_prefix}{})", capsule.within_field);

    out.push_str(&format!("{indent}wirespec_cursor_t sub;\n"));
    out.push_str(&format!(
        "{indent}r = wirespec_cursor_sub(cur, {within_expr}, &sub);\n"
    ));
    out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));

    // Determine the tag expression for switching
    // For expr-based tags: use the expression (e.g., "(out->type_and_flags >> 4)")
    // For field-based tags: use the field directly
    let tag_switch_expr = if let Some(ref expr) = capsule.tag_expr {
        expr_to_c(expr, &ExprContext::Parse)
    } else {
        format!("{struct_prefix}{}", capsule.tag.field_name)
    };

    // Switch on tag
    out.push_str(&format!("{indent}switch ({tag_switch_expr}) {{\n"));
    for variant in &capsule.variants {
        emit_capsule_variant_case(out, variant, capsule, prefix, indent, struct_prefix);
    }
    out.push_str(&format!("{indent}}}\n"));

    // Check trailing data in sub-cursor
    out.push_str(&format!(
        "{indent}if (wirespec_cursor_remaining(&sub) != 0) return WIRESPEC_ERR_TRAILING_DATA;\n"
    ));
}

fn emit_capsule_variant_case(
    out: &mut String,
    variant: &CodecVariantScope,
    capsule: &CodecCapsule,
    prefix: &str,
    indent: &str,
    struct_prefix: &str,
) {
    let tag_val = c_frame_tag_value(prefix, &capsule.name, &variant.name);
    let vname = to_snake_case(&variant.name);
    let inner_indent = format!("{indent}        ");

    match &variant.pattern {
        VariantPattern::Exact { value } => {
            out.push_str(&format!("{indent}    case {value}:\n"));
        }
        VariantPattern::RangeInclusive { start, end } => {
            let count = end.saturating_sub(*start).saturating_add(1);
            if count > 4096 {
                panic!(
                    "range pattern {start}..={end} too large ({count} values) for switch expansion"
                );
            }
            for v in *start..=*end {
                out.push_str(&format!("{indent}    case {v}:\n"));
            }
        }
        VariantPattern::Wildcard => {
            out.push_str(&format!("{indent}    default:\n"));
        }
    }

    out.push_str(&format!("{inner_indent}{struct_prefix}tag = {tag_val};\n"));

    // Parse variant fields from sub-cursor
    let variant_prefix = format!("{struct_prefix}value.{vname}.");

    // Build ExprContext that resolves header fields to `out->` and
    // variant-local fields to `out->value.{variant}.`
    let header_field_names: Vec<String> = capsule
        .header_fields
        .iter()
        .map(|f| f.name.clone())
        .collect();
    let expr_ctx = ExprContext::CapsuleVariantParse {
        variant_prefix: variant_prefix.clone(),
        header_field_names,
    };

    for item in &variant.items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = variant.fields.iter().find(|f| &f.field_id == field_id) {
                    emit_capsule_field_parse(
                        out,
                        f,
                        &variant.fields,
                        prefix,
                        &inner_indent,
                        &variant_prefix,
                        &expr_ctx,
                    );
                }
            }
            CodecItem::Derived(d) => {
                let expr_str = expr_to_c(&d.expr, &expr_ctx);
                out.push_str(&format!(
                    "{inner_indent}{variant_prefix}{} = {expr_str};\n",
                    d.name
                ));
            }
            CodecItem::Require(r) => {
                let expr_str = expr_to_c(&r.expr, &expr_ctx);
                out.push_str(&format!(
                    "{inner_indent}if (!({expr_str})) return WIRESPEC_ERR_CONSTRAINT;\n"
                ));
            }
        }
    }

    out.push_str(&format!("{inner_indent}break;\n"));
}

/// Like emit_field_parse but uses `&sub` cursor for capsule payload variants.
fn emit_capsule_field_parse(
    out: &mut String,
    f: &CodecField,
    all_fields: &[CodecField],
    prefix: &str,
    indent: &str,
    struct_prefix: &str,
    expr_ctx: &ExprContext,
) {
    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let read_fn = cursor_read_fn(&f.wire_type, f.endianness);
            if needs_signed_cast(&f.wire_type) {
                let utype = unsigned_c_type(&f.wire_type);
                out.push_str(&format!(
                    "{indent}r = {read_fn}(&sub, ({utype} *)&{struct_prefix}{});\n",
                    f.name
                ));
            } else {
                out.push_str(&format!(
                    "{indent}r = {read_fn}(&sub, &{struct_prefix}{});\n",
                    f.name
                ));
            }
            out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
        }
        FieldStrategy::Struct => {
            if let Some(ref ref_name) = f.ref_type_name {
                let parse_fn = c_func_name(prefix, ref_name, "parse_cursor");
                out.push_str(&format!(
                    "{indent}r = {parse_fn}(&sub, &{struct_prefix}{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
            }
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = f.ref_type_name {
                let parse_fn = c_func_name(prefix, ref_name, "parse_cursor");
                out.push_str(&format!(
                    "{indent}r = {parse_fn}(&sub, &{struct_prefix}{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
            }
        }
        FieldStrategy::Conditional => {
            if let Some(ref cond) = f.condition {
                let cond_str = expr_to_c(cond, expr_ctx);
                out.push_str(&format!("{indent}{struct_prefix}has_{} = false;\n", f.name));
                out.push_str(&format!("{indent}if ({cond_str}) {{\n"));

                // Determine the inner wire type
                let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);

                // Check if the inner type is a struct reference
                if f.ref_type_name.is_some() && matches!(inner_wt, WireType::Struct(_)) {
                    let ref_name = f.ref_type_name.as_ref().unwrap();
                    let parse_fn = c_func_name(prefix, ref_name, "parse_cursor");
                    out.push_str(&format!(
                        "{indent}    r = {parse_fn}(&sub, &{struct_prefix}{});\n",
                        f.name
                    ));
                } else {
                    let read_fn = cursor_read_fn(inner_wt, f.endianness);
                    out.push_str(&format!(
                        "{indent}    r = {read_fn}(&sub, &{struct_prefix}{});\n",
                        f.name
                    ));
                }
                out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
                out.push_str(&format!(
                    "{indent}    {struct_prefix}has_{} = true;\n",
                    f.name
                ));
                out.push_str(&format!("{indent}}}\n"));
            }
        }
        FieldStrategy::BytesRemaining => {
            out.push_str(&format!(
                "{indent}{struct_prefix}{}.ptr = sub.base + sub.pos;\n",
                f.name
            ));
            out.push_str(&format!(
                "{indent}{struct_prefix}{}.len = wirespec_cursor_remaining(&sub);\n",
                f.name
            ));
            out.push_str(&format!("{indent}sub.pos = sub.len;\n"));
        }
        FieldStrategy::BytesFixed => {
            if let Some(BytesSpec::Fixed { size }) = &f.bytes_spec {
                out.push_str(&format!(
                    "{indent}r = wirespec_cursor_read_bytes(&sub, {size}, &{struct_prefix}{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
            }
        }
        FieldStrategy::BytesLength => {
            if let Some(BytesSpec::Length { ref expr }) = f.bytes_spec {
                let len_expr = expr_to_c(expr, expr_ctx);
                out.push_str(&format!(
                    "{indent}r = wirespec_cursor_read_bytes(&sub, (size_t){len_expr}, &{struct_prefix}{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
            }
        }
        _ => {
            // For other strategies in capsule context, emit using the regular function
            // with the correct expression context, then replace `cur` references with
            // `&sub` to use the sub-cursor.
            let mut tmp = String::new();
            let mut dummy_bg = Vec::new();
            let ctx = FieldParseCtx {
                prefix,
                indent,
                struct_prefix,
                expr_ctx,
            };
            emit_field_parse_with_ctx(&mut tmp, f, all_fields, &ctx, &mut dummy_bg);
            // Replace cursor variable: (cur, -> (&sub, and cur-> -> sub.
            let tmp = tmp.replace("(cur, ", "(&sub, ");
            let tmp = tmp.replace("cur->", "sub.");
            out.push_str(&tmp);
        }
    }
}
