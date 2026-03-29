// crates/wirespec-backend-rust/src/parse_emit.rs
//
// Per-field parse code generation for Rust (cursor reads, bitgroups, conditionals, arrays).

use wirespec_codec::ir::*;

use crate::expr::{ExprContext, expr_to_rust, extract_field_name};
use crate::names::to_pascal_case;
use crate::type_map::*;

/// Emit parse body for a list of items (fields, derived, requires).
/// Each field becomes a local `let` binding.
pub fn emit_parse_items(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    emit_field_parse(out, f, fields, indent, &mut emitted_bitgroups);
                }
            }
            CodecItem::Derived(d) => {
                let expr_str = expr_to_rust(&d.expr, &ExprContext::Parse);
                out.push_str(&format!("{indent}let {} = {expr_str};\n", d.name));
            }
            CodecItem::Require(r) => {
                let expr_str = expr_to_rust(&r.expr, &ExprContext::Parse);
                out.push_str(&format!(
                    "{indent}if !({expr_str}) {{ return Err(Error::Constraint); }}\n"
                ));
            }
        }
    }
}

fn emit_field_parse(
    out: &mut String,
    f: &CodecField,
    all_fields: &[CodecField],
    indent: &str,
    emitted_bitgroups: &mut Vec<u32>,
) {
    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let read_method = cursor_read_method(&f.wire_type, f.endianness);
            out.push_str(&format!("{indent}let {} = cur.{read_method}()?;\n", f.name));
        }
        FieldStrategy::BitGroup => {
            if let Some(ref bg) = f.bitgroup_member
                && !emitted_bitgroups.contains(&bg.group_id)
            {
                emitted_bitgroups.push(bg.group_id);
                emit_bitgroup_parse(out, all_fields, indent, bg);
            }
        }
        FieldStrategy::BytesFixed => {
            if let Some(BytesSpec::Fixed { size }) = &f.bytes_spec {
                out.push_str(&format!(
                    "{indent}let {} = cur.read_bytes({size})?;\n",
                    f.name
                ));
            }
        }
        FieldStrategy::BytesLength => {
            if let Some(BytesSpec::Length { ref expr }) = f.bytes_spec {
                let len_expr = expr_to_rust(expr, &ExprContext::Parse);
                out.push_str(&format!(
                    "{indent}let {} = cur.read_bytes({len_expr} as usize)?;\n",
                    f.name
                ));
            }
        }
        FieldStrategy::BytesRemaining => {
            out.push_str(&format!("{indent}let {} = cur.read_remaining();\n", f.name));
        }
        FieldStrategy::BytesLor => {
            if let Some(BytesSpec::LengthOrRemaining { ref expr }) = f.bytes_spec {
                let len_expr = expr_to_rust(expr, &ExprContext::Parse);
                let value_id = get_value_ref_id(expr);
                let field_name = extract_field_name(&value_id);
                out.push_str(&format!(
                    "{indent}let {} = if let Some(l) = {field_name} {{ cur.read_bytes(l as usize)? }} else {{ cur.read_remaining() }};\n",
                    f.name
                ));
                let _ = len_expr; // len_expr covered by the unwrap pattern
            }
        }
        FieldStrategy::Conditional => {
            if let Some(ref cond) = f.condition {
                let cond_str = expr_to_rust(cond, &ExprContext::Parse);
                let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);

                match inner_wt {
                    WireType::Bytes => {
                        // Bytes conditional
                        if let Some(ref bytes_spec) = f.bytes_spec {
                            match bytes_spec {
                                BytesSpec::Fixed { size } => {
                                    out.push_str(&format!(
                                        "{indent}let {} = if {cond_str} {{ Some(cur.read_bytes({size})?) }} else {{ None }};\n",
                                        f.name
                                    ));
                                }
                                BytesSpec::Remaining => {
                                    out.push_str(&format!(
                                        "{indent}let {} = if {cond_str} {{ Some(cur.read_remaining()) }} else {{ None }};\n",
                                        f.name
                                    ));
                                }
                                _ => {
                                    out.push_str(&format!(
                                        "{indent}let {} = if {cond_str} {{ Some(cur.read_remaining()) }} else {{ None }};\n",
                                        f.name
                                    ));
                                }
                            }
                        } else {
                            out.push_str(&format!(
                                "{indent}let {} = if {cond_str} {{ Some(cur.read_remaining()) }} else {{ None }};\n",
                                f.name
                            ));
                        }
                    }
                    WireType::Struct(ref_name) => {
                        let type_name = to_pascal_case(ref_name);
                        out.push_str(&format!(
                            "{indent}let {} = if {cond_str} {{ Some({type_name}::parse(cur)?) }} else {{ None }};\n",
                            f.name
                        ));
                    }
                    _ => {
                        let read_method = cursor_read_method(inner_wt, f.endianness);
                        out.push_str(&format!(
                            "{indent}let {} = if {cond_str} {{ Some(cur.{read_method}()?) }} else {{ None }};\n",
                            f.name
                        ));
                    }
                }
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                emit_array_parse(out, f, arr, indent);
            }
        }
        FieldStrategy::Struct => {
            if let Some(ref ref_name) = f.ref_type_name {
                let type_name = to_pascal_case(ref_name);
                out.push_str(&format!(
                    "{indent}let {} = {type_name}::parse(cur)?;\n",
                    f.name
                ));
            }
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = f.ref_type_name {
                let parse_fn = format!("{}_parse", crate::names::to_snake_case(ref_name));
                out.push_str(&format!("{indent}let {} = {parse_fn}(cur)?;\n", f.name));
            } else {
                // Fallback: read as u64
                out.push_str(&format!("{indent}let {} = cur.read_u64be()?;\n", f.name));
            }
        }
    }
}

