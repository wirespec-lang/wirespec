// crates/wirespec-backend-c/src/serialize_emit.rs
//
// Per-field serialize code generation (buffer writes).

use wirespec_codec::ir::*;

use crate::names::*;
use crate::type_map::*;

/// Returns true if the wire type is a signed integer that needs a cast
/// when passed to the unsigned write functions.
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

/// Emit serialize body for a list of items.
pub fn emit_serialize_items(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    prefix: &str,
    indent: &str,
) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    emit_field_serialize(out, f, fields, prefix, indent, &mut emitted_bitgroups);
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
    prefix: &str,
    indent: &str,
    emitted_bitgroups: &mut Vec<u32>,
) {
    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let write_fn_name = write_fn(&f.wire_type, f.endianness);
            if needs_signed_cast(&f.wire_type) {
                let utype = unsigned_c_type(&f.wire_type);
                out.push_str(&format!(
                    "{indent}r = {write_fn_name}(buf, cap, &pos, ({utype})val->{});\n",
                    f.name
                ));
            } else {
                out.push_str(&format!(
                    "{indent}r = {write_fn_name}(buf, cap, &pos, val->{});\n",
                    f.name
                ));
            }
            out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
        }
        FieldStrategy::BitGroup => {
            if let Some(ref bg) = f.bitgroup_member {
                if !emitted_bitgroups.contains(&bg.group_id) {
                    emitted_bitgroups.push(bg.group_id);
                    emit_bitgroup_serialize(out, all_fields, prefix, indent, bg);
                }
            }
        }
        FieldStrategy::BytesFixed
        | FieldStrategy::BytesLength
        | FieldStrategy::BytesRemaining
        | FieldStrategy::BytesLor => {
            out.push_str(&format!(
                "{indent}r = wirespec_write_bytes(buf, cap, &pos, val->{0}.ptr, val->{0}.len);\n",
                f.name
            ));
            out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
        }
        FieldStrategy::Conditional => {
            out.push_str(&format!("{indent}if (val->has_{}) {{\n", f.name));
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            // Check if the inner type is a struct or VarInt reference
            if f.ref_type_name.is_some() && matches!(inner_wt, WireType::Struct(_)) {
                let ref_name = f.ref_type_name.as_ref().unwrap();
                let serialize_fn = c_func_name(prefix, ref_name, "serialize");
                out.push_str(&format!(
                    "{indent}    {{\n{indent}        size_t _written = 0;\n"
                ));
                out.push_str(&format!(
                    "{indent}        r = {serialize_fn}(&val->{}, buf + pos, cap - pos, &_written);\n",
                    f.name
                ));
                out.push_str(&format!(
                    "{indent}        if (r != WIRESPEC_OK) return r;\n"
                ));
                out.push_str(&format!("{indent}        pos += _written;\n"));
                out.push_str(&format!("{indent}    }}\n"));
            } else if f.ref_type_name.is_some()
                && matches!(inner_wt, WireType::VarInt | WireType::ContVarInt)
            {
                let ref_name = f.ref_type_name.as_ref().unwrap();
                let serialize_fn = c_func_name(prefix, ref_name, "serialize");
                out.push_str(&format!(
                    "{indent}    {{\n{indent}        size_t _written = 0;\n"
                ));
                out.push_str(&format!(
                    "{indent}        r = {serialize_fn}(val->{}, buf + pos, cap - pos, &_written);\n",
                    f.name
                ));
                out.push_str(&format!(
                    "{indent}        if (r != WIRESPEC_OK) return r;\n"
                ));
                out.push_str(&format!("{indent}        pos += _written;\n"));
                out.push_str(&format!("{indent}    }}\n"));
            } else {
                let write_fn_name = write_fn(inner_wt, f.endianness);
                out.push_str(&format!(
                    "{indent}    r = {write_fn_name}(buf, cap, &pos, val->{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
            }
            out.push_str(&format!("{indent}}}\n"));
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                emit_array_serialize(out, f, arr, prefix, indent);
            }
        }
        FieldStrategy::Struct => {
            if let Some(ref ref_name) = f.ref_type_name {
                let serialize_fn = c_func_name(prefix, ref_name, "serialize");
                out.push_str(&format!("{indent}{{\n{indent}    size_t _written = 0;\n"));
                out.push_str(&format!(
                    "{indent}    r = {serialize_fn}(&val->{}, buf + pos, cap - pos, &_written);\n",
                    f.name
                ));
                out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
                out.push_str(&format!("{indent}    pos += _written;\n"));
                out.push_str(&format!("{indent}}}\n"));
            }
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = f.ref_type_name {
                let serialize_fn = c_func_name(prefix, ref_name, "serialize");
                out.push_str(&format!("{indent}{{\n{indent}    size_t _written = 0;\n"));
                out.push_str(&format!(
                    "{indent}    r = {serialize_fn}(val->{}, buf + pos, cap - pos, &_written);\n",
                    f.name
                ));
                out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
                out.push_str(&format!("{indent}    pos += _written;\n"));
                out.push_str(&format!("{indent}}}\n"));
            }
        }
    }
}

