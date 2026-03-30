// crates/wirespec-backend-rust/src/serialize_emit.rs
//
// Per-field serialize code generation for Rust (writer writes).

use wirespec_codec::ir::*;

use crate::names::{rust_count_ident, rust_ident, rust_temp_ident, to_pascal_case};
use crate::type_map::*;

fn prefixed_field(val_prefix: &str, name: &str) -> String {
    format!("{val_prefix}{}", rust_ident(name))
}

fn prefixed_count_field(val_prefix: &str, name: &str) -> String {
    format!("{val_prefix}{}", rust_count_ident(name))
}

fn borrowed_value_expr(wt: &WireType, value_expr: &str) -> String {
    match wt {
        WireType::Bytes | WireType::Struct(_) | WireType::Frame(_) | WireType::Capsule(_) => {
            value_expr.to_string()
        }
        _ => format!("*{value_expr}"),
    }
}

fn variant_tag_len_stmt(
    variant: &CodecVariantScope,
    tag_wire_type: &WireType,
    tag_ref_type_name: Option<&str>,
    indent: &str,
) -> String {
    if let Some(w) = wire_type_byte_width(tag_wire_type) {
        return format!("{indent}len += {w};\n");
    }

    if matches!(tag_wire_type, WireType::VarInt | WireType::ContVarInt) {
        if let Some(ref_name) = tag_ref_type_name {
            let wire_size_fn = format!("{}_wire_size", crate::names::to_snake_case(ref_name));
            let representative = match &variant.pattern {
                VariantPattern::Exact { value } => value.to_string(),
                VariantPattern::RangeInclusive { start, .. } => start.to_string(),
                VariantPattern::Wildcard => "0".into(),
            };
            return format!("{indent}len += {wire_size_fn}({representative});\n");
        }
        return format!("{indent}len += 8; /* varint */\n");
    }

    String::new()
}

/// Emit serialize body for a list of items.
/// `val_prefix` is typically "self." for packet fields or variant-specific for frames.
pub fn emit_serialize_items(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
    val_prefix: &str,
) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    // Pre-scan: find length fields that are associated with ASN.1 payload fields.
    // These length fields must NOT be serialized normally — their value is recomputed
    // from the encoded ASN.1 payload.
    let asn1_length_fields = collect_asn1_length_field_names(fields);

    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    // Skip length fields that will be recomputed by ASN.1 payload serialization
                    if asn1_length_fields.contains(&f.name.as_str()) {
                        continue;
                    }
                    emit_field_serialize(
                        out,
                        f,
                        fields,
                        indent,
                        val_prefix,
                        &mut emitted_bitgroups,
                    );
                }
            }
            CodecItem::Derived(_) | CodecItem::Require(_) => {
                // Derived and require don't generate serialize code
            }
        }
    }
}

/// Collect names of length fields that are referenced by ASN.1 payload fields.
fn collect_asn1_length_field_names(fields: &[CodecField]) -> Vec<&str> {
    let mut names = Vec::new();
    for f in fields {
        if f.asn1_hint.is_some()
            && let Some(BytesSpec::Length {
                expr: CodecExpr::ValueRef { reference },
            }) = &f.bytes_spec
        {
            let field_name = crate::expr::extract_field_name(&reference.value_id);
            names.push(field_name);
        }
    }
    names
}

