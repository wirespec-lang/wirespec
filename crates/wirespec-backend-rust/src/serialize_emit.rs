// crates/wirespec-backend-rust/src/serialize_emit.rs
//
// Per-field serialize code generation for Rust (writer writes).

use wirespec_codec::ir::*;

use crate::names::to_pascal_case;
use crate::type_map::*;

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

    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
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

fn emit_field_serialize(
    out: &mut String,
    f: &CodecField,
    all_fields: &[CodecField],
    indent: &str,
    val_prefix: &str,
    emitted_bitgroups: &mut Vec<u32>,
) {
    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let write_method = writer_write_method(&f.wire_type, f.endianness);
            out.push_str(&format!(
                "{indent}w.{write_method}({val_prefix}{})?;\n",
                f.name
            ));
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
            out.push_str(&format!(
                "{indent}w.write_bytes({val_prefix}{})?;\n",
                f.name
            ));
        }
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            match inner_wt {
                WireType::Bytes => {
                    out.push_str(&format!(
                        "{indent}if let Some(ref val) = {val_prefix}{} {{ w.write_bytes(val)?; }}\n",
                        f.name
                    ));
                }
                WireType::Struct(_) => {
                    out.push_str(&format!(
                        "{indent}if let Some(ref val) = {val_prefix}{} {{ val.serialize(w)?; }}\n",
                        f.name
                    ));
                }
                _ => {
                    let write_method = writer_write_method(inner_wt, f.endianness);
                    out.push_str(&format!(
                        "{indent}if let Some(ref val) = {val_prefix}{} {{ w.{write_method}(*val)?; }}\n",
                        f.name
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
            out.push_str(&format!("{indent}{val_prefix}{}.serialize(w)?;\n", f.name));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = f.ref_type_name {
                let serialize_fn = format!("{}_serialize", crate::names::to_snake_case(ref_name));
                out.push_str(&format!(
                    "{indent}{serialize_fn}({val_prefix}{}, w)?;\n",
                    f.name
                ));
            } else {
                out.push_str(&format!(
                    "{indent}w.write_u64be({val_prefix}{})?;\n",
                    f.name
                ));
            }
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
            out.push_str(&format!(
                    "{indent}{var_name} |= (({val_prefix}{} as {container_type}) & 0x{mask:x}) << {shift};\n",
                    f.name
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
    out.push_str(&format!(
        "{indent}for _i in 0..{val_prefix}{}_count {{\n",
        f.name
    ));

    match arr.element_strategy {
        FieldStrategy::Primitive => {
            let write_method = writer_write_method(&arr.element_wire_type, None);
            out.push_str(&format!(
                "{indent}    w.{write_method}({val_prefix}{}[_i])?;\n",
                f.name
            ));
        }
        FieldStrategy::Struct => {
            out.push_str(&format!(
                "{indent}    {val_prefix}{}[_i].serialize(w)?;\n",
                f.name
            ));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = arr.element_ref_type_name {
                let serialize_fn = format!("{}_serialize", crate::names::to_snake_case(ref_name));
                out.push_str(&format!(
                    "{indent}    {serialize_fn}({val_prefix}{}[_i], w)?;\n",
                    f.name
                ));
            }
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
            out.push_str(&format!("{indent}len += {val_prefix}{}.len();\n", f.name));
        }
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            match inner_wt {
                WireType::Bytes => {
                    out.push_str(&format!(
                        "{indent}if let Some(ref b) = {val_prefix}{} {{ len += b.len(); }}\n",
                        f.name
                    ));
                }
                WireType::Struct(_) => {
                    out.push_str(&format!(
                        "{indent}if let Some(ref v) = {val_prefix}{} {{ len += v.serialized_len(); }}\n",
                        f.name
                    ));
                }
                _ => {
                    if let Some(w) = wire_type_byte_width(inner_wt) {
                        out.push_str(&format!(
                            "{indent}if {val_prefix}{}.is_some() {{ len += {w}; }}\n",
                            f.name
                        ));
                    }
                }
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                if let Some(w) = wire_type_byte_width(&arr.element_wire_type) {
                    out.push_str(&format!(
                        "{indent}len += {val_prefix}{}_count * {w};\n",
                        f.name
                    ));
                } else {
                    // Variable-size elements
                    out.push_str(&format!(
                        "{indent}for _i in 0..{val_prefix}{}_count {{\n",
                        f.name
                    ));
                    out.push_str(&format!(
                        "{indent}    len += {val_prefix}{}[_i].serialized_len();\n",
                        f.name
                    ));
                    out.push_str(&format!("{indent}}}\n"));
                }
            }
        }
        FieldStrategy::Struct => {
            out.push_str(&format!(
                "{indent}len += {val_prefix}{}.serialized_len();\n",
                f.name
            ));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = f.ref_type_name {
                let wire_size_fn = format!("{}_wire_size", crate::names::to_snake_case(ref_name));
                out.push_str(&format!(
                    "{indent}len += {wire_size_fn}({val_prefix}{});\n",
                    f.name
                ));
            } else {
                out.push_str(&format!("{indent}len += 8; /* varint */\n"));
            }
        }
    }
}

/// Emit serialize body for frame variants (Rust enum match).
pub fn emit_frame_serialize_body(out: &mut String, frame: &CodecFrame, indent: &str) {
    // Write the tag first
    let tag_write_method = writer_write_method(&frame.tag.wire_type, frame.tag.endianness);
    out.push_str(&format!("{indent}w.{tag_write_method}(tag)?;\n"));

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
    // Tag length
    if let Some(w) = wire_type_byte_width(&frame.tag.wire_type) {
        out.push_str(&format!("{indent}len += {w};\n"));
    }

    // Match on self for variant-specific lengths
    out.push_str(&format!("{indent}match self {{\n"));

    for variant in &frame.variants {
        let variant_name = to_pascal_case(&variant.name);

        if variant.fields.is_empty() {
            out.push_str(&format!("{indent}    Self::{variant_name} => {{}}\n"));
        } else {
            out.push_str(&format!("{indent}    Self::{variant_name} {{ "));
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
                    names.push(f.name.clone());
                    if f.strategy == FieldStrategy::Array {
                        names.push(format!("{}_count", f.name));
                    }
                }
            }
            CodecItem::Derived(d) => {
                names.push(d.name.clone());
            }
            CodecItem::Require(_) => {}
        }
    }
    names
}

fn emit_variant_serialize_fields(out: &mut String, variant: &CodecVariantScope, indent: &str) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    for item in &variant.items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = variant.fields.iter().find(|f| &f.field_id == field_id) {
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
    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let write_method = writer_write_method(&f.wire_type, f.endianness);
            out.push_str(&format!("{indent}w.{write_method}(*{})?;\n", f.name));
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
                        out.push_str(&format!(
                                    "{indent}{var_name} |= ((*{} as {container_type}) & 0x{mask:x}) << {shift};\n",
                                    af.name
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
            out.push_str(&format!("{indent}w.write_bytes({})?\n;", f.name));
        }
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            let write_method = writer_write_method(inner_wt, f.endianness);
            out.push_str(&format!(
                "{indent}if let Some(ref val) = {} {{ w.{write_method}(*val)?; }}\n",
                f.name
            ));
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                out.push_str(&format!("{indent}for _i in 0..*{}_count {{\n", f.name));
                match arr.element_strategy {
                    FieldStrategy::Primitive => {
                        let write_method = writer_write_method(&arr.element_wire_type, None);
                        out.push_str(&format!("{indent}    w.{write_method}({}[_i])?;\n", f.name));
                    }
                    FieldStrategy::Struct => {
                        out.push_str(&format!("{indent}    {}[_i].serialize(w)?;\n", f.name));
                    }
                    _ => {}
                }
                out.push_str(&format!("{indent}}}\n"));
            }
        }
        FieldStrategy::Struct => {
            out.push_str(&format!("{indent}{}.serialize(w)?;\n", f.name));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            out.push_str(&format!("{indent}w.write_u64be(*{})?;\n", f.name));
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
            out.push_str(&format!("{indent}len += {}.len();\n", f.name));
        }
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            if let Some(w) = wire_type_byte_width(inner_wt) {
                out.push_str(&format!(
                    "{indent}if {}.is_some() {{ len += {w}; }}\n",
                    f.name
                ));
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec
                && let Some(w) = wire_type_byte_width(&arr.element_wire_type)
            {
                out.push_str(&format!("{indent}len += *{}_count * {w};\n", f.name));
            }
        }
        FieldStrategy::Struct => {
            out.push_str(&format!("{indent}len += {}.serialized_len();\n", f.name));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = f.ref_type_name {
                let wire_size_fn = format!("{}_wire_size", crate::names::to_snake_case(ref_name));
                out.push_str(&format!("{indent}len += {wire_size_fn}(*{});\n", f.name));
            } else {
                out.push_str(&format!("{indent}len += 8; /* varint */\n"));
            }
        }
    }
}