fn emit_bitgroup_serialize(
    out: &mut String,
    all_fields: &[CodecField],
    _prefix: &str,
    indent: &str,
    bg: &BitgroupMember,
) {
    let group_id = bg.group_id;
    let total_bits = bg.total_bits;
    let container_type = bitgroup_c_type(total_bits);
    let write_fn_name = bitgroup_write_fn(total_bits, bg.group_endianness);
    let var_name = format!("_bg{group_id}");

    out.push_str(&format!("{indent}{{\n"));
    out.push_str(&format!("{indent}    {container_type} {var_name} = 0;\n"));

    // Combine all fields in this group with shift+OR
    for f in all_fields {
        if let Some(ref mbg) = f.bitgroup_member {
            if mbg.group_id == group_id {
                let mask = (1u64 << mbg.member_width_bits) - 1;
                let shift = mbg.member_offset_bits;
                let cast_type = container_type;
                out.push_str(&format!(
                    "{indent}    {var_name} |= (({cast_type})(val->{} & 0x{mask:x})) << {shift};\n",
                    f.name
                ));
            }
        }
    }

    out.push_str(&format!(
        "{indent}    r = {write_fn_name}(buf, cap, &pos, {var_name});\n"
    ));
    out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
    out.push_str(&format!("{indent}}}\n"));
}