fn emit_field_serialize(
    out: &mut String,
    f: &CodecField,
    all_fields: &[CodecField],
    indent: &str,
    val_prefix: &str,
    emitted_bitgroups: &mut Vec<u32>,
) {
    let field_ref = prefixed_field(val_prefix, &f.name);

    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let write_expr = rust_write_expr(
                "w",
                &f.wire_type,
                f.ref_type_name.as_deref(),
                f.endianness,
                &field_ref,
            );
            out.push_str(&format!("{indent}{write_expr};\n"));
        }
        FieldStrategy::BitGroup => {
            if let Some(ref bg) = f.bitgroup_member
                && !emitted_bitgroups.contains(&bg.group_id)
            {
                emitted_bitgroups.push(bg.group_id);
                emit_bitgroup_serialize(out, all_fields, indent, val_prefix, bg);
            }
        }
        FieldStrategy::BytesFixed
        | FieldStrategy::BytesLength
        | FieldStrategy::BytesRemaining
        | FieldStrategy::BytesLor => {
            if let Some(ref hint) = f.asn1_hint {
                // Encode payload first
                out.push_str(&format!(
                    "{indent}let {} = {}::encode(&{field_ref}).map_err(|_| Error::Asn1Encode)?;\n",
                    rust_temp_ident("_", &f.name, "_encoded"),
                    hint.encoding
                ));
                // For length-prefixed ASN.1 fields, write the recomputed length before the payload
                if let Some(BytesSpec::Length {
                    expr: CodecExpr::ValueRef { reference },
                }) = &f.bytes_spec
                {
                    let len_field = crate::expr::extract_field_name(&reference.value_id);
                    if let Some(lf) = all_fields.iter().find(|af| af.name == len_field) {
                        let write_method = writer_write_method(&lf.wire_type, lf.endianness);
                        let encoded_name = rust_temp_ident("_", &f.name, "_encoded");
                        out.push_str(&format!(
                            "{indent}w.{write_method}({encoded_name}.len() as {})?;\n",
                            wire_type_to_rust(&lf.wire_type)
                        ));
                    }
                }
                out.push_str(&format!(
                    "{indent}w.write_bytes(&{})?;\n",
                    rust_temp_ident("_", &f.name, "_encoded")
                ));
            } else {
                out.push_str(&format!("{indent}w.write_bytes({field_ref})?;\n"));
            }
        }
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            let option_ref = prefixed_field(val_prefix, &f.name);
            match inner_wt {
                WireType::Bytes => {
                    out.push_str(&format!(
                        "{indent}if let Some(ref val) = {option_ref} {{ w.write_bytes(val)?; }}\n"
                    ));
                }
                WireType::Array => unreachable!("unexpected conditional array field"),
                _ => {
                    let value_expr = borrowed_value_expr(inner_wt, "val");
                    let write_expr = rust_write_expr(
                        "w",
                        inner_wt,
                        f.ref_type_name.as_deref(),
                        f.endianness,
                        &value_expr,
                    );
                    out.push_str(&format!(
                        "{indent}if let Some(ref val) = {option_ref} {{ {write_expr}; }}\n"
                    ));
                }
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                emit_array_serialize(out, f, arr, indent, val_prefix);
            }
        }
        FieldStrategy::Struct => {
            let write_expr = rust_write_expr(
                "w",
                &f.wire_type,
                f.ref_type_name.as_deref(),
                f.endianness,
                &field_ref,
            );
            out.push_str(&format!("{indent}{write_expr};\n"));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            let write_expr = rust_write_expr(
                "w",
                &f.wire_type,
                f.ref_type_name.as_deref(),
                f.endianness,
                &field_ref,
            );
            out.push_str(&format!("{indent}{write_expr};\n"));
        }
    }
}

fn emit_bitgroup_serialize(
    out: &mut String,
    all_fields: &[CodecField],
    indent: &str,
    val_prefix: &str,
    bg: &BitgroupMember,
) {
    let group_id = bg.group_id;
    let total_bits = bg.total_bits;
    let container_type = bitgroup_rust_type(total_bits);
    let write_method = bitgroup_write_method(total_bits, bg.group_endianness);
    let var_name = format!("_bg{group_id}");

    out.push_str(&format!(
        "{indent}let mut {var_name}: {container_type} = 0;\n"
    ));

    for f in all_fields {
        if let Some(ref mbg) = f.bitgroup_member
            && mbg.group_id == group_id
        {
            let mask = (1u64 << mbg.member_width_bits) - 1;
            let shift = mbg.member_offset_bits;
            let field_ref = prefixed_field(val_prefix, &f.name);
            out.push_str(&format!(
                    "{indent}{var_name} |= (({field_ref} as {container_type}) & 0x{mask:x}) << {shift};\n"
                ));
        }
    }

    out.push_str(&format!("{indent}w.{write_method}({var_name})?;\n"));
}

