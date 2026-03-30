// crates/wirespec-backend-rust/src/parse_emit.rs
//
// Per-field parse code generation for Rust (cursor reads, bitgroups, conditionals, arrays).

use wirespec_codec::ir::*;

use crate::expr::{
    ExprContext, expr_to_rust, expr_to_rust_bool_with_field_aliases,
    expr_to_rust_with_field_aliases, extract_field_name,
};
use crate::names::{rust_count_ident, rust_ident, rust_temp_ident, to_pascal_case, to_snake_case};
use crate::type_map::*;

fn rust_parse_expr(
    cursor_var: &str,
    wt: &WireType,
    ref_type_name: Option<&str>,
    endianness: Option<wirespec_sema::types::Endianness>,
) -> String {
    match wt {
        WireType::Struct(name) | WireType::Frame(name) | WireType::Capsule(name) => {
            let cursor_arg = if cursor_var == "cur" {
                "cur".to_string()
            } else {
                format!("&mut {cursor_var}")
            };
            let type_name = to_pascal_case(name);
            format!("{type_name}::parse({cursor_arg})?")
        }
        WireType::VarInt | WireType::ContVarInt => {
            if let Some(ref_name) = ref_type_name {
                let cursor_arg = if cursor_var == "cur" {
                    "cur".to_string()
                } else {
                    format!("&mut {cursor_var}")
                };
                let parse_fn = format!("{}_parse", to_snake_case(ref_name));
                format!("{parse_fn}({cursor_arg})?")
            } else {
                format!("{cursor_var}.read_u64be()?")
            }
        }
        _ => {
            let read_method = cursor_read_method(wt, endianness);
            format!("{cursor_var}.{read_method}()?")
        }
    }
}

/// Emit parse body for a list of items (fields, derived, requires).
/// Each field becomes a local `let` binding.
pub fn emit_parse_items(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
) {
    emit_parse_items_with_field_aliases(out, fields, items, indent, "cur", &[]);
}