fn emit_bitgroup_parse(
    out: &mut String,
    all_fields: &[CodecField],
    indent: &str,
    bg: &BitgroupMember,
) {
    let group_id = bg.group_id;
    let read_method = bitgroup_read_method(bg.total_bits, bg.group_endianness);
    let var_name = format!("_bg{group_id}");

    out.push_str(&format!("{indent}let {var_name} = cur.{read_method}()?;\n"));

    // Extract all fields in this group
    for f in all_fields {
        if let Some(ref mbg) = f.bitgroup_member
            && mbg.group_id == group_id
        {
            let mask = (1u64 << mbg.member_width_bits) - 1;
            let shift = mbg.member_offset_bits;
            let target_type = wire_type_to_rust(&f.wire_type);
            out.push_str(&format!(
                "{indent}let {} = (({var_name} >> {shift}) & 0x{mask:x}) as {target_type};\n",
                f.name
            ));
        }
    }
}

fn emit_array_parse(out: &mut String, f: &CodecField, arr: &ArraySpec, indent: &str) {
    if let Some(ref count_expr) = arr.count_expr {
        let count_str = expr_to_rust(count_expr, &ExprContext::Parse);
        let max_elems = f.max_elements.unwrap_or(256);
        let elem_type = wire_type_to_rust(&arr.element_wire_type);

        out.push_str(&format!(
            "{indent}let {}_count = {count_str} as usize;\n",
            f.name
        ));
        out.push_str(&format!(
            "{indent}if {}_count > {max_elems} {{ return Err(Error::Capacity); }}\n",
            f.name
        ));

        // Initialize the array with defaults
        out.push_str(&format!(
            "{indent}let mut {} = [<{elem_type}>::default(); {max_elems}];\n",
            f.name
        ));

        out.push_str(&format!("{indent}for _i in 0..{}_count {{\n", f.name));

        match arr.element_strategy {
            FieldStrategy::Primitive => {
                let read_method = cursor_read_method(&arr.element_wire_type, None);
                out.push_str(&format!(
                    "{indent}    {}[_i] = cur.{read_method}()?;\n",
                    f.name
                ));
            }
            FieldStrategy::Struct => {
                if let Some(ref ref_name) = arr.element_ref_type_name {
                    let type_name = to_pascal_case(ref_name);
                    out.push_str(&format!(
                        "{indent}    {}[_i] = {type_name}::parse(cur)?;\n",
                        f.name
                    ));
                }
            }
            _ => {
                out.push_str(&format!(
                    "{indent}    /* unsupported array element strategy */\n"
                ));
            }
        }

        out.push_str(&format!("{indent}}}\n"));
    }
}