fn emit_array_serialize(
    out: &mut String,
    f: &CodecField,
    arr: &ArraySpec,
    indent: &str,
    val_prefix: &str,
) {
    let count_ref = prefixed_count_field(val_prefix, &f.name);
    let field_ref = prefixed_field(val_prefix, &f.name);
    out.push_str(&format!("{indent}for _i in 0..{count_ref} {{\n"));

    match arr.element_strategy {
        FieldStrategy::Primitive => {
            let write_expr = rust_write_expr(
                "w",
                &arr.element_wire_type,
                arr.element_ref_type_name.as_deref(),
                None,
                &format!("{field_ref}[_i]"),
            );
            out.push_str(&format!("{indent}    {write_expr};\n"));
        }
        FieldStrategy::Struct => {
            let write_expr = rust_write_expr(
                "w",
                &arr.element_wire_type,
                arr.element_ref_type_name.as_deref(),
                None,
                &format!("{field_ref}[_i]"),
            );
            out.push_str(&format!("{indent}    {write_expr};\n"));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            let write_expr = rust_write_expr(
                "w",
                &arr.element_wire_type,
                arr.element_ref_type_name.as_deref(),
                None,
                &format!("{field_ref}[_i]"),
            );
            out.push_str(&format!("{indent}    {write_expr};\n"));
        }
        _ => unreachable!(
            "unexpected array element strategy: {:?}",
            arr.element_strategy
        ),
    }

    out.push_str(&format!("{indent}}}\n"));
}

/// Emit serialized_len body for fields.
pub fn emit_serialized_len_items(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
    val_prefix: &str,
) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    emit_field_serialized_len(out, f, indent, val_prefix, &mut emitted_bitgroups);
                }
            }
            CodecItem::Derived(_) | CodecItem::Require(_) => {}
        }
    }
}

fn emit_field_serialized_len(
    out: &mut String,
    f: &CodecField,
    indent: &str,
    val_prefix: &str,
    emitted_bitgroups: &mut Vec<u32>,
) {
    let field_ref = prefixed_field(val_prefix, &f.name);
    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            if let Some(w) = wire_type_byte_width(&f.wire_type) {
                out.push_str(&format!("{indent}len += {w};\n"));
            }
        }
        FieldStrategy::BitGroup => {
            if let Some(ref bg) = f.bitgroup_member
                && !emitted_bitgroups.contains(&bg.group_id)
            {
                emitted_bitgroups.push(bg.group_id);
                let bytes = bg.total_bits.div_ceil(8);
                out.push_str(&format!("{indent}len += {bytes};\n"));
            }
        }
        FieldStrategy::BytesFixed => {
            if let Some(BytesSpec::Fixed { size }) = &f.bytes_spec {
                out.push_str(&format!("{indent}len += {size};\n"));
            }
        }
        FieldStrategy::BytesLength | FieldStrategy::BytesRemaining | FieldStrategy::BytesLor => {
            if let Some(ref hint) = f.asn1_hint {
                out.push_str(&format!(
                    "{indent}len += {}::encode(&{field_ref}).map(|b| b.len()).unwrap_or(0);\n",
                    hint.encoding
                ));
            } else {
                out.push_str(&format!("{indent}len += {field_ref}.len();\n"));
            }
        }
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            let option_ref = prefixed_field(val_prefix, &f.name);
            match inner_wt {
                WireType::Bytes => {
                    out.push_str(&format!(
                        "{indent}if let Some(ref b) = {option_ref} {{ len += b.len(); }}\n"
                    ));
                }
                WireType::Struct(_) | WireType::Frame(_) | WireType::Capsule(_) => {
                    out.push_str(&format!(
                        "{indent}if let Some(ref v) = {option_ref} {{ len += v.serialized_len(); }}\n"
                    ));
                }
                WireType::VarInt | WireType::ContVarInt => {
                    if let Some(ref ref_name) = f.ref_type_name {
                        let wire_size_fn =
                            format!("{}_wire_size", crate::names::to_snake_case(ref_name));
                        out.push_str(&format!(
                            "{indent}if let Some(ref v) = {option_ref} {{ len += {wire_size_fn}(*v); }}\n"
                        ));
                    } else {
                        out.push_str(&format!(
                            "{indent}if {option_ref}.is_some() {{ len += 8; }}\n"
                        ));
                    }
                }
                _ => {
                    if let Some(w) = wire_type_byte_width(inner_wt) {
                        out.push_str(&format!(
                            "{indent}if {option_ref}.is_some() {{ len += {w}; }}\n"
                        ));
                    }
                }
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                let count_ref = prefixed_count_field(val_prefix, &f.name);
                let field_ref = prefixed_field(val_prefix, &f.name);
                if let Some(w) = wire_type_byte_width(&arr.element_wire_type) {
                    out.push_str(&format!("{indent}len += {count_ref} * {w};\n"));
                } else {
                    // Variable-size elements
                    out.push_str(&format!("{indent}for _i in 0..{count_ref} {{\n"));
                    out.push_str(&format!(
                        "{indent}    len += {field_ref}[_i].serialized_len();\n"
                    ));
                    out.push_str(&format!("{indent}}}\n"));
                }
            }
        }
        FieldStrategy::Struct => {
            out.push_str(&format!("{indent}len += {field_ref}.serialized_len();\n"));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = f.ref_type_name {
                let wire_size_fn = format!("{}_wire_size", crate::names::to_snake_case(ref_name));
                out.push_str(&format!("{indent}len += {wire_size_fn}({field_ref});\n"));
            } else {
                out.push_str(&format!("{indent}len += 8; /* varint */\n"));
            }
        }
    }
}