fn emit_parse_items_with_field_aliases(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
    cursor_var: &str,
    field_aliases: &[(&str, &str)],
) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    emit_field_parse(
                        out,
                        f,
                        fields,
                        indent,
                        &mut emitted_bitgroups,
                        cursor_var,
                        field_aliases,
                    );
                }
            }
            CodecItem::Derived(d) => {
                let name = rust_ident(&d.name);
                let expr_str =
                    expr_to_rust_with_field_aliases(&d.expr, &ExprContext::Parse, field_aliases);
                out.push_str(&format!("{indent}let {name} = {expr_str};\n"));
            }
            CodecItem::Require(r) => {
                let expr_str =
                    expr_to_rust_with_field_aliases(&r.expr, &ExprContext::Parse, field_aliases);
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
    cursor_var: &str,
    field_aliases: &[(&str, &str)],
) {
    let field_name = rust_ident(&f.name);

    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let read_expr = rust_parse_expr(
                cursor_var,
                &f.wire_type,
                f.ref_type_name.as_deref(),
                f.endianness,
            );
            out.push_str(&format!("{indent}let {field_name} = {read_expr};\n"));
        }
        FieldStrategy::BitGroup => {
            if let Some(ref bg) = f.bitgroup_member
                && !emitted_bitgroups.contains(&bg.group_id)
            {
                emitted_bitgroups.push(bg.group_id);
                emit_bitgroup_parse(out, all_fields, indent, bg, cursor_var);
            }
        }
        FieldStrategy::BytesFixed => {
            if let Some(BytesSpec::Fixed { size }) = &f.bytes_spec {
                out.push_str(&format!(
                    "{indent}let {field_name} = {cursor_var}.read_bytes({size})?;\n"
                ));
            }
        }
        FieldStrategy::BytesLength => {
            if let Some(BytesSpec::Length { ref expr }) = f.bytes_spec {
                let len_expr =
                    expr_to_rust_with_field_aliases(expr, &ExprContext::Parse, field_aliases);
                if let Some(ref hint) = f.asn1_hint {
                    let bytes_name = rust_temp_ident("_", &f.name, "_bytes");
                    out.push_str(&format!(
                        "{indent}let {bytes_name} = {cursor_var}.read_bytes({len_expr} as usize)?;\n"
                    ));
                    out.push_str(&format!(
                        "{indent}let {field_name} = {}::decode::<{}>({bytes_name}).map_err(|_| Error::Asn1Decode)?;\n",
                        hint.encoding, hint.type_name
                    ));
                } else {
                    out.push_str(&format!(
                        "{indent}let {field_name} = {cursor_var}.read_bytes({len_expr} as usize)?;\n"
                    ));
                }
            }
        }
        FieldStrategy::BytesRemaining => {
            if let Some(ref hint) = f.asn1_hint {
                let bytes_name = rust_temp_ident("_", &f.name, "_bytes");
                out.push_str(&format!(
                    "{indent}let {bytes_name} = {cursor_var}.read_remaining();\n"
                ));
                out.push_str(&format!(
                    "{indent}let {field_name} = {}::decode::<{}>({bytes_name}).map_err(|_| Error::Asn1Decode)?;\n",
                    hint.encoding, hint.type_name
                ));
            } else {
                out.push_str(&format!(
                    "{indent}let {field_name} = {cursor_var}.read_remaining();\n"
                ));
            }
        }
        FieldStrategy::BytesLor => {
            if let Some(BytesSpec::LengthOrRemaining { ref expr }) = f.bytes_spec {
                let value_id = get_value_ref_id(expr);
                let length_name = rust_ident(extract_field_name(&value_id));
                out.push_str(&format!(
                    "{indent}let {field_name} = if let Some(l) = {length_name} {{ {cursor_var}.read_bytes(l as usize)? }} else {{ {cursor_var}.read_remaining() }};\n"
                ));
            }
        }
        FieldStrategy::Conditional => {
            if let Some(ref cond) = f.condition {
                let cond_str =
                    expr_to_rust_bool_with_field_aliases(cond, &ExprContext::Parse, field_aliases);
                let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);

                match inner_wt {
                    WireType::Bytes => {
                        if let Some(ref bytes_spec) = f.bytes_spec {
                            match bytes_spec {
                                BytesSpec::Fixed { size } => {
                                    out.push_str(&format!(
                                        "{indent}let {field_name} = if {cond_str} {{ Some({cursor_var}.read_bytes({size})?) }} else {{ None }};\n"
                                    ));
                                }
                                BytesSpec::Remaining => {
                                    out.push_str(&format!(
                                        "{indent}let {field_name} = if {cond_str} {{ Some({cursor_var}.read_remaining()) }} else {{ None }};\n"
                                    ));
                                }
                                _ => {
                                    out.push_str(&format!(
                                        "{indent}let {field_name} = if {cond_str} {{ Some({cursor_var}.read_remaining()) }} else {{ None }};\n"
                                    ));
                                }
                            }
                        } else {
                            out.push_str(&format!(
                                "{indent}let {field_name} = if {cond_str} {{ Some({cursor_var}.read_remaining()) }} else {{ None }};\n"
                            ));
                        }
                    }
                    WireType::Array => {
                        unreachable!("unexpected conditional array field");
                    }
                    _ => {
                        let read_expr = rust_parse_expr(
                            cursor_var,
                            inner_wt,
                            f.ref_type_name.as_deref(),
                            f.endianness,
                        );
                        out.push_str(&format!(
                            "{indent}let {field_name} = if {cond_str} {{ Some({read_expr}) }} else {{ None }};\n"
                        ));
                    }
                }
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                emit_array_parse(out, f, arr, indent, cursor_var, field_aliases);
            }
        }
        FieldStrategy::Struct | FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            let read_expr = rust_parse_expr(
                cursor_var,
                &f.wire_type,
                f.ref_type_name.as_deref(),
                f.endianness,
            );
            out.push_str(&format!("{indent}let {field_name} = {read_expr};\n"));
        }
    }
}

fn emit_bitgroup_parse(
    out: &mut String,
    all_fields: &[CodecField],
    indent: &str,
    bg: &BitgroupMember,
    cursor_var: &str,
) {
    let group_id = bg.group_id;
    let read_method = bitgroup_read_method(bg.total_bits, bg.group_endianness);
    let var_name = format!("_bg{group_id}");

    out.push_str(&format!(
        "{indent}let {var_name} = {cursor_var}.{read_method}()?;\n"
    ));

    for f in all_fields {
        if let Some(ref mbg) = f.bitgroup_member
            && mbg.group_id == group_id
        {
            let mask = (1u64 << mbg.member_width_bits) - 1;
            let shift = mbg.member_offset_bits;
            let target_type = wire_type_to_rust(&f.wire_type);
            let field_name = rust_ident(&f.name);
            out.push_str(&format!(
                "{indent}let {field_name} = (({var_name} >> {shift}) & 0x{mask:x}) as {target_type};\n"
            ));
        }
    }
}