/// Extract value_id from a CodecExpr if it's a ValueRef, or return empty string.
fn get_value_ref_id(expr: &CodecExpr) -> String {
    match expr {
        CodecExpr::ValueRef { reference } => reference.value_id.clone(),
        _ => String::new(),
    }
}

/// Emit parse for frame variants: reads tag, matches, parses variant fields.
pub fn emit_frame_parse_body(out: &mut String, frame: &CodecFrame, indent: &str) {
    // Read the tag field
    let tag_read_method = cursor_read_method(&frame.tag.wire_type, frame.tag.endianness);
    out.push_str(&format!(
        "{indent}let _tag_val = cur.{tag_read_method}()?;\n"
    ));

    // Match on tag value
    out.push_str(&format!("{indent}let _result = match _tag_val {{\n"));

    for variant in &frame.variants {
        emit_variant_parse_arm(out, variant, indent);
    }

    out.push_str(&format!("{indent}}};\n"));
    out.push_str(&format!("{indent}Ok((_result, _tag_val))\n"));
}

fn emit_variant_parse_arm(out: &mut String, variant: &CodecVariantScope, indent: &str) {
    let variant_name = to_pascal_case(&variant.name);
    let inner_indent = format!("{indent}        ");

    // Pattern
    match &variant.pattern {
        VariantPattern::Exact { value } => {
            out.push_str(&format!("{indent}    {value} => {{\n"));
        }
        VariantPattern::RangeInclusive { start, end } => {
            out.push_str(&format!("{indent}    {start}..={end} => {{\n"));
        }
        VariantPattern::Wildcard => {
            out.push_str(&format!("{indent}    _ => {{\n"));
        }
    }

    if variant.fields.is_empty()
        && variant
            .items
            .iter()
            .all(|i| matches!(i, CodecItem::Require(_)))
    {
        // Handle requires for empty variants
        for item in &variant.items {
            if let CodecItem::Require(r) = item {
                let expr_str = expr_to_rust(&r.expr, &ExprContext::Parse);
                out.push_str(&format!(
                    "{inner_indent}if !({expr_str}) {{ return Err(Error::Constraint); }}\n"
                ));
            }
        }
        out.push_str(&format!("{inner_indent}Self::{variant_name}\n"));
    } else {
        // Parse variant fields
        emit_parse_items(out, &variant.fields, &variant.items, &inner_indent);

        // Construct the variant
        if variant.fields.is_empty() {
            out.push_str(&format!("{inner_indent}Self::{variant_name}\n"));
        } else {
            out.push_str(&format!("{inner_indent}Self::{variant_name} {{\n"));
            // Include all fields and derived items
            for item in &variant.items {
                match item {
                    CodecItem::Field { field_id } => {
                        if let Some(f) = variant.fields.iter().find(|f| &f.field_id == field_id) {
                            // For arrays, include count
                            if f.strategy == FieldStrategy::Array {
                                out.push_str(&format!("{inner_indent}    {},\n", f.name));
                                out.push_str(&format!("{inner_indent}    {}_count,\n", f.name));
                            } else {
                                out.push_str(&format!("{inner_indent}    {},\n", f.name));
                            }
                        }
                    }
                    CodecItem::Derived(d) => {
                        out.push_str(&format!("{inner_indent}    {},\n", d.name));
                    }
                    CodecItem::Require(_) => {}
                }
            }
            out.push_str(&format!("{inner_indent}}}\n"));
        }
    }

    out.push_str(&format!("{indent}    }}\n"));
}