/// Emit serialize body for frame variants (Rust enum match).
pub fn emit_frame_serialize_body(out: &mut String, frame: &CodecFrame, indent: &str) {
    // Write the tag first
    let tag_write_expr = rust_write_expr(
        "w",
        &frame.tag.wire_type,
        frame.tag.ref_type_name.as_deref(),
        frame.tag.endianness,
        "tag",
    );
    out.push_str(&format!("{indent}{tag_write_expr};\n"));

    // Match on self for variant-specific serialization
    out.push_str(&format!("{indent}match self {{\n"));

    for variant in &frame.variants {
        let variant_name = to_pascal_case(&variant.name);
        let inner_indent = format!("{indent}        ");

        if variant.fields.is_empty() {
            out.push_str(&format!("{indent}    Self::{variant_name} => {{}}\n"));
        } else {
            // Destructure variant fields
            out.push_str(&format!("{indent}    Self::{variant_name} {{ "));
            let field_names: Vec<String> = collect_variant_field_names(variant);
            out.push_str(&field_names.join(", "));
            out.push_str(" } => {\n");

            // Serialize each field
            emit_variant_serialize_fields(out, variant, &inner_indent);

            out.push_str(&format!("{indent}    }}\n"));
        }
    }

    out.push_str(&format!("{indent}}}\n"));
}

/// Emit serialized_len body for frame variants.
pub fn emit_frame_serialized_len_body(out: &mut String, frame: &CodecFrame, indent: &str) {
    // Match on self for variant-specific lengths
    out.push_str(&format!("{indent}match self {{\n"));

    for variant in &frame.variants {
        let variant_name = to_pascal_case(&variant.name);
        let inner_indent = format!("{indent}        ");

        if variant.fields.is_empty() {
            out.push_str(&format!("{indent}    Self::{variant_name} => {{\n"));
            out.push_str(&variant_tag_len_stmt(
                variant,
                &frame.tag.wire_type,
                frame.tag.ref_type_name.as_deref(),
                &inner_indent,
            ));
            out.push_str(&format!("{indent}    }}\n"));
        } else {
            out.push_str(&format!("{indent}    Self::{variant_name} {{ "));
            let field_names: Vec<String> = collect_variant_field_names(variant);
            out.push_str(&field_names.join(", "));
            out.push_str(" } => {\n");

            out.push_str(&variant_tag_len_stmt(
                variant,
                &frame.tag.wire_type,
                frame.tag.ref_type_name.as_deref(),
                &inner_indent,
            ));
            emit_variant_serialized_len_fields(out, variant, &inner_indent);

            out.push_str(&format!("{indent}    }}\n"));
        }
    }

    out.push_str(&format!("{indent}}}\n"));
}