fn emit_array_parse(
    out: &mut String,
    f: &CodecField,
    arr: &ArraySpec,
    indent: &str,
    cursor_var: &str,
    field_aliases: &[(&str, &str)],
) {
    let max_elems = f.max_elements.unwrap_or(256);
    let field_name = rust_ident(&f.name);
    let count_name = rust_count_ident(&f.name);
    if let Some(ref count_expr) = arr.count_expr {
        let count_str =
            expr_to_rust_with_field_aliases(count_expr, &ExprContext::Parse, field_aliases);

        out.push_str(&format!(
            "{indent}let {count_name} = {count_str} as usize;\n"
        ));
        out.push_str(&format!(
            "{indent}if {count_name} > {max_elems} {{ return Err(Error::Capacity); }}\n"
        ));
        out.push_str(&format!(
            "{indent}let mut {field_name} = std::array::from_fn(|_| Default::default());\n"
        ));
        out.push_str(&format!("{indent}for _i in 0..{count_name} {{\n"));
        emit_array_element_read_indexed(out, f, arr, indent, "_i", cursor_var);
        out.push_str(&format!("{indent}}}\n"));
    } else {
        out.push_str(&format!(
            "{indent}let mut {field_name} = std::array::from_fn(|_| Default::default());\n"
        ));
        out.push_str(&format!("{indent}let mut {count_name}: usize = 0;\n"));
        out.push_str(&format!("{indent}while {cursor_var}.remaining() > 0 {{\n"));
        out.push_str(&format!(
            "{indent}    if {count_name} >= {max_elems} {{ return Err(Error::Capacity); }}\n"
        ));
        emit_array_element_read_indexed(out, f, arr, indent, &count_name, cursor_var);
        out.push_str(&format!("{indent}    {count_name} += 1;\n"));
        out.push_str(&format!("{indent}}}\n"));
    }
}