fn emit_array_serialize(
    out: &mut String,
    f: &CodecField,
    arr: &ArraySpec,
    prefix: &str,
    indent: &str,
) {
    out.push_str(&format!(
        "{indent}for (uint32_t _i = 0; _i < val->{}_count; _i++) {{\n",
        f.name
    ));

    match arr.element_strategy {
        FieldStrategy::Primitive => {
            let write_fn_name = write_fn(&arr.element_wire_type, None);
            out.push_str(&format!(
                "{indent}    r = {write_fn_name}(buf, cap, &pos, val->{}[_i]);\n",
                f.name
            ));
            out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
        }
        FieldStrategy::Struct => {
            if let Some(ref ref_name) = arr.element_ref_type_name {
                let serialize_fn = c_func_name(prefix, ref_name, "serialize");
                out.push_str(&format!(
                    "{indent}    {{\n{indent}        size_t _written = 0;\n"
                ));
                out.push_str(&format!(
                    "{indent}        r = {serialize_fn}(&val->{}[_i], buf + pos, cap - pos, &_written);\n",
                    f.name
                ));
                out.push_str(&format!(
                    "{indent}        if (r != WIRESPEC_OK) return r;\n"
                ));
                out.push_str(&format!("{indent}        pos += _written;\n"));
                out.push_str(&format!("{indent}    }}\n"));
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

/// Emit serialize body for frame variants.
pub fn emit_frame_serialize_body(out: &mut String, frame: &CodecFrame, prefix: &str, indent: &str) {
    // Write the raw tag value once before the switch
    match &frame.tag.wire_type {
        WireType::VarInt | WireType::ContVarInt => {
            // VarInt tags use the VarInt serialize function
            let ref_name = frame
                .tag
                .ref_type_name
                .as_deref()
                .unwrap_or(&frame.tag.field_name);
            let varint_fn = crate::names::c_func_name(prefix, ref_name, "serialize");
            out.push_str(&format!(
                "{indent}{{\n{indent}    size_t _written;\n{indent}    r = {varint_fn}(val->frame_type, buf + pos, cap - pos, &_written);\n{indent}    if (r != WIRESPEC_OK) return r;\n{indent}    pos += _written;\n{indent}}}\n"
            ));
        }
        _ => {
            let write_fn_name = write_fn(&frame.tag.wire_type, frame.tag.endianness);
            if needs_signed_cast(&frame.tag.wire_type) {
                let utype = unsigned_c_type(&frame.tag.wire_type);
                out.push_str(&format!(
                    "{indent}r = {write_fn_name}(buf, cap, &pos, ({utype})val->frame_type);\n"
                ));
            } else {
                out.push_str(&format!(
                    "{indent}r = {write_fn_name}(buf, cap, &pos, val->frame_type);\n"
                ));
            }
            out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
        }
    }

    // Switch on tag enum for variant-specific fields
    out.push_str(&format!("{indent}switch (val->tag) {{\n"));
    for variant in &frame.variants {
        let tag_val = c_frame_tag_value(prefix, &frame.name, &variant.name);
        let vname = to_snake_case(&variant.name);
        let inner_indent = format!("{indent}        ");

        out.push_str(&format!("{indent}    case {tag_val}: {{\n"));

        // Serialize variant fields (no per-case tag write)
        emit_variant_serialize_items_public(
            out,
            &variant.fields,
            &variant.items,
            prefix,
            &inner_indent,
            &format!("val->value.{vname}."),
        );

        out.push_str(&format!("{inner_indent}break;\n"));
        out.push_str(&format!("{indent}    }}\n"));
    }
    out.push_str(&format!("{indent}    default: break;\n"));
    out.push_str(&format!("{indent}}}\n"));
}

/// Emit serialize items for variant fields (using a variant-specific prefix).
pub fn emit_variant_serialize_items_public(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    prefix: &str,
    indent: &str,
    val_prefix: &str,
) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    emit_variant_field_serialize(
                        out,
                        f,
                        fields,
                        prefix,
                        indent,
                        val_prefix,
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
    prefix: &str,
    indent: &str,
    val_prefix: &str,
    emitted_bitgroups: &mut Vec<u32>,
) {
    match f.strategy {
        FieldStrategy::Primitive | FieldStrategy::Checksum => {
            let write_fn_name = write_fn(&f.wire_type, f.endianness);
            if needs_signed_cast(&f.wire_type) {
                let utype = unsigned_c_type(&f.wire_type);
                out.push_str(&format!(
                    "{indent}r = {write_fn_name}(buf, cap, &pos, ({utype}){val_prefix}{});\n",
                    f.name
                ));
            } else {
                out.push_str(&format!(
                    "{indent}r = {write_fn_name}(buf, cap, &pos, {val_prefix}{});\n",
                    f.name
                ));
            }
            out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
        }
        FieldStrategy::BitGroup => {
            if let Some(ref bg) = f.bitgroup_member {
                if !emitted_bitgroups.contains(&bg.group_id) {
                    emitted_bitgroups.push(bg.group_id);
                    // Emit bitgroup serialize with variant prefix
                    let group_id = bg.group_id;
                    let total_bits = bg.total_bits;
                    let container_type = bitgroup_c_type(total_bits);
                    let write_fn_name = bitgroup_write_fn(total_bits, bg.group_endianness);
                    let var_name = format!("_bg{group_id}");

                    out.push_str(&format!("{indent}{{\n"));
                    out.push_str(&format!("{indent}    {container_type} {var_name} = 0;\n"));

                    for af in all_fields {
                        if let Some(ref mbg) = af.bitgroup_member {
                            if mbg.group_id == group_id {
                                let mask = (1u64 << mbg.member_width_bits) - 1;
                                let shift = mbg.member_offset_bits;
                                out.push_str(&format!(
                                    "{indent}    {var_name} |= (({container_type})({val_prefix}{} & 0x{mask:x})) << {shift};\n",
                                    af.name
                                ));
                            }
                        }
                    }

                    out.push_str(&format!(
                        "{indent}    r = {write_fn_name}(buf, cap, &pos, {var_name});\n"
                    ));
                    out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
                    out.push_str(&format!("{indent}}}\n"));
                }
            }
        }
        FieldStrategy::BytesFixed
        | FieldStrategy::BytesLength
        | FieldStrategy::BytesRemaining
        | FieldStrategy::BytesLor => {
            out.push_str(&format!(
                "{indent}r = wirespec_write_bytes(buf, cap, &pos, {val_prefix}{0}.ptr, {val_prefix}{0}.len);\n",
                f.name
            ));
            out.push_str(&format!("{indent}if (r != WIRESPEC_OK) return r;\n"));
        }
        FieldStrategy::Conditional => {
            out.push_str(&format!("{indent}if ({val_prefix}has_{}) {{\n", f.name));
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            // Check if the inner type is a struct or VarInt reference
            if f.ref_type_name.is_some() && matches!(inner_wt, WireType::Struct(_)) {
                let ref_name = f.ref_type_name.as_ref().unwrap();
                let serialize_fn = c_func_name(prefix, ref_name, "serialize");
                out.push_str(&format!(
                    "{indent}    {{\n{indent}        size_t _written = 0;\n"
                ));
                out.push_str(&format!(
                    "{indent}        r = {serialize_fn}(&{val_prefix}{}, buf + pos, cap - pos, &_written);\n",
                    f.name
                ));
                out.push_str(&format!(
                    "{indent}        if (r != WIRESPEC_OK) return r;\n"
                ));
                out.push_str(&format!("{indent}        pos += _written;\n"));
                out.push_str(&format!("{indent}    }}\n"));
            } else if f.ref_type_name.is_some()
                && matches!(inner_wt, WireType::VarInt | WireType::ContVarInt)
            {
                let ref_name = f.ref_type_name.as_ref().unwrap();
                let serialize_fn = c_func_name(prefix, ref_name, "serialize");
                out.push_str(&format!(
                    "{indent}    {{\n{indent}        size_t _written = 0;\n"
                ));
                out.push_str(&format!(
                    "{indent}        r = {serialize_fn}({val_prefix}{}, buf + pos, cap - pos, &_written);\n",
                    f.name
                ));
                out.push_str(&format!(
                    "{indent}        if (r != WIRESPEC_OK) return r;\n"
                ));
                out.push_str(&format!("{indent}        pos += _written;\n"));
                out.push_str(&format!("{indent}    }}\n"));
            } else {
                let write_fn_name = write_fn(inner_wt, f.endianness);
                out.push_str(&format!(
                    "{indent}    r = {write_fn_name}(buf, cap, &pos, {val_prefix}{});\n",
                    f.name
                ));
                out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
            }
            out.push_str(&format!("{indent}}}\n"));
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                out.push_str(&format!(
                    "{indent}for (uint32_t _i = 0; _i < {val_prefix}{}_count; _i++) {{\n",
                    f.name
                ));
                match arr.element_strategy {
                    FieldStrategy::Primitive => {
                        let write_fn_name = write_fn(&arr.element_wire_type, None);
                        out.push_str(&format!(
                            "{indent}    r = {write_fn_name}(buf, cap, &pos, {val_prefix}{}[_i]);\n",
                            f.name
                        ));
                        out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
                    }
                    FieldStrategy::Struct => {
                        if let Some(ref ref_name) = arr.element_ref_type_name {
                            let serialize_fn = c_func_name(prefix, ref_name, "serialize");
                            out.push_str(&format!(
                                "{indent}    {{\n{indent}        size_t _written = 0;\n"
                            ));
                            out.push_str(&format!(
                                "{indent}        r = {serialize_fn}(&{val_prefix}{}[_i], buf + pos, cap - pos, &_written);\n",
                                f.name
                            ));
                            out.push_str(&format!(
                                "{indent}        if (r != WIRESPEC_OK) return r;\n"
                            ));
                            out.push_str(&format!("{indent}        pos += _written;\n"));
                            out.push_str(&format!("{indent}    }}\n"));
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
        FieldStrategy::Struct => {
            if let Some(ref ref_name) = f.ref_type_name {
                let serialize_fn = c_func_name(prefix, ref_name, "serialize");
                out.push_str(&format!("{indent}{{\n{indent}    size_t _written = 0;\n"));
                out.push_str(&format!(
                    "{indent}    r = {serialize_fn}(&{val_prefix}{}, buf + pos, cap - pos, &_written);\n",
                    f.name
                ));
                out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
                out.push_str(&format!("{indent}    pos += _written;\n"));
                out.push_str(&format!("{indent}}}\n"));
            }
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            if let Some(ref ref_name) = f.ref_type_name {
                let serialize_fn = c_func_name(prefix, ref_name, "serialize");
                out.push_str(&format!("{indent}{{\n{indent}    size_t _written = 0;\n"));
                out.push_str(&format!(
                    "{indent}    r = {serialize_fn}({val_prefix}{}, buf + pos, cap - pos, &_written);\n",
                    f.name
                ));
                out.push_str(&format!("{indent}    if (r != WIRESPEC_OK) return r;\n"));
                out.push_str(&format!("{indent}    pos += _written;\n"));
                out.push_str(&format!("{indent}}}\n"));
            }
        }
    }
}

/// Emit serialized_len body for fields.
pub fn emit_serialized_len_items(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    prefix: &str,
    indent: &str,
    val_prefix: &str,
) {
    let mut emitted_bitgroups: Vec<u32> = Vec::new();

    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    emit_field_serialized_len(
                        out,
                        f,
                        fields,
                        prefix,
                        indent,
                        val_prefix,
                        &mut emitted_bitgroups,
                    );
                }
            }
            CodecItem::Derived(_) | CodecItem::Require(_) => {}
        }
    }
}

fn emit_field_serialized_len(
    out: &mut String,
    f: &CodecField,
    _all_fields: &[CodecField],
    prefix: &str,
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
            if let Some(ref bg) = f.bitgroup_member {
                if !emitted_bitgroups.contains(&bg.group_id) {
                    emitted_bitgroups.push(bg.group_id);
                    let bytes = (bg.total_bits + 7) / 8;
                    out.push_str(&format!("{indent}len += {bytes};\n"));
                }
            }
        }
        FieldStrategy::BytesFixed => {
            if let Some(BytesSpec::Fixed { size }) = &f.bytes_spec {
                out.push_str(&format!("{indent}len += {size};\n"));
            }
        }
        FieldStrategy::BytesLength | FieldStrategy::BytesRemaining | FieldStrategy::BytesLor => {
            out.push_str(&format!("{indent}len += {val_prefix}{}.len;\n", f.name));
        }
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            if let Some(w) = wire_type_byte_width(inner_wt) {
                out.push_str(&format!(
                    "{indent}if ({val_prefix}has_{}) len += {w};\n",
                    f.name
                ));
            } else if f.ref_type_name.is_some() && matches!(inner_wt, WireType::Struct(_)) {
                let ref_name = f.ref_type_name.as_ref().unwrap();
                let len_fn = c_func_name(prefix, ref_name, "serialized_len");
                out.push_str(&format!(
                    "{indent}if ({val_prefix}has_{}) len += {len_fn}(&{val_prefix}{});\n",
                    f.name, f.name
                ));
            } else if f.ref_type_name.is_some()
                && matches!(inner_wt, WireType::VarInt | WireType::ContVarInt)
            {
                let ref_name = f.ref_type_name.as_ref().unwrap();
                let len_fn = c_func_name(prefix, ref_name, "wire_size");
                out.push_str(&format!(
                    "{indent}if ({val_prefix}has_{}) len += {len_fn}({val_prefix}{});\n",
                    f.name, f.name
                ));
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                if let Some(w) = wire_type_byte_width(&arr.element_wire_type) {
                    out.push_str(&format!(
                        "{indent}len += {val_prefix}{}_count * {w};\n",
                        f.name
                    ));
                } else if let Some(ref ref_name) = arr.element_ref_type_name {
                    let len_fn = c_func_name(prefix, ref_name, "serialized_len");
                    out.push_str(&format!(
                        "{indent}for (uint32_t _i = 0; _i < {val_prefix}{}_count; _i++) {{\n",
                        f.name
                    ));
                    out.push_str(&format!(
                        "{indent}    len += {len_fn}(&{val_prefix}{}[_i]);\n",
                        f.name
                    ));
                    out.push_str(&format!("{indent}}}\n"));
                }
            }
        }
        FieldStrategy::Struct => {
            if let Some(ref ref_name) = f.ref_type_name {
                let len_fn = c_func_name(prefix, ref_name, "serialized_len");
                out.push_str(&format!(
                    "{indent}len += {len_fn}(&{val_prefix}{});\n",
                    f.name
                ));
            }
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            // VarInt has variable length, use wire_size (takes value, not pointer)
            if let Some(ref ref_name) = f.ref_type_name {
                let len_fn = c_func_name(prefix, ref_name, "wire_size");
                out.push_str(&format!(
                    "{indent}len += {len_fn}({val_prefix}{});\n",
                    f.name
                ));
            }
        }
    }
}