/// Emit serialize body for capsule payload variants.
pub fn emit_capsule_serialize_body(out: &mut String, capsule: &CodecCapsule, indent: &str) {
    let payload_type = format!("{}Payload", to_pascal_case(&capsule.name));

    out.push_str(&format!("{indent}match &self.payload {{\n"));

    for variant in &capsule.variants {
        let variant_name = to_pascal_case(&variant.name);
        let inner_indent = format!("{indent}        ");

        if variant.fields.is_empty() {
            out.push_str(&format!(
                "{indent}    {payload_type}::{variant_name} => {{}}\n"
            ));
        } else {
            out.push_str(&format!("{indent}    {payload_type}::{variant_name} {{ "));
            let field_names: Vec<String> = collect_variant_field_names(variant);
            out.push_str(&field_names.join(", "));
            out.push_str(" } => {\n");

            emit_variant_serialize_fields(out, variant, &inner_indent);

            out.push_str(&format!("{indent}    }}\n"));
        }
    }

    out.push_str(&format!("{indent}}}\n"));
}

/// Emit serialized_len body for capsule payload variants.
pub fn emit_capsule_serialized_len_body(out: &mut String, capsule: &CodecCapsule, indent: &str) {
    let payload_type = format!("{}Payload", to_pascal_case(&capsule.name));

    out.push_str(&format!("{indent}match &self.payload {{\n"));

    for variant in &capsule.variants {
        let variant_name = to_pascal_case(&variant.name);

        if variant.fields.is_empty() {
            out.push_str(&format!(
                "{indent}    {payload_type}::{variant_name} => {{}}\n"
            ));
        } else {
            out.push_str(&format!("{indent}    {payload_type}::{variant_name} {{ "));
            let field_names: Vec<String> = collect_variant_field_names(variant);
            out.push_str(&field_names.join(", "));
            out.push_str(" } => {\n");

            let inner_indent = format!("{indent}        ");
            emit_variant_serialized_len_fields(out, variant, &inner_indent);

            out.push_str(&format!("{indent}    }}\n"));
        }
    }

    out.push_str(&format!("{indent}}}\n"));
}

/// Collect field names from a variant (for destructuring in match arms).
fn collect_variant_field_names(variant: &CodecVariantScope) -> Vec<String> {
    let mut names = Vec::new();
    for item in &variant.items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = variant.fields.iter().find(|f| &f.field_id == field_id) {
                    names.push(rust_ident(&f.name));
                    if f.strategy == FieldStrategy::Array {
                        names.push(rust_count_ident(&f.name));
                    }
                }
            }
            CodecItem::Derived(d) => {
                names.push(rust_ident(&d.name));
            }
            CodecItem::Require(_) => {}
        }
    }
    names
}

fn emit_variant_serialize_fields(out: &mut String, variant: &CodecVariantScope, indent: &str) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    // Pre-scan: skip length fields associated with ASN.1 payloads
    let asn1_length_fields = collect_asn1_length_field_names(&variant.fields);

    for item in &variant.items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = variant.fields.iter().find(|f| &f.field_id == field_id) {
                    if asn1_length_fields.contains(&f.name.as_str()) {
                        continue;
                    }
                    emit_variant_field_serialize(
                        out,
                        f,
                        &variant.fields,
                        indent,
                        &mut emitted_bitgroups,
                    );
                }
            }
            CodecItem::Derived(_) | CodecItem::Require(_) => {}
        }
    }
}