fn emit_array_element_read_indexed(
    out: &mut String,
    f: &CodecField,
    arr: &ArraySpec,
    indent: &str,
    idx: &str,
    cursor_var: &str,
) {
    let field_name = rust_ident(&f.name);

    match arr.element_strategy {
        FieldStrategy::Primitive | FieldStrategy::Struct => {
            let read_expr = rust_parse_expr(
                cursor_var,
                &arr.element_wire_type,
                arr.element_ref_type_name.as_deref(),
                None,
            );
            out.push_str(&format!("{indent}    {field_name}[{idx}] = {read_expr};\n"));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            let read_expr = rust_parse_expr(
                cursor_var,
                &arr.element_wire_type,
                arr.element_ref_type_name.as_deref(),
                None,
            );
            out.push_str(&format!("{indent}    {field_name}[{idx}] = {read_expr};\n"));
        }
        _ => unreachable!(
            "unexpected array element strategy: {:?}",
            arr.element_strategy
        ),
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
    let tag_read_expr = rust_parse_expr(
        "cur",
        &frame.tag.wire_type,
        frame.tag.ref_type_name.as_deref(),
        frame.tag.endianness,
    );
    out.push_str(&format!("{indent}let _tag_val = {tag_read_expr};\n"));

    out.push_str(&format!("{indent}let _result = match _tag_val {{\n"));
    for variant in &frame.variants {
        emit_variant_parse_arm(out, variant, indent, &frame.tag.field_name);
    }
    out.push_str(&format!("{indent}}};\n"));
    out.push_str(&format!("{indent}Ok((_result, _tag_val))\n"));
}

fn emit_variant_parse_arm(
    out: &mut String,
    variant: &CodecVariantScope,
    indent: &str,
    tag_field_name: &str,
) {
    let variant_name = to_pascal_case(&variant.name);
    let inner_indent = format!("{indent}        ");
    let field_aliases = [(tag_field_name, "_tag_val")];

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
        for item in &variant.items {
            if let CodecItem::Require(r) = item {
                let expr_str =
                    expr_to_rust_with_field_aliases(&r.expr, &ExprContext::Parse, &field_aliases);
                out.push_str(&format!(
                    "{inner_indent}if !({expr_str}) {{ return Err(Error::Constraint); }}\n"
                ));
            }
        }
        out.push_str(&format!("{inner_indent}Self::{variant_name}\n"));
    } else {
        emit_parse_items_with_field_aliases(
            out,
            &variant.fields,
            &variant.items,
            &inner_indent,
            "cur",
            &field_aliases,
        );

        if variant.fields.is_empty() {
            out.push_str(&format!("{inner_indent}Self::{variant_name}\n"));
        } else {
            out.push_str(&format!("{inner_indent}Self::{variant_name} {{\n"));
            for item in &variant.items {
                match item {
                    CodecItem::Field { field_id } => {
                        if let Some(f) = variant.fields.iter().find(|f| &f.field_id == field_id) {
                            let field_name = rust_ident(&f.name);
                            if f.strategy == FieldStrategy::Array {
                                let count_name = rust_count_ident(&f.name);
                                out.push_str(&format!("{inner_indent}    {field_name},\n"));
                                out.push_str(&format!("{inner_indent}    {count_name},\n"));
                            } else {
                                out.push_str(&format!("{inner_indent}    {field_name},\n"));
                            }
                        }
                    }
                    CodecItem::Derived(d) => {
                        let name = rust_ident(&d.name);
                        out.push_str(&format!("{inner_indent}    {name},\n"));
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
    let type_name = to_pascal_case(&capsule.name);
    let payload_type = format!("{type_name}Payload");

    emit_parse_items(out, &capsule.header_fields, &capsule.header_items, indent);

    let tag_match_expr = if let Some(ref expr) = capsule.tag_expr {
        expr_to_rust(expr, &ExprContext::Parse)
    } else {
        rust_ident(&capsule.tag.field_name)
    };
    let within_expr = rust_ident(&capsule.within_field);

    out.push_str(&format!(
        "{indent}let mut sub = cur.sub_cursor({within_expr} as usize)?;\n"
    ));

    out.push_str(&format!(
        "{indent}let payload = match {tag_match_expr} {{\n"
    ));
    for variant in &capsule.variants {
        emit_capsule_variant_arm(out, variant, indent, &payload_type);
    }
    out.push_str(&format!("{indent}}};\n"));

    out.push_str(&format!("{indent}Ok(Self {{\n"));
    for item in &capsule.header_items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = capsule
                    .header_fields
                    .iter()
                    .find(|f| &f.field_id == field_id)
                {
                    let field_name = rust_ident(&f.name);
                    out.push_str(&format!("{indent}    {field_name},\n"));
                }
            }
            CodecItem::Derived(d) => {
                let name = rust_ident(&d.name);
                out.push_str(&format!("{indent}    {name},\n"));
            }
            CodecItem::Require(_) => {}
        }
    }
    out.push_str(&format!("{indent}    payload,\n"));
    out.push_str(&format!("{indent}}})\n"));
}

fn emit_capsule_variant_arm(
    out: &mut String,
    variant: &CodecVariantScope,
    indent: &str,
    payload_type: &str,
) {
    let variant_name = to_pascal_case(&variant.name);
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

    emit_capsule_variant_parse_items(out, &variant.fields, &variant.items, &inner_indent);

    if variant.fields.is_empty() {
        out.push_str(&format!("{inner_indent}{payload_type}::{variant_name}\n"));
    } else {
        out.push_str(&format!(
            "{inner_indent}{payload_type}::{variant_name} {{\n"
        ));
        for item in &variant.items {
            match item {
                CodecItem::Field { field_id } => {
                    if let Some(f) = variant.fields.iter().find(|f| &f.field_id == field_id) {
                        let field_name = rust_ident(&f.name);
                        if f.strategy == FieldStrategy::Array {
                            let count_name = rust_count_ident(&f.name);
                            out.push_str(&format!("{inner_indent}    {field_name},\n"));
                            out.push_str(&format!("{inner_indent}    {count_name},\n"));
                        } else {
                            out.push_str(&format!("{inner_indent}    {field_name},\n"));
                        }
                    }
                }
                CodecItem::Derived(d) => {
                    let name = rust_ident(&d.name);
                    out.push_str(&format!("{inner_indent}    {name},\n"));
                }
                CodecItem::Require(_) => {}
            }
        }
        out.push_str(&format!("{inner_indent}}}\n"));
    }

    out.push_str(&format!("{indent}    }}\n"));
}

fn emit_capsule_variant_parse_items(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
) {
    emit_parse_items_with_field_aliases(out, fields, items, indent, "sub", &[]);
}