/// Emit parse for capsule (header fields + sub-cursor dispatch).
pub fn emit_capsule_parse_body(out: &mut String, capsule: &CodecCapsule, indent: &str) {
    // Parse header fields
    emit_parse_items(out, &capsule.header_fields, &capsule.header_items, indent);

    // Create sub-cursor for "within" field
    let within_expr = if let Some(ref expr) = capsule.tag_expr {
        expr_to_rust(expr, &ExprContext::Parse)
    } else {
        capsule.within_field.clone()
    };

    out.push_str(&format!(
        "{indent}let mut sub = cur.sub_cursor({within_expr} as usize)?;\n"
    ));

    // Read tag for dispatch
    let tag_field_name = &capsule.tag.field_name;
    out.push_str(&format!("{indent}match {tag_field_name} {{\n"));

    for variant in &capsule.variants {
        emit_capsule_variant_arm(out, variant, indent);
    }

    out.push_str(&format!("{indent}}}\n"));
}

fn emit_capsule_variant_arm(out: &mut String, variant: &CodecVariantScope, indent: &str) {
    let _variant_name = to_pascal_case(&variant.name);
    let inner_indent = format!("{indent}        ");

    match &variant.pattern {
        VariantPattern::Exact { value } => {
            out.push_str(&format!("{indent}    {value} => {{\n"));
        }
        VariantPattern::RangeInclusive { start, end } => {
            out.push_str(&format!("{indent}    {start}..={end} => {{\n"));
        }
        VariantPattern::Wildcard => {
            out.push_str(&format!("{indent}    _ => {{\n"));
        }
    }

    // Parse variant fields using &mut sub cursor
    // We use "sub" instead of "cur" for capsule variant fields
    emit_capsule_variant_parse_items(out, &variant.fields, &variant.items, &inner_indent);

    out.push_str(&format!("{indent}    }}\n"));
}

fn emit_capsule_variant_parse_items(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    emit_capsule_field_parse(out, f, fields, indent, &mut emitted_bitgroups);
                }
            }
            CodecItem::Derived(d) => {
                let expr_str = expr_to_rust(&d.expr, &ExprContext::Parse);
                out.push_str(&format!("{indent}let {} = {expr_str};\n", d.name));
            }
            CodecItem::Require(r) => {
                let expr_str = expr_to_rust(&r.expr, &ExprContext::Parse);
                out.push_str(&format!(
                    "{indent}if !({expr_str}) {{ return Err(Error::Constraint); }}\n"
                ));
            }
        }
    }
}

fn emit_capsule_field_parse(
    out: &mut String,
    f: &CodecField,
    _all_fields: &[CodecField],
    indent: &str,
    _emitted_bitgroups: &mut Vec<u32>,
) {
    // Same as regular field parse but uses `sub` cursor instead of `cur`
    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let read_method = cursor_read_method(&f.wire_type, f.endianness);
            out.push_str(&format!("{indent}let {} = sub.{read_method}()?;\n", f.name));
        }
        FieldStrategy::BytesRemaining => {
            out.push_str(&format!("{indent}let {} = sub.read_remaining();\n", f.name));
        }
        FieldStrategy::BytesFixed => {
            if let Some(BytesSpec::Fixed { size }) = &f.bytes_spec {
                out.push_str(&format!(
                    "{indent}let {} = sub.read_bytes({size})?;\n",
                    f.name
                ));
            }
        }
        FieldStrategy::BytesLength => {
            if let Some(BytesSpec::Length { ref expr }) = f.bytes_spec {
                let len_expr = expr_to_rust(expr, &ExprContext::Parse);
                out.push_str(&format!(
                    "{indent}let {} = sub.read_bytes({len_expr} as usize)?;\n",
                    f.name
                ));
            }
        }
        _ => {
            // Fallback for other strategies
            let read_method = cursor_read_method(&f.wire_type, f.endianness);
            out.push_str(&format!("{indent}let {} = sub.{read_method}()?;\n", f.name));
        }
    }
}