fn emit_variant_field_serialize(
    out: &mut String,
    f: &CodecField,
    all_fields: &[CodecField],
    indent: &str,
    emitted_bitgroups: &mut Vec<u32>,
) {
    // For frame variants, field refs are direct (destructured) not self.field
    let field_name = rust_ident(&f.name);
    let count_name = rust_count_ident(&f.name);

    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let write_expr = rust_write_expr(
                "w",
                &f.wire_type,
                f.ref_type_name.as_deref(),
                f.endianness,
                "*val",
            );
            out.push_str(&format!(
                "{indent}{{ let val = {field_name}; {write_expr}; }}\n"
            ));
        }
        FieldStrategy::BitGroup => {
            if let Some(ref bg) = f.bitgroup_member
                && !emitted_bitgroups.contains(&bg.group_id)
            {
                emitted_bitgroups.push(bg.group_id);
                let container_type = bitgroup_rust_type(bg.total_bits);
                let write_method = bitgroup_write_method(bg.total_bits, bg.group_endianness);
                let var_name = format!("_bg{}", bg.group_id);

                out.push_str(&format!(
                    "{indent}let mut {var_name}: {container_type} = 0;\n"
                ));

                for af in all_fields {
                    if let Some(ref mbg) = af.bitgroup_member
                        && mbg.group_id == bg.group_id
                    {
                        let mask = (1u64 << mbg.member_width_bits) - 1;
                        let shift = mbg.member_offset_bits;
                        let member_name = rust_ident(&af.name);
                        out.push_str(&format!(
                                    "{indent}{var_name} |= ((*{member_name} as {container_type}) & 0x{mask:x}) << {shift};\n"
                                ));
                    }
                }

                out.push_str(&format!("{indent}w.{write_method}({var_name})?;\n"));
            }
        }
        FieldStrategy::BytesFixed
        | FieldStrategy::BytesLength
        | FieldStrategy::BytesRemaining
        | FieldStrategy::BytesLor => {
            if let Some(ref hint) = f.asn1_hint {
                let encoded_name = rust_temp_ident("_", &f.name, "_encoded");
                out.push_str(&format!(
                    "{indent}let {encoded_name} = {}::encode({field_name}).map_err(|_| Error::Asn1Encode)?;\n",
                    hint.encoding
                ));
                // For length-prefixed ASN.1 fields, write the recomputed length before the payload
                if let Some(BytesSpec::Length {
                    expr: CodecExpr::ValueRef { reference },
                }) = &f.bytes_spec
                {
                    let len_field = crate::expr::extract_field_name(&reference.value_id);
                    if let Some(lf) = all_fields.iter().find(|af| af.name == len_field) {
                        let write_method = writer_write_method(&lf.wire_type, lf.endianness);
                        out.push_str(&format!(
                            "{indent}w.{write_method}({encoded_name}.len() as {})?;\n",
                            wire_type_to_rust(&lf.wire_type)
                        ));
                    }
                }
                out.push_str(&format!("{indent}w.write_bytes(&{encoded_name})?;\n"));
            } else {
                out.push_str(&format!("{indent}w.write_bytes({field_name})?;\n"));
            }
        }
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            match inner_wt {
                WireType::Bytes => {
                    out.push_str(&format!(
                        "{indent}if let Some(val) = {field_name} {{ w.write_bytes(val)?; }}\n"
                    ));
                }
                WireType::Array => unreachable!("unexpected conditional array field"),
                _ => {
                    let value_expr = borrowed_value_expr(inner_wt, "val");
                    let write_expr = rust_write_expr(
                        "w",
                        inner_wt,
                        f.ref_type_name.as_deref(),
                        f.endianness,
                        &value_expr,
                    );
                    out.push_str(&format!(
                        "{indent}if let Some(val) = {field_name} {{ {write_expr}; }}\n"
                    ));
                }
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                out.push_str(&format!("{indent}for _i in 0..*{count_name} {{\n"));
                match arr.element_strategy {
                    FieldStrategy::Primitive => {
                        let write_expr = rust_write_expr(
                            "w",
                            &arr.element_wire_type,
                            arr.element_ref_type_name.as_deref(),
                            None,
                            &format!("{field_name}[_i]"),
                        );
                        out.push_str(&format!("{indent}    {write_expr};\n"));
                    }
                    FieldStrategy::Struct => {
                        let write_expr = rust_write_expr(
                            "w",
                            &arr.element_wire_type,
                            arr.element_ref_type_name.as_deref(),
                            None,
                            &format!("{field_name}[_i]"),
                        );
                        out.push_str(&format!("{indent}    {write_expr};\n"));
                    }
                    FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
                        let write_expr = rust_write_expr(
                            "w",
                            &arr.element_wire_type,
                            arr.element_ref_type_name.as_deref(),
                            None,
                            &format!("{field_name}[_i]"),
                        );
                        out.push_str(&format!("{indent}    {write_expr};\n"));
                    }
                    _ => {}
                }
                out.push_str(&format!("{indent}}}\n"));
            }
        }
        FieldStrategy::Struct => {
            let write_expr = rust_write_expr(
                "w",
                &f.wire_type,
                f.ref_type_name.as_deref(),
                f.endianness,
                field_name.as_str(),
            );
            out.push_str(&format!("{indent}{write_expr};\n"));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            let write_expr = rust_write_expr(
                "w",
                &f.wire_type,
                f.ref_type_name.as_deref(),
                f.endianness,
                &format!("*{field_name}"),
            );
            out.push_str(&format!("{indent}{write_expr};\n"));
        }
    }
}

fn emit_variant_serialized_len_fields(out: &mut String, variant: &CodecVariantScope, indent: &str) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    for item in &variant.items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = variant.fields.iter().find(|f| &f.field_id == field_id) {
                    emit_variant_field_serialized_len(out, f, indent, &mut emitted_bitgroups);
                }
            }
            CodecItem::Derived(_) | CodecItem::Require(_) => {}
        }
    }
}

fn emit_variant_field_serialized_len(
    out: &mut String,
    f: &CodecField,
    indent: &str,
    emitted_bitgroups: &mut Vec<u32>,
) {
    let field_name = rust_ident(&f.name);
    let count_name = rust_count_ident(&f.name);

    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            if let Some(w) = wire_type_byte_width(&f.wire_type) {
                out.push_str(&format!("{indent}len += {w};\n"));
            }
        }
        FieldStrategy::BitGroup => {
            if let Some(ref bg) = f.bitgroup_member
                && !emitted_bitgroups.contains(&bg.group_id)
            {
                emitted_bitgroups.push(bg.group_id);
                let bytes = bg.total_bits.div_ceil(8);
                out.push_str(&format!("{indent}len += {bytes};\n"));
            }
        }
        FieldStrategy::BytesFixed => {
            if let Some(BytesSpec::Fixed { size }) = &f.bytes_spec {
                out.push_str(&format!("{indent}len += {size};\n"));
            }
        }
        FieldStrategy::BytesLength | FieldStrategy::BytesRemaining | FieldStrategy::BytesLor => {
            if let Some(ref hint) = f.asn1_hint {
                out.push_str(&format!(
                    "{indent}len += {}::encode({field_name}).map(|b| b.len()).unwrap_or(0);\n",
                    hint.encoding
                ));
            } else {
                out.push_str(&format!("{indent}len += {field_name}.len();\n"));
            }
        }
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            match inner_wt {
                WireType::Bytes => {
                    out.push_str(&format!(
                        "{indent}if let Some(v) = {field_name} {{ len += v.len(); }}\n"
                    ));
                }
                WireType::Struct(_) | WireType::Frame(_) | WireType::Capsule(_) => {
                    out.push_str(&format!(
                        "{indent}if let Some(v) = {field_name} {{ len += v.serialized_len(); }}\n"
                    ));
                }
                WireType::VarInt | WireType::ContVarInt => {
                    if let Some(ref ref_name) = f.ref_type_name {
                        let wire_size_fn =
                            format!("{}_wire_size", crate::names::to_snake_case(ref_name));
                        out.push_str(&format!(
                            "{indent}if let Some(v) = {field_name} {{ len += {wire_size_fn}(*v); }}\n"
                        ));
                    } else {
                        out.push_str(&format!(
                            "{indent}if {field_name}.is_some() {{ len += 8; }}\n"
                        ));
                    }
                }
                _ => {
                    if let Some(w) = wire_type_byte_width(inner_wt) {
                        out.push_str(&format!(
                            "{indent}if {field_name}.is_some() {{ len += {w}; }}\n"
                        ));
                    }
                }
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec
                && let Some(w) = wire_type_byte_width(&arr.element_wire_type)
            {
                out.push_str(&format!("{indent}len += *{count_name} * {w};\n"));
            } else if f.array_spec.is_some() {
                out.push_str(&format!("{indent}for _i in 0..*{count_name} {{\n"));
                out.push_str(&format!(
                    "{indent}    len += {field_name}[_i].serialized_len();\n"
                ));
                out.push_str(&format!("{indent}}}\n"));
            }
        }
        FieldStrategy::Struct => {
            out.push_str(&format!("{indent}len += {field_name}.serialized_len();\n"));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = f.ref_type_name {
                let wire_size_fn = format!("{}_wire_size", crate::names::to_snake_case(ref_name));
                out.push_str(&format!("{indent}len += {wire_size_fn}(*{field_name});\n"));
            } else {
                out.push_str(&format!("{indent}len += 8; /* varint */\n"));
            }
        }
    }
}
