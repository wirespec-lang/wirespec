// crates/wirespec-backend-rust/src/emit.rs
//
// Full .rs file emission: structs, enums, impl blocks, frames, capsules,
// varints, state machines.

use crate::names::*;
use crate::parse_emit;
use crate::serialize_emit;
use crate::type_map::*;
use wirespec_codec::ir::*;
use wirespec_sema::expr::{SemanticExpr, SemanticLiteral, TransitionPeerKind};
use wirespec_sema::ir::*;
use wirespec_sema::types::PrimitiveWireType;

const MAX_ARRAY_ELEMENTS: u32 = 256;

/// Resolve enum fields: change strategy from Struct to Primitive and set
/// wire_type to the enum's underlying primitive type. This allows the
/// standard primitive read/write codegen path to handle enum fields.
/// Also resolves enum types in array element specs.
fn resolve_enum_fields(fields: &[CodecField], enums: &[SemanticEnum]) -> Vec<CodecField> {
    fields
        .iter()
        .map(|f| {
            if f.strategy == FieldStrategy::Struct
                && let WireType::Enum(ref name) = f.wire_type
                && let Some(e) = enums.iter().find(|e| &e.name == name)
            {
                let underlying_wt = semantic_type_to_wire_type_simple(&e.underlying_type);
                let mut f2 = f.clone();
                f2.strategy = FieldStrategy::Primitive;
                f2.wire_type = underlying_wt;
                f2.ref_type_name = None;
                // Propagate endianness from the enum's underlying type
                if let wirespec_sema::types::SemanticType::Primitive { endianness, .. } =
                    &e.underlying_type
                {
                    f2.endianness = *endianness;
                }
                return f2;
            }
            // Also resolve enum types in array element specs
            if f.strategy == FieldStrategy::Array
                && let Some(ref arr) = f.array_spec
                && let WireType::Enum(ref name) = arr.element_wire_type
                && let Some(e) = enums.iter().find(|e| &e.name == name)
            {
                let underlying_wt = semantic_type_to_wire_type_simple(&e.underlying_type);
                let mut f2 = f.clone();
                let mut arr2 = arr.clone();
                arr2.element_wire_type = underlying_wt;
                arr2.element_strategy = FieldStrategy::Primitive;
                arr2.element_ref_type_name = None;
                f2.array_spec = Some(arr2);
                return f2;
            }
            f.clone()
        })
        .collect()
}

/// Simple mapping from SemanticType (Primitive only) to WireType.
fn semantic_type_to_wire_type_simple(ty: &wirespec_sema::types::SemanticType) -> WireType {
    use wirespec_sema::types::SemanticType;
    match ty {
        SemanticType::Primitive { wire, .. } => match wire {
            PrimitiveWireType::U8 => WireType::U8,
            PrimitiveWireType::U16 => WireType::U16,
            PrimitiveWireType::U24 => WireType::U24,
            PrimitiveWireType::U32 => WireType::U32,
            PrimitiveWireType::U64 => WireType::U64,
            PrimitiveWireType::I8 => WireType::I8,
            PrimitiveWireType::I16 => WireType::I16,
            PrimitiveWireType::I32 => WireType::I32,
            PrimitiveWireType::I64 => WireType::I64,
            PrimitiveWireType::Bool => WireType::Bool,
            PrimitiveWireType::Bit => WireType::Bit,
        },
        _ => {
            // After sema validation, non-integer underlying types are rejected,
            // so this branch is truly unreachable.
            unreachable!("unexpected non-primitive enum underlying type: {:?}", ty)
        }
    }
}

/// Resolve enum fields in variant scopes (for frames and capsules).
fn resolve_enum_variants(
    variants: &[CodecVariantScope],
    enums: &[SemanticEnum],
) -> Vec<CodecVariantScope> {
    variants
        .iter()
        .map(|v| {
            let mut v2 = v.clone();
            v2.fields = resolve_enum_fields(&v.fields, enums);
            v2
        })
        .collect()
}

/// Check if any field in a list uses bytes (needs lifetime `<'a>`).
fn fields_need_lifetime(fields: &[CodecField]) -> bool {
    for f in fields {
        match f.strategy {
            FieldStrategy::BytesFixed
            | FieldStrategy::BytesLength
            | FieldStrategy::BytesRemaining
            | FieldStrategy::BytesLor => {
                if f.asn1_hint.is_some() {
                    continue; // ASN.1 decoded fields are owned, no lifetime
                }
                return true;
            }
            FieldStrategy::Conditional => {
                let inner = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
                if matches!(inner, WireType::Bytes) {
                    return true;
                }
                if wire_type_is_named(inner) {
                    // Conservatively assume named types in conditionals may have lifetime
                    return true;
                }
            }
            FieldStrategy::Struct => {
                // Enum refs are type aliases for primitives, no lifetime needed
                if matches!(f.wire_type, WireType::Enum(_)) {
                    continue;
                }
                // Named struct types might have lifetimes; conservative
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Check if a packet needs lifetime.
fn packet_needs_lifetime(packet: &CodecPacket) -> bool {
    fields_need_lifetime(&packet.fields)
}

/// Check if a frame needs lifetime.
fn frame_needs_lifetime(frame: &CodecFrame) -> bool {
    for variant in &frame.variants {
        if fields_need_lifetime(&variant.fields) {
            return true;
        }
    }
    false
}

/// Check if a capsule needs lifetime.
fn capsule_needs_lifetime(capsule: &CodecCapsule) -> bool {
    if fields_need_lifetime(&capsule.header_fields) {
        return true;
    }
    for variant in &capsule.variants {
        if fields_need_lifetime(&variant.fields) {
            return true;
        }
    }
    false
}

/// Lifetime suffix: `<'a>` if needed, empty otherwise.
fn lifetime_suffix(needs: bool) -> &'static str {
    if needs { "<'a>" } else { "" }
}

fn collect_named_types_needing_lifetime(module: &CodecModule) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();

    for packet in &module.packets {
        if packet_needs_lifetime(packet) {
            names.insert(packet.name.clone());
        }
    }
    for frame in &module.frames {
        if frame_needs_lifetime(frame) {
            names.insert(frame.name.clone());
        }
    }
    for capsule in &module.capsules {
        if capsule_needs_lifetime(capsule) {
            names.insert(capsule.name.clone());
        }
    }

    names
}

/// Emit the complete .rs source file content.
pub fn emit_source(module: &CodecModule, _prefix: &str) -> String {
    let mut out = String::new();
    let named_types_needing_lifetime = collect_named_types_needing_lifetime(module);

    // Header
    out.push_str("// Auto-generated by wirespec. Do not edit.\n");
    out.push_str("#![allow(unused_imports, unused_variables, dead_code, unused_mut, unused_parens, unreachable_patterns)]\n\n");
    out.push_str("use wirespec_rt::*;\n\n");

    // Check if any field in any packet/frame/capsule has asn1_hint
    let has_asn1 = module
        .packets
        .iter()
        .any(|p| p.fields.iter().any(|f| f.asn1_hint.is_some()))
        || module.frames.iter().any(|fr| {
            fr.variants
                .iter()
                .any(|v| v.fields.iter().any(|f| f.asn1_hint.is_some()))
        })
        || module.capsules.iter().any(|c| {
            c.header_fields.iter().any(|f| f.asn1_hint.is_some())
                || c.variants
                    .iter()
                    .any(|v| v.fields.iter().any(|f| f.asn1_hint.is_some()))
        });
    if has_asn1 {
        // Collect unique encodings from all ASN.1 hints
        let mut encodings = std::collections::BTreeSet::new();
        let all_fields_iter = module
            .packets
            .iter()
            .flat_map(|p| p.fields.iter())
            .chain(
                module
                    .frames
                    .iter()
                    .flat_map(|fr| fr.variants.iter().flat_map(|v| v.fields.iter())),
            )
            .chain(module.capsules.iter().flat_map(|c| {
                c.header_fields
                    .iter()
                    .chain(c.variants.iter().flat_map(|v| v.fields.iter()))
            }));
        for f in all_fields_iter {
            if let Some(ref hint) = f.asn1_hint {
                encodings.insert(hint.encoding.clone());
            }
        }
        for enc in &encodings {
            out.push_str(&format!("use rasn::{enc};\n"));
        }

        // Collect unique imports from ASN.1 hints
        let mut imports: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for packet in &module.packets {
            for f in &packet.fields {
                if let Some(ref hint) = f.asn1_hint
                    && let Some(ref rust_mod) = hint.rust_module
                {
                    imports
                        .entry(rust_mod.clone())
                        .or_default()
                        .push(hint.type_name.clone());
                }
            }
        }
        for frame in &module.frames {
            for v in &frame.variants {
                for f in &v.fields {
                    if let Some(ref hint) = f.asn1_hint
                        && let Some(ref rust_mod) = hint.rust_module
                    {
                        imports
                            .entry(rust_mod.clone())
                            .or_default()
                            .push(hint.type_name.clone());
                    }
                }
            }
        }
        for capsule in &module.capsules {
            for f in capsule
                .header_fields
                .iter()
                .chain(capsule.variants.iter().flat_map(|v| v.fields.iter()))
            {
                if let Some(ref hint) = f.asn1_hint
                    && let Some(ref rust_mod) = hint.rust_module
                {
                    imports
                        .entry(rust_mod.clone())
                        .or_default()
                        .push(hint.type_name.clone());
                }
            }
        }

        // Emit use statements
        for (mod_path, mut types) in imports {
            types.sort();
            types.dedup();
            if types.len() == 1 {
                out.push_str(&format!("use {}::{};\n", mod_path, types[0]));
            } else {
                out.push_str(&format!("use {}::{{{}}};\n", mod_path, types.join(", ")));
            }
        }
        out.push('\n');
    }

    // VarInts
    for vi in &module.varints {
        emit_varint(&mut out, vi);
    }

    // Constants
    for c in &module.consts {
        emit_const(&mut out, c);
    }
    if !module.consts.is_empty() {
        out.push('\n');
    }

    // Enums (as type alias + pub const values)
    for e in &module.enums {
        emit_enum(&mut out, e);
    }

    // Packets
    for packet in &module.packets {
        emit_packet(
            &mut out,
            packet,
            &module.enums,
            &named_types_needing_lifetime,
        );
    }

    // Frames
    for frame in &module.frames {
        emit_frame(
            &mut out,
            frame,
            &module.enums,
            &named_types_needing_lifetime,
        );
    }

    // Capsules
    for capsule in &module.capsules {
        emit_capsule(
            &mut out,
            capsule,
            &module.enums,
            &named_types_needing_lifetime,
        );
    }

    // State Machines
    for sm in &module.state_machines {
        emit_state_machine(&mut out, sm, &module.state_machines);
    }

    out
}

// ── Constants ──

fn emit_const(out: &mut String, c: &wirespec_sema::ir::SemanticConst) {
    let name = c.name.to_uppercase();
    let ty = semantic_type_to_rust(&c.ty);
    let val = semantic_literal_to_rust(&c.value);
    out.push_str(&format!("pub const {name}: {ty} = {val};\n"));
}

fn semantic_type_to_rust(ty: &wirespec_sema::types::SemanticType) -> String {
    use wirespec_sema::types::SemanticType;
    match ty {
        SemanticType::Primitive { wire, .. } => match wire {
            wirespec_sema::types::PrimitiveWireType::U8 => "u8",
            wirespec_sema::types::PrimitiveWireType::U16 => "u16",
            wirespec_sema::types::PrimitiveWireType::U24 => "u32",
            wirespec_sema::types::PrimitiveWireType::U32 => "u32",
            wirespec_sema::types::PrimitiveWireType::U64 => "u64",
            wirespec_sema::types::PrimitiveWireType::I8 => "i8",
            wirespec_sema::types::PrimitiveWireType::I16 => "i16",
            wirespec_sema::types::PrimitiveWireType::I32 => "i32",
            wirespec_sema::types::PrimitiveWireType::I64 => "i64",
            wirespec_sema::types::PrimitiveWireType::Bool => "bool",
            wirespec_sema::types::PrimitiveWireType::Bit => "u8",
        }
        .to_string(),
        SemanticType::Bits { width_bits } => if *width_bits <= 8 {
            "u8"
        } else if *width_bits <= 16 {
            "u16"
        } else if *width_bits <= 32 {
            "u32"
        } else {
            "u64"
        }
        .to_string(),
        SemanticType::PacketRef { name, .. } => to_pascal_case(name),
        SemanticType::FrameRef { name, .. } => to_pascal_case(name),
        SemanticType::CapsuleRef { name, .. } => to_pascal_case(name),
        SemanticType::EnumRef { name, .. } => to_pascal_case(name),
        SemanticType::VarIntRef { .. } => "u64".to_string(),
        SemanticType::Array { element_type, .. } => {
            let elem = semantic_type_to_rust(element_type);
            format!("Vec<{elem}>")
        }
        SemanticType::Bytes { .. } => "Vec<u8>".to_string(),
    }
}

/// Map a SM state field to its Rust type, using `child_sm_name` for
/// child-SM references and handling arrays of child SMs.
fn sm_field_type_to_rust(field: &wirespec_sema::ir::SemanticStateField) -> String {
    use wirespec_sema::types::SemanticType;
    match &field.ty {
        SemanticType::Array { element_type, .. } => {
            if let Some(ref child_sm) = field.child_sm_name {
                format!("Vec<{}>", to_pascal_case(child_sm))
            } else {
                let elem_ty = semantic_type_to_rust(element_type);
                format!("Vec<{elem_ty}>")
            }
        }
        _ if field.child_sm_name.is_some() => to_pascal_case(field.child_sm_name.as_ref().unwrap()),
        _ => semantic_type_to_rust(&field.ty),
    }
}

fn semantic_literal_to_rust(lit: &wirespec_sema::expr::SemanticLiteral) -> String {
    match lit {
        wirespec_sema::expr::SemanticLiteral::Int(n) => format!("{n}"),
        wirespec_sema::expr::SemanticLiteral::Bool(b) => format!("{b}"),
        wirespec_sema::expr::SemanticLiteral::String(s) => format!("{s:?}"),
        wirespec_sema::expr::SemanticLiteral::Null => "0".into(),
    }
}

// ── Enums ──

fn emit_enum(out: &mut String, e: &wirespec_sema::ir::SemanticEnum) {
    let base_type = semantic_type_to_rust(&e.underlying_type);
    let type_name = to_pascal_case(&e.name);
    // Type alias for the enum
    out.push_str(&format!("pub type {type_name} = {base_type};\n"));
    for member in &e.members {
        let const_name = format!(
            "{}_{}",
            to_snake_case(&e.name).to_uppercase(),
            to_snake_case(&member.name).to_uppercase()
        );
        out.push_str(&format!(
            "pub const {const_name}: {type_name} = {};\n",
            member.value
        ));
    }
    out.push('\n');
}

// ── Packets ──

fn emit_packet(
    out: &mut String,
    packet: &CodecPacket,
    enums: &[SemanticEnum],
    named_types_needing_lifetime: &std::collections::HashSet<String>,
) {
    let type_name = to_pascal_case(&packet.name);
    let needs_lt = packet_needs_lifetime(packet);
    let lt = lifetime_suffix(needs_lt);

    // Resolve enum fields for parse/serialize code (struct declaration keeps original types)
    let resolved_fields = resolve_enum_fields(&packet.fields, enums);

    let has_cksum = packet.checksum_plan.is_some();
    // Parse needs offset tracking for recompute-compare algorithms (CRC, fletcher)
    let parse_needs_offset = has_cksum
        && packet
            .checksum_plan
            .as_ref()
            .is_some_and(|p| p.input_model == ChecksumInputModel::RecomputeWithSkippedField);
    // Serialize always needs offset tracking when checksum is present (all algorithms need it)

    // Struct definition (uses original fields to preserve enum type aliases)
    out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
    out.push_str(&format!("pub struct {type_name}{lt} {{\n"));
    emit_struct_fields(
        out,
        &packet.fields,
        &packet.items,
        named_types_needing_lifetime,
    );
    out.push_str("}\n\n");

    // Skip Default impl when any field uses ASN.1 (no sensible default).
    let has_asn1_field = packet.fields.iter().any(|f| f.asn1_hint.is_some());
    if !has_asn1_field {
        out.push_str(&format!("impl{lt} Default for {type_name}{lt} {{\n"));
        out.push_str("    fn default() -> Self {\n");
        out.push_str("        Self {\n");
        emit_default_struct_items(out, &packet.fields, &packet.items, "            ");
        out.push_str("        }\n");
        out.push_str("    }\n");
        out.push_str("}\n\n");
    }

    // Impl block
    out.push_str(&format!("impl{lt} {type_name}{lt} {{\n"));

    // parse method (uses resolved fields)
    out.push_str(&format!(
        "    pub fn parse(cur: &mut Cursor{}) -> Result<Self> {{\n",
        if needs_lt { "<'a>" } else { "<'_>" }
    ));

    // Record start position for checksum verification
    if has_cksum {
        out.push_str("        let _start = cur.consumed();\n");
    }
    if parse_needs_offset {
        out.push_str("        let mut _cksum_offset: usize = 0;\n");
    }

    // Emit parse items, tracking checksum field offset if needed
    if parse_needs_offset {
        let cksum_plan = packet.checksum_plan.as_ref().unwrap();
        emit_parse_items_with_cksum_tracking(
            out,
            &resolved_fields,
            &packet.items,
            "        ",
            &cksum_plan.field_name,
        );
    } else {
        parse_emit::emit_parse_items(out, &resolved_fields, &packet.items, "        ");
    }

    // Checksum verify after parse, before returning
    if let Some(ref plan) = packet.checksum_plan {
        emit_checksum_verify_rust(out, plan, "        ");
    }

    // Return Ok(Self { ... })
    out.push_str("        Ok(Self {\n");
    emit_struct_init_fields(out, &resolved_fields, &packet.items, "            ");
    out.push_str("        })\n");
    out.push_str("    }\n\n");

    // serialize method (uses resolved fields)
    out.push_str("    pub fn serialize(&self, w: &mut Writer<'_>) -> Result<()> {\n");

    if has_cksum {
        out.push_str("        let _start = w.written();\n");
        out.push_str("        let mut _cksum_offset: usize = 0;\n");
        let cksum_plan = packet.checksum_plan.as_ref().unwrap();
        emit_serialize_items_with_cksum_tracking(
            out,
            &resolved_fields,
            &packet.items,
            "        ",
            "self.",
            &cksum_plan.field_name,
        );
    } else {
        serialize_emit::emit_serialize_items(
            out,
            &resolved_fields,
            &packet.items,
            "        ",
            "self.",
        );
    }

    // Checksum compute after all fields written
    if let Some(ref plan) = packet.checksum_plan {
        emit_checksum_compute_rust(out, plan, "        ");
    }

    out.push_str("        Ok(())\n");
    out.push_str("    }\n\n");

    // serialized_len method (uses resolved fields)
    let has_asn1 = resolved_fields.iter().any(|f| f.asn1_hint.is_some());
    if has_asn1 {
        out.push_str("    /// Note: for ASN.1 fields, this method encodes the payload to compute the length.\n");
        out.push_str(
            "    /// For best performance, use `serialize()` directly with a pre-allocated buffer.\n",
        );
    }
    out.push_str("    pub fn serialized_len(&self) -> usize {\n");
    out.push_str("        let mut len = 0usize;\n");
    serialize_emit::emit_serialized_len_items(
        out,
        &resolved_fields,
        &packet.items,
        "        ",
        "self.",
    );
    out.push_str("        len\n");
    out.push_str("    }\n");

    out.push_str("}\n\n");
}

/// Emit parse items while tracking the byte offset of the checksum field.
fn emit_parse_items_with_cksum_tracking(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
    cksum_field_name: &str,
) {
    for item in items {
        if let CodecItem::Field { field_id } = item
            && let Some(f) = fields.iter().find(|f| &f.field_id == field_id)
            && f.name == cksum_field_name
        {
            out.push_str(&format!(
                "{indent}_cksum_offset = cur.consumed() - _start;\n"
            ));
        }
        let single_items = [item.clone()];
        parse_emit::emit_parse_items(out, fields, &single_items, indent);
    }
}

/// Emit serialize items while tracking the byte offset of the checksum field.
fn emit_serialize_items_with_cksum_tracking(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
    val_prefix: &str,
    cksum_field_name: &str,
) {
    for item in items {
        if let CodecItem::Field { field_id } = item
            && let Some(f) = fields.iter().find(|f| &f.field_id == field_id)
            && f.name == cksum_field_name
        {
            out.push_str(&format!("{indent}_cksum_offset = w.written() - _start;\n"));
        }
        let single_items = [item.clone()];
        serialize_emit::emit_serialize_items(out, fields, &single_items, indent, val_prefix);
    }
}

/// Emit checksum verification code after parse.
fn emit_checksum_verify_rust(out: &mut String, plan: &ChecksumPlan, indent: &str) {
    use wirespec_sema::checksum_catalog;

    let algo = &plan.algorithm_id;
    let spec = checksum_catalog::lookup(algo);

    match spec.map(|s| s.verify_mode) {
        Some(checksum_catalog::ChecksumVerifyMode::ZeroSum) => {
            // Zero-sum verify: sum of entire scope including checksum field == 0
            out.push_str(&format!("{indent}{{\n"));
            out.push_str(&format!(
                "{indent}    let _cksum = {algo}_checksum(&cur.bytes()[_start..cur.consumed()]);\n"
            ));
            out.push_str(&format!(
                "{indent}    if _cksum != 0 {{ return Err(Error::Checksum); }}\n"
            ));
            out.push_str(&format!("{indent}}}\n"));
        }
        Some(checksum_catalog::ChecksumVerifyMode::RecomputeCompare) => {
            // Recompute skipping the checksum field, compare
            let width = plan.field_width_bytes;
            out.push_str(&format!("{indent}{{\n"));
            out.push_str(&format!(
                "{indent}    let _scope = &cur.bytes()[_start..cur.consumed()];\n"
            ));
            out.push_str(&format!(
                "{indent}    let _computed = {algo}_verify(_scope, _cksum_offset, {width});\n"
            ));
            out.push_str(&format!(
                "{indent}    if {field} != _computed {{ return Err(Error::Checksum); }}\n",
                field = plan.field_name,
            ));
            out.push_str(&format!("{indent}}}\n"));
        }
        None => {
            unreachable!("unknown checksum algorithm: {algo}");
        }
    }
}

/// Emit checksum compute code after serialize.
fn emit_checksum_compute_rust(out: &mut String, plan: &ChecksumPlan, indent: &str) {
    use wirespec_sema::checksum_catalog;

    let algo = &plan.algorithm_id;
    let spec = checksum_catalog::lookup(algo);

    out.push_str(&format!("{indent}// Checksum compute ({algo})\n"));

    match spec.map(|s| s.verify_mode) {
        Some(checksum_catalog::ChecksumVerifyMode::ZeroSum) => {
            // Zero-sum compute: patch-in-place
            out.push_str(&format!("{indent}{{\n"));
            out.push_str(&format!("{indent}    let _written = w.as_written_mut();\n"));
            out.push_str(&format!(
                "{indent}    {algo}_checksum_compute(&mut _written[_start..], _cksum_offset);\n"
            ));
            out.push_str(&format!("{indent}}}\n"));
        }
        Some(checksum_catalog::ChecksumVerifyMode::RecomputeCompare) => {
            // Recompute: compute value, write bytes big-endian
            let width = plan.field_width_bytes;
            out.push_str(&format!("{indent}{{\n"));
            out.push_str(&format!("{indent}    let _end = w.written();\n"));
            out.push_str(&format!("{indent}    let _written = w.as_written_mut();\n"));
            out.push_str(&format!(
                "{indent}    let _val = {algo}_compute(&mut _written[_start.._end], _cksum_offset);\n"
            ));
            for i in 0..width {
                let shift = (width - 1 - i) * 8;
                out.push_str(&format!(
                    "{indent}    _written[_start + _cksum_offset + {i}] = ((_val >> {shift}) & 0xFF) as u8;\n"
                ));
            }
            out.push_str(&format!("{indent}}}\n"));
        }
        None => {
            unreachable!("unknown checksum algorithm: {algo}");
        }
    }
}

// ── Frames ──

fn emit_frame(
    out: &mut String,
    frame: &CodecFrame,
    enums: &[SemanticEnum],
    named_types_needing_lifetime: &std::collections::HashSet<String>,
) {
    let type_name = to_pascal_case(&frame.name);
    let needs_lt = frame_needs_lifetime(frame);
    let lt = lifetime_suffix(needs_lt);
    let tag_type = wire_type_to_rust(&frame.tag.wire_type);

    // Resolve enum fields in variants for parse/serialize
    let resolved_variants = resolve_enum_variants(&frame.variants, enums);

    // Enum definition (uses original fields to preserve types)
    out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
    out.push_str(&format!("pub enum {type_name}{lt} {{\n"));
    for variant in &frame.variants {
        let variant_name = to_pascal_case(&variant.name);
        if variant.fields.is_empty()
            && !variant
                .items
                .iter()
                .any(|i| matches!(i, CodecItem::Derived(_)))
        {
            out.push_str(&format!("    {variant_name},\n"));
        } else {
            out.push_str(&format!("    {variant_name} {{\n"));
            emit_variant_struct_fields(out, variant, named_types_needing_lifetime);
            out.push_str("    },\n");
        }
    }
    out.push_str("}\n\n");

    // Skip Default impl when any variant field uses ASN.1 (no sensible default).
    let frame_has_asn1 = frame
        .variants
        .iter()
        .any(|v| v.fields.iter().any(|f| f.asn1_hint.is_some()));
    if !frame_has_asn1 {
        emit_default_enum_impl(out, &type_name, lt, &frame.variants);
    }

    // Create a resolved frame for parse/serialize
    let resolved_frame = CodecFrame {
        frame_id: frame.frame_id.clone(),
        name: frame.name.clone(),
        tag: frame.tag.clone(),
        variants: resolved_variants,
    };

    // Impl block
    out.push_str(&format!("impl{lt} {type_name}{lt} {{\n"));

    // parse method — returns (Self, tag_type)
    out.push_str(&format!(
        "    pub fn parse(cur: &mut Cursor{}) -> Result<(Self, {tag_type})> {{\n",
        if needs_lt { "<'a>" } else { "<'_>" }
    ));
    parse_emit::emit_frame_parse_body(out, &resolved_frame, "        ");
    out.push_str("    }\n\n");

    // serialize method — tag is passed in
    out.push_str(&format!(
        "    pub fn serialize(&self, tag: {tag_type}, w: &mut Writer<'_>) -> Result<()> {{\n"
    ));
    serialize_emit::emit_frame_serialize_body(out, &resolved_frame, "        ");
    out.push_str("        Ok(())\n");
    out.push_str("    }\n\n");

    // serialized_len method
    let has_asn1 = resolved_frame
        .variants
        .iter()
        .any(|v| v.fields.iter().any(|f| f.asn1_hint.is_some()));
    if has_asn1 {
        out.push_str("    /// Note: for ASN.1 fields, this method encodes the payload to compute the length.\n");
        out.push_str(
            "    /// For best performance, use `serialize()` directly with a pre-allocated buffer.\n",
        );
    }
    out.push_str("    pub fn serialized_len(&self) -> usize {\n");
    out.push_str("        let mut len = 0usize;\n");
    serialize_emit::emit_frame_serialized_len_body(out, &resolved_frame, "        ");
    out.push_str("        len\n");
    out.push_str("    }\n");

    out.push_str("}\n\n");
}

// ── Capsules ──

fn emit_capsule(
    out: &mut String,
    capsule: &CodecCapsule,
    enums: &[SemanticEnum],
    named_types_needing_lifetime: &std::collections::HashSet<String>,
) {
    let type_name = to_pascal_case(&capsule.name);
    let needs_lt = capsule_needs_lifetime(capsule);
    let lt = lifetime_suffix(needs_lt);

    // Resolve enum fields for parse/serialize
    let resolved_header_fields = resolve_enum_fields(&capsule.header_fields, enums);
    let resolved_variants = resolve_enum_variants(&capsule.variants, enums);

    // Struct definition (header fields + payload enum) -- uses original fields
    out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
    out.push_str(&format!("pub struct {type_name}{lt} {{\n"));
    // Header fields
    emit_struct_fields(
        out,
        &capsule.header_fields,
        &capsule.header_items,
        named_types_needing_lifetime,
    );
    // Payload as an enum
    out.push_str(&format!("    pub payload: {type_name}Payload{lt},\n"));
    out.push_str("}\n\n");

    // Payload enum -- uses original fields
    out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
    out.push_str(&format!("pub enum {type_name}Payload{lt} {{\n"));
    for variant in &capsule.variants {
        let variant_name = to_pascal_case(&variant.name);
        if variant.fields.is_empty() {
            out.push_str(&format!("    {variant_name},\n"));
        } else {
            out.push_str(&format!("    {variant_name} {{\n"));
            emit_variant_struct_fields(out, variant, named_types_needing_lifetime);
            out.push_str("    },\n");
        }
    }
    out.push_str("}\n\n");

    // Skip Default impl when any field uses ASN.1 (no sensible default).
    let capsule_has_asn1 = capsule.header_fields.iter().any(|f| f.asn1_hint.is_some())
        || capsule
            .variants
            .iter()
            .any(|v| v.fields.iter().any(|f| f.asn1_hint.is_some()));
    if !capsule_has_asn1 {
        out.push_str(&format!("impl{lt} Default for {type_name}{lt} {{\n"));
        out.push_str("    fn default() -> Self {\n");
        out.push_str("        Self {\n");
        emit_default_struct_items(
            out,
            &capsule.header_fields,
            &capsule.header_items,
            "            ",
        );
        out.push_str("            payload: Default::default(),\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
        out.push_str("}\n\n");

        let payload_type_name = format!("{type_name}Payload");
        emit_default_enum_impl(out, &payload_type_name, lt, &capsule.variants);
    }

    // Create resolved capsule for parse/serialize
    let resolved_capsule = CodecCapsule {
        capsule_id: capsule.capsule_id.clone(),
        name: capsule.name.clone(),
        tag: capsule.tag.clone(),
        within_field: capsule.within_field.clone(),
        tag_expr: capsule.tag_expr.clone(),
        header_fields: resolved_header_fields.clone(),
        header_items: capsule.header_items.clone(),
        header_checksum_plan: capsule.header_checksum_plan.clone(),
        variants: resolved_variants,
    };

    // Impl block
    out.push_str(&format!("impl{lt} {type_name}{lt} {{\n"));

    // parse method
    out.push_str(&format!(
        "    pub fn parse(cur: &mut Cursor{}) -> Result<Self> {{\n",
        if needs_lt { "<'a>" } else { "<'_>" }
    ));
    parse_emit::emit_capsule_parse_body(out, &resolved_capsule, "        ");
    out.push_str("    }\n\n");

    // serialize method
    out.push_str("    pub fn serialize(&self, w: &mut Writer<'_>) -> Result<()> {\n");
    serialize_emit::emit_serialize_items(
        out,
        &resolved_header_fields,
        &capsule.header_items,
        "        ",
        "self.",
    );
    // Serialize payload variants
    serialize_emit::emit_capsule_serialize_body(out, &resolved_capsule, "        ");
    out.push_str("        Ok(())\n");
    out.push_str("    }\n\n");

    // serialized_len method
    let has_asn1 = resolved_header_fields.iter().any(|f| f.asn1_hint.is_some())
        || resolved_capsule
            .variants
            .iter()
            .any(|v| v.fields.iter().any(|f| f.asn1_hint.is_some()));
    if has_asn1 {
        out.push_str("    /// Note: for ASN.1 fields, this method encodes the payload to compute the length.\n");
        out.push_str(
            "    /// For best performance, use `serialize()` directly with a pre-allocated buffer.\n",
        );
    }
    out.push_str("    pub fn serialized_len(&self) -> usize {\n");
    out.push_str("        let mut len = 0usize;\n");
    serialize_emit::emit_serialized_len_items(
        out,
        &resolved_header_fields,
        &capsule.header_items,
        "        ",
        "self.",
    );
    // Payload variant lengths
    serialize_emit::emit_capsule_serialized_len_body(out, &resolved_capsule, "        ");
    out.push_str("        len\n");
    out.push_str("    }\n");

    out.push_str("}\n\n");
}

// ── Shared helpers ──

/// Emit struct fields for packets and capsule headers.
fn emit_struct_fields(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    named_types_needing_lifetime: &std::collections::HashSet<String>,
) {
    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    emit_single_struct_field(out, f, named_types_needing_lifetime);
                }
            }
            CodecItem::Derived(d) => {
                let ty = wire_type_to_rust(&d.wire_type);
                out.push_str(&format!("    pub {}: {ty},\n", rust_ident(&d.name)));
            }
            CodecItem::Require(_) => {}
        }
    }
}

fn emit_single_struct_field(
    out: &mut String,
    f: &CodecField,
    named_types_needing_lifetime: &std::collections::HashSet<String>,
) {
    let field_name = rust_ident(&f.name);
    match f.strategy {
        FieldStrategy::BitGroup => {
            let ty = wire_type_to_rust(&f.wire_type);
            out.push_str(&format!("    pub {field_name}: {ty},\n"));
        }
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            let inner_ty = rust_field_type(inner_wt, named_types_needing_lifetime);
            out.push_str(&format!("    pub {field_name}: Option<{inner_ty}>,\n"));
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                let elem_ty = rust_field_type(&arr.element_wire_type, named_types_needing_lifetime);
                let max_elems = f.max_elements.unwrap_or(MAX_ARRAY_ELEMENTS);
                out.push_str(&format!(
                    "    pub {field_name}: [{elem_ty}; {max_elems}],\n"
                ));
                out.push_str(&format!("    pub {}: usize,\n", rust_count_ident(&f.name)));
            }
        }
        FieldStrategy::BytesFixed
        | FieldStrategy::BytesLength
        | FieldStrategy::BytesRemaining
        | FieldStrategy::BytesLor => {
            if let Some(ref hint) = f.asn1_hint {
                out.push_str(&format!("    pub {field_name}: {},\n", hint.type_name));
            } else {
                out.push_str(&format!("    pub {field_name}: &'a [u8],\n"));
            }
        }
        FieldStrategy::Struct => {
            let ty = rust_field_type(&f.wire_type, named_types_needing_lifetime);
            out.push_str(&format!("    pub {field_name}: {ty},\n"));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            out.push_str(&format!("    pub {field_name}: u64,\n"));
        }
        _ => {
            // Primitive / Checksum
            let ty = wire_type_to_rust(&f.wire_type);
            out.push_str(&format!("    pub {field_name}: {ty},\n"));
        }
    }
}

/// Get the Rust field type string for a wire type.
fn rust_field_type(
    wt: &WireType,
    named_types_needing_lifetime: &std::collections::HashSet<String>,
) -> String {
    match wt {
        WireType::Bytes => "&'a [u8]".into(),
        WireType::Struct(name) | WireType::Frame(name) | WireType::Capsule(name) => {
            let type_name = to_pascal_case(name);
            if named_types_needing_lifetime.contains(name) {
                format!("{type_name}<'a>")
            } else {
                type_name
            }
        }
        WireType::Enum(name) => {
            // Enums are represented as their underlying type in Rust (pub const pattern)
            // We need to look at the enum to know the type; for now use the wire type
            to_pascal_case(name)
        }
        _ => wire_type_to_rust(wt).to_string(),
    }
}

/// Emit the field initializers for Ok(Self { ... }) construction.
fn emit_struct_init_fields(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
) {
    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    out.push_str(&format!("{indent}{},\n", rust_ident(&f.name)));
                    if f.strategy == FieldStrategy::Array {
                        out.push_str(&format!("{indent}{},\n", rust_count_ident(&f.name)));
                    }
                }
            }
            CodecItem::Derived(d) => {
                out.push_str(&format!("{indent}{},\n", rust_ident(&d.name)));
            }
            CodecItem::Require(_) => {}
        }
    }
}

/// Emit struct fields for frame/capsule variant inner fields.
fn emit_variant_struct_fields(
    out: &mut String,
    variant: &CodecVariantScope,
    named_types_needing_lifetime: &std::collections::HashSet<String>,
) {
    for item in &variant.items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = variant.fields.iter().find(|f| &f.field_id == field_id) {
                    emit_variant_single_field(out, f, named_types_needing_lifetime);
                }
            }
            CodecItem::Derived(d) => {
                let ty = wire_type_to_rust(&d.wire_type);
                out.push_str(&format!("        {}: {ty},\n", rust_ident(&d.name)));
            }
            CodecItem::Require(_) => {}
        }
    }
}

fn emit_variant_single_field(
    out: &mut String,
    f: &CodecField,
    named_types_needing_lifetime: &std::collections::HashSet<String>,
) {
    let field_name = rust_ident(&f.name);
    match f.strategy {
        FieldStrategy::Conditional => {
            let inner_wt = f.inner_wire_type.as_ref().unwrap_or(&f.wire_type);
            let inner_ty = rust_field_type(inner_wt, named_types_needing_lifetime);
            out.push_str(&format!("        {field_name}: Option<{inner_ty}>,\n"));
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                let elem_ty = rust_field_type(&arr.element_wire_type, named_types_needing_lifetime);
                let max_elems = f.max_elements.unwrap_or(MAX_ARRAY_ELEMENTS);
                out.push_str(&format!(
                    "        {field_name}: [{elem_ty}; {max_elems}],\n"
                ));
                out.push_str(&format!("        {}: usize,\n", rust_count_ident(&f.name)));
            }
        }
        FieldStrategy::BytesFixed
        | FieldStrategy::BytesLength
        | FieldStrategy::BytesRemaining
        | FieldStrategy::BytesLor => {
            if let Some(ref hint) = f.asn1_hint {
                out.push_str(&format!("        {field_name}: {},\n", hint.type_name));
            } else {
                out.push_str(&format!("        {field_name}: &'a [u8],\n"));
            }
        }
        FieldStrategy::Struct => {
            let ty = rust_field_type(&f.wire_type, named_types_needing_lifetime);
            out.push_str(&format!("        {field_name}: {ty},\n"));
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            out.push_str(&format!("        {field_name}: u64,\n"));
        }
        _ => {
            let ty = wire_type_to_rust(&f.wire_type);
            out.push_str(&format!("        {field_name}: {ty},\n"));
        }
    }
}

fn default_value_expr_for_field(f: &CodecField) -> String {
    if let FieldStrategy::Array = f.strategy {
        return "std::array::from_fn(|_| Default::default())".into();
    }

    if f.asn1_hint.is_some() {
        "panic!(\"Default padding is unavailable for ASN.1-backed fields\")".into()
    } else {
        "Default::default()".into()
    }
}

fn emit_default_struct_items(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    indent: &str,
) {
    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    out.push_str(&format!(
                        "{indent}{}: {},\n",
                        rust_ident(&f.name),
                        default_value_expr_for_field(f)
                    ));
                    if f.strategy == FieldStrategy::Array {
                        out.push_str(&format!(
                            "{indent}{}: Default::default(),\n",
                            rust_count_ident(&f.name)
                        ));
                    }
                }
            }
            CodecItem::Derived(d) => {
                out.push_str(&format!(
                    "{indent}{}: Default::default(),\n",
                    rust_ident(&d.name)
                ));
            }
            CodecItem::Require(_) => {}
        }
    }
}

fn variant_is_unit(variant: &CodecVariantScope) -> bool {
    variant.fields.is_empty()
        && !variant
            .items
            .iter()
            .any(|i| matches!(i, CodecItem::Derived(_)))
}

fn emit_default_enum_impl(
    out: &mut String,
    enum_name: &str,
    lt: &str,
    variants: &[CodecVariantScope],
) {
    if let Some(first_variant) = variants.first() {
        let first_name = to_pascal_case(&first_variant.name);
        out.push_str(&format!("impl{lt} Default for {enum_name}{lt} {{\n"));
        out.push_str("    fn default() -> Self {\n");
        if variant_is_unit(first_variant) {
            out.push_str(&format!("        Self::{first_name}\n"));
        } else {
            out.push_str(&format!("        Self::{first_name} {{\n"));
            emit_default_struct_items(
                out,
                &first_variant.fields,
                &first_variant.items,
                "            ",
            );
            out.push_str("        }\n");
        }
        out.push_str("    }\n");
        out.push_str("}\n\n");
    }
}

// ── VarInts ──

fn emit_varint(out: &mut String, vi: &SemanticVarInt) {
    let snake = to_snake_case(&vi.name);

    match vi.encoding {
        VarIntEncoding::PrefixMatch => emit_varint_prefix_match(out, vi, &snake),
        VarIntEncoding::ContinuationBit => emit_varint_continuation_bit(out, vi, &snake),
    }
}

fn emit_varint_prefix_match(out: &mut String, vi: &SemanticVarInt, snake: &str) {
    let prefix_bits = vi.prefix_bits.unwrap_or(2) as u32;

    // ── parse ──
    out.push_str(&format!(
        "pub fn {snake}_parse(cur: &mut Cursor<'_>) -> Result<u64> {{\n"
    ));
    out.push_str("    let first = cur.read_u8()?;\n");
    out.push_str(&format!("    let prefix = first >> {};\n", 8 - prefix_bits));
    if vi.strict && vi.branches.len() > 1 {
        // @strict: capture val from match so we can validate after
        out.push_str("    let val = match prefix {\n");
    } else {
        out.push_str("    match prefix {\n");
    }

    for branch in &vi.branches {
        let pv = branch.prefix_value;
        let total_bytes = branch.total_bytes as u32;
        let value_bits_first = 8 - prefix_bits as u8;
        let value_mask_first: u8 = (1u16 << value_bits_first) as u8 - 1;

        out.push_str(&format!("        {pv} => {{\n"));

        if total_bytes == 1 {
            out.push_str(&format!(
                "            let val = (first & 0x{value_mask_first:02x}) as u64;\n"
            ));
        } else {
            out.push_str(&format!(
                "            let mut val = ((first & 0x{value_mask_first:02x}) as u64) << {};\n",
                (total_bytes - 1) * 8
            ));
            for i in 1..total_bytes {
                out.push_str(&format!(
                    "            val |= (cur.read_u8()? as u64) << {};\n",
                    (total_bytes - 1 - i) * 8
                ));
            }
        }

        if vi.strict && vi.branches.len() > 1 {
            out.push_str("            val\n");
        } else {
            out.push_str("            Ok(val)\n");
        }
        out.push_str("        }\n");
    }

    if vi.strict && vi.branches.len() > 1 {
        out.push_str("        _ => { return Err(Error::InvalidTag); }\n");
        out.push_str("    };\n");
        // @strict: reject non-canonical encodings
        out.push_str("    // @strict: reject non-canonical encodings\n");
        for i in 1..vi.branches.len() {
            let prev_max = vi.branches[i - 1].max_value;
            let prefix_val = vi.branches[i].prefix_value;
            out.push_str(&format!(
                "    if prefix == {} && val <= {} {{ return Err(Error::Noncanonical); }}\n",
                prefix_val, prev_max
            ));
        }
        out.push_str("    Ok(val)\n");
    } else {
        out.push_str("        _ => Err(Error::InvalidTag),\n");
        out.push_str("    }\n");
    }

    out.push_str("}\n\n");

    // ── serialize ──
    out.push_str(&format!(
        "pub fn {snake}_serialize(val: u64, w: &mut Writer<'_>) -> Result<()> {{\n"
    ));

    for (i, branch) in vi.branches.iter().enumerate() {
        let max_val = branch.max_value;
        let total_bytes = branch.total_bytes as u32;
        let prefix_byte = (branch.prefix_value as u8) << (8 - prefix_bits);

        let cond = if i == 0 {
            format!("    if val <= {max_val}")
        } else {
            format!("    }} else if val <= {max_val}")
        };

        out.push_str(&format!("{cond} {{\n"));

        if total_bytes == 1 {
            out.push_str(&format!(
                "        w.write_u8(0x{prefix_byte:02x} | (val as u8))?;\n"
            ));
        } else {
            out.push_str(&format!(
                "        w.write_u8(0x{prefix_byte:02x} | ((val >> {}) as u8))?;\n",
                (total_bytes - 1) * 8
            ));
            for j in 1..total_bytes {
                let shift = (total_bytes - 1 - j) * 8;
                if shift == 0 {
                    out.push_str("        w.write_u8(val as u8)?;\n");
                } else {
                    out.push_str(&format!("        w.write_u8((val >> {shift}) as u8)?;\n"));
                }
            }
        }
    }

    if !vi.branches.is_empty() {
        out.push_str("    } else {\n");
        out.push_str("        return Err(Error::Overflow);\n");
        out.push_str("    }\n");
    }

    out.push_str("    Ok(())\n");
    out.push_str("}\n\n");

    // ── wire_size ──
    out.push_str(&format!("pub fn {snake}_wire_size(val: u64) -> usize {{\n"));

    for (i, branch) in vi.branches.iter().enumerate() {
        let max_val = branch.max_value;
        let total_bytes = branch.total_bytes;

        if i == 0 {
            out.push_str(&format!("    if val <= {max_val} {{ {total_bytes} }}\n"));
        } else {
            out.push_str(&format!(
                "    else if val <= {max_val} {{ {total_bytes} }}\n"
            ));
        }
    }

    out.push_str(&format!("    else {{ {} }}\n", vi.max_bytes));
    out.push_str("}\n\n");
}

fn emit_varint_continuation_bit(out: &mut String, vi: &SemanticVarInt, snake: &str) {
    let value_bits = vi.value_bits_per_byte.unwrap_or(7) as u32;
    let max_bytes = vi.max_bytes;
    let value_mask: u8 = (1u16 << value_bits) as u8 - 1;
    let cont_mask: u8 = !value_mask;

    // ── parse ──
    out.push_str(&format!(
        "pub fn {snake}_parse(cur: &mut Cursor<'_>) -> Result<u64> {{\n"
    ));
    out.push_str("    let mut value: u64 = 0;\n");
    out.push_str("    let mut shift: u32 = 0;\n");
    out.push_str(&format!("    for _ in 0..{max_bytes} {{\n"));
    out.push_str("        let byte = cur.read_u8()?;\n");
    out.push_str(&format!(
        "        let chunk = (byte & 0x{value_mask:02x}) as u64;\n"
    ));
    out.push_str("        value |= chunk << shift;\n");
    out.push_str(&format!(
        "        if (byte & 0x{cont_mask:02x}) == 0 {{ return Ok(value); }}\n"
    ));
    out.push_str(&format!("        shift += {value_bits};\n"));
    out.push_str("    }\n");
    out.push_str("    Err(Error::Overflow)\n");
    out.push_str("}\n\n");

    // ── serialize ──
    out.push_str(&format!(
        "pub fn {snake}_serialize(mut val: u64, w: &mut Writer<'_>) -> Result<()> {{\n"
    ));
    out.push_str(&format!("    for _ in 0..{max_bytes} {{\n"));
    out.push_str(&format!(
        "        let mut byte = (val & 0x{value_mask:02x}) as u8;\n"
    ));
    out.push_str(&format!("        val >>= {value_bits};\n"));
    out.push_str(&format!(
        "        if val != 0 {{ byte |= 0x{cont_mask:02x}; }}\n"
    ));
    out.push_str("        w.write_u8(byte)?;\n");
    out.push_str("        if val == 0 { return Ok(()); }\n");
    out.push_str("    }\n");
    out.push_str("    Err(Error::Overflow)\n");
    out.push_str("}\n\n");

    // ── wire_size ──
    out.push_str(&format!("pub fn {snake}_wire_size(val: u64) -> usize {{\n"));
    out.push_str("    let mut v = val;\n");
    out.push_str("    let mut n = 1usize;\n");
    out.push_str(&format!(
        "    while v >= (1u64 << {value_bits}) && n < {max_bytes} {{\n"
    ));
    out.push_str(&format!("        v >>= {value_bits};\n"));
    out.push_str("        n += 1;\n");
    out.push_str("    }\n");
    out.push_str("    n\n");
    out.push_str("}\n\n");
}

// ── State Machines ──

fn emit_state_machine(
    out: &mut String,
    sm: &SemanticStateMachine,
    all_sms: &[SemanticStateMachine],
) {
    let sm_name = to_pascal_case(&sm.name);
    let all_states: Vec<_> = all_sms
        .iter()
        .flat_map(|state_machine| state_machine.states.iter().cloned())
        .collect();

    // ── State enum ──
    out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
    out.push_str(&format!("pub enum {sm_name} {{\n"));
    for state in &sm.states {
        let state_name = to_pascal_case(&state.name);
        if state.fields.is_empty() {
            out.push_str(&format!("    {state_name},\n"));
        } else {
            out.push_str(&format!("    {state_name} {{\n"));
            for field in &state.fields {
                let ty = sm_field_type_to_rust(field);
                out.push_str(&format!("        {}: {ty},\n", rust_ident(&field.name)));
            }
            out.push_str("    },\n");
        }
    }
    out.push_str("}\n\n");

    // ── Event enum ──
    let has_events = !sm.events.is_empty();
    if has_events {
        out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
        out.push_str(&format!("pub enum {sm_name}Event {{\n"));
        for event in &sm.events {
            let event_name = to_pascal_case(&event.name);
            if event.params.is_empty() {
                out.push_str(&format!("    {event_name},\n"));
            } else {
                out.push_str(&format!("    {event_name} {{\n"));
                for param in &event.params {
                    let ty = semantic_type_to_rust(&param.ty);
                    out.push_str(&format!("        {}: {ty},\n", rust_ident(&param.name)));
                }
                out.push_str("    },\n");
            }
        }
        out.push_str("}\n\n");
    }

    // ── impl block ──
    out.push_str(&format!("impl {sm_name} {{\n"));

    // ── new() constructor ──
    // Find the initial state
    if let Some(initial_state) = sm.states.iter().find(|s| s.state_id == sm.initial_state_id) {
        let initial_name = to_pascal_case(&initial_state.name);
        out.push_str("    pub fn new() -> Self {\n");
        if initial_state.fields.is_empty() {
            out.push_str(&format!("        Self::{initial_name}\n"));
        } else {
            out.push_str(&format!("        Self::{initial_name} {{\n"));
            for field in &initial_state.fields {
                let default_val = if let Some(ref lit) = field.default_value {
                    semantic_literal_to_rust(lit)
                } else if let Some(ref child_sm) = field.child_sm_name {
                    // Child SM field: use the child's constructor
                    let child_pascal = to_pascal_case(child_sm);
                    match &field.ty {
                        wirespec_sema::types::SemanticType::Array { .. } => {
                            "Vec::new()".to_string()
                        }
                        _ => format!("{child_pascal}::new()"),
                    }
                } else {
                    // Use Default::default() for the type
                    "0".into()
                };
                out.push_str(&format!(
                    "            {}: {default_val},\n",
                    rust_ident(&field.name)
                ));
            }
            out.push_str("        }\n");
        }
        out.push_str("    }\n\n");
    }

    // ── dispatch() ──
    if has_events && !sm.transitions.is_empty() {
        out.push_str(&format!(
            "    pub fn dispatch(&mut self, event: &{sm_name}Event) -> Result<()> {{\n"
        ));
        out.push_str("        match (&*self, event) {\n");

        for trans in &sm.transitions {
            let src_state_name = to_pascal_case(&trans.src_state_name);
            let dst_state_name = to_pascal_case(&trans.dst_state_name);
            let event_name = to_pascal_case(&trans.event_name);

            // Find source and destination state definitions
            let src_state = sm.states.iter().find(|s| s.state_id == trans.src_state_id);
            let dst_state = sm.states.iter().find(|s| s.state_id == trans.dst_state_id);
            let event_def = sm.events.iter().find(|e| e.event_id == trans.event_id);

            // Build the match pattern for source state
            let src_pattern = if let Some(src) = src_state {
                if src.fields.is_empty() {
                    format!("Self::{src_state_name}")
                } else {
                    let field_bindings: Vec<String> =
                        src.fields.iter().map(|f| rust_ident(&f.name)).collect();
                    format!("Self::{src_state_name} {{ {} }}", field_bindings.join(", "))
                }
            } else {
                format!("Self::{src_state_name}")
            };

            // Build the match pattern for event
            let event_pattern = if let Some(ev) = event_def {
                if ev.params.is_empty() {
                    format!("{sm_name}Event::{event_name}")
                } else {
                    let param_bindings: Vec<String> =
                        ev.params.iter().map(|p| rust_ident(&p.name)).collect();
                    format!(
                        "{sm_name}Event::{event_name} {{ {} }}",
                        param_bindings.join(", ")
                    )
                }
            } else {
                format!("{sm_name}Event::{event_name}")
            };

            out.push_str(&format!(
                "            ({src_pattern}, {event_pattern}) => {{\n"
            ));

            // Guard check
            if let Some(ref guard) = trans.guard {
                let guard_str = sm_expr_to_rust_ctx(guard, &all_states);
                out.push_str(&format!(
                    "                if !({guard_str}) {{ return Err(Error::InvalidState); }}\n"
                ));
            }

            if let Some(ref delegate) = trans.delegate {
                // Delegate: auto-copy src -> dst, then forward event to child SM
                // Try to extract the child field name from the delegate target
                let child_field = extract_delegate_child_field(&delegate.target);
                let child_sm_name = child_field.as_ref().and_then(|fname| {
                    src_state.and_then(|s| {
                        s.fields
                            .iter()
                            .find(|f| f.name == *fname)
                            .and_then(|f| f.child_sm_name.clone())
                    })
                });

                out.push_str("                // delegate: auto-copy + forward to child SM\n");
                out.push_str("                let mut new_state = self.clone();\n");

                if let (Some(field_name), Some(child_sm_str)) = (&child_field, &child_sm_name) {
                    // The delegate's event_name is the event parameter to forward
                    let delegate_event_param = rust_ident(&delegate.event_name);
                    // Check if the delegate target is an indexed access (e.g., src.paths[idx])
                    let is_indexed = matches!(delegate.target, SemanticExpr::Subscript { .. });

                    // Look up the child SM to generate ordinal-to-event mapping
                    let child_sm_def = all_sms.iter().find(|s| s.name == *child_sm_str);
                    let child_pascal = to_pascal_case(child_sm_str);
                    let delegate_param_def = event_def
                        .and_then(|ev| ev.params.iter().find(|p| p.name == *delegate_event_param));
                    let delegate_param_is_integer_like = delegate_param_def
                        .map(|p| p.ty.is_integer_like())
                        .unwrap_or(false);
                    let delegate_event_param_names: std::collections::HashSet<&str> = event_def
                        .map(|ev| ev.params.iter().map(|p| p.name.as_str()).collect())
                        .unwrap_or_default();
                    let child_field_name = rust_ident(field_name);

                    // Generate the event conversion match from u8 ordinal to child event enum
                    let event_conversion = if delegate_param_is_integer_like {
                        if let Some(child_def) = child_sm_def {
                            if child_def.events.iter().all(|ev| ev.params.is_empty()) {
                                let mut match_arms = String::new();
                                for (ordinal, ev) in child_def.events.iter().enumerate() {
                                    let ev_pascal = to_pascal_case(&ev.name);
                                    match_arms.push_str(&format!(
                                        "                        {ordinal} => {child_pascal}Event::{ev_pascal},\n"
                                    ));
                                }
                                Some(match_arms)
                            } else {
                                Some(String::new())
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let delegate_requires_runtime_error = delegate_param_is_integer_like
                        && event_conversion
                            .as_ref()
                            .is_some_and(|arms| arms.is_empty());

                    if is_indexed {
                        // Indexed delegate: src.field[index] <- ev
                        // Extract the index expression
                        let index_expr =
                            if let SemanticExpr::Subscript { index, .. } = &delegate.target {
                                match index.as_ref() {
                                    SemanticExpr::ValueRef { reference }
                                        if delegate_event_param_names
                                            .contains(reference.value_id.as_str()) =>
                                    {
                                        format!("{}.clone()", rust_ident(&reference.value_id))
                                    }
                                    _ => sm_expr_to_rust_ctx(index, &all_states),
                                }
                            } else {
                                "0".to_string()
                            };
                        out.push_str(&format!(
                            "                if let Self::{src_state_name} {{ ref mut {child_field_name}, .. }} = new_state {{\n"
                        ));
                        out.push_str(&format!(
                            "                    let _idx = ({index_expr}) as usize;\n"
                        ));
                        out.push_str(&format!(
                            "                    let _old = std::mem::discriminant({child_field_name}.get(_idx).ok_or(Error::InvalidState)?);\n"
                        ));
                        if delegate_requires_runtime_error {
                            out.push_str("                    return Err(Error::InvalidState);\n");
                        } else if let Some(ref arms) = event_conversion {
                            out.push_str(&format!(
                                "                    let _child_event = match {delegate_event_param}.clone() {{\n"
                            ));
                            out.push_str(arms);
                            out.push_str(
                                "                        _ => return Err(Error::InvalidState),\n",
                            );
                            out.push_str("                    };\n");
                            out.push_str(&format!(
                                "                    {child_field_name}.get_mut(_idx).ok_or(Error::InvalidState)?.dispatch(&_child_event)?;\n"
                            ));
                        } else {
                            out.push_str(&format!(
                                "                    {child_field_name}.get_mut(_idx).ok_or(Error::InvalidState)?.dispatch({delegate_event_param})?;\n"
                            ));
                        }
                    } else {
                        // Simple delegate: src.field <- ev
                        out.push_str(&format!(
                            "                if let Self::{src_state_name} {{ ref mut {child_field_name}, .. }} = new_state {{\n"
                        ));
                        out.push_str(&format!(
                            "                    let _old = std::mem::discriminant(&*{child_field_name});\n"
                        ));
                        if delegate_requires_runtime_error {
                            out.push_str("                    return Err(Error::InvalidState);\n");
                        } else if let Some(ref arms) = event_conversion {
                            out.push_str(&format!(
                                "                    let _child_event = match {delegate_event_param}.clone() {{\n"
                            ));
                            out.push_str(arms);
                            out.push_str(
                                "                        _ => return Err(Error::InvalidState),\n",
                            );
                            out.push_str("                    };\n");
                            out.push_str(&format!(
                                "                    {child_field_name}.dispatch(&_child_event)?;\n"
                            ));
                        } else {
                            out.push_str(&format!(
                                "                    {child_field_name}.dispatch({delegate_event_param})?;\n"
                            ));
                        }
                    }
                    if sm.has_child_state_changed {
                        let discriminant_ref = if is_indexed {
                            format!("&{child_field_name}[_idx]")
                        } else {
                            format!("&*{child_field_name}")
                        };
                        out.push_str(&format!(
                            "                    if std::mem::discriminant({discriminant_ref}) != _old {{\n"
                        ));
                        out.push_str(&format!(
                            "                        let _ = new_state.dispatch(&{sm_name}Event::ChildStateChanged);\n"
                        ));
                        out.push_str("                    }\n");
                    }
                    out.push_str("                }\n");
                } else {
                    // Fallback: we couldn't resolve the child field
                    let delegate_event_param = &delegate.event_name;
                    out.push_str(&format!(
                        "                // delegate: could not resolve child field for {delegate_event_param}\n"
                    ));
                }

                out.push_str("                *self = new_state;\n");
                out.push_str("                Ok(())\n");
            } else {
                // Build destination state
                if let Some(dst) = dst_state {
                    if dst.fields.is_empty() {
                        out.push_str(&format!(
                            "                let mut new_state = Self::{dst_state_name};\n"
                        ));
                    } else {
                        out.push_str(&format!(
                            "                let mut new_state = Self::{dst_state_name} {{\n"
                        ));

                        // For each destination field, check if there's an action that assigns it
                        for dst_field in &dst.fields {
                            let dst_field_name = rust_ident(&dst_field.name);
                            let assigned_value =
                                find_action_for_field(&trans.actions, &dst_field.name, &all_states);
                            if let Some(expr_str) = assigned_value {
                                out.push_str(&format!(
                                    "                    {dst_field_name}: {expr_str},\n"
                                ));
                            } else {
                                // Try to copy from source state if same field name exists
                                let src_has_field = src_state
                                    .map(|s| s.fields.iter().any(|f| f.name == dst_field.name))
                                    .unwrap_or(false);
                                if src_has_field {
                                    let copy_expr = if is_sm_field_copy_type(dst_field) {
                                        format!("*{}", dst_field_name)
                                    } else {
                                        format!("{}.clone()", dst_field_name)
                                    };
                                    out.push_str(&format!(
                                        "                    {dst_field_name}: {copy_expr},\n"
                                    ));
                                } else if let Some(ref lit) = dst_field.default_value {
                                    let val = semantic_literal_to_rust(lit);
                                    out.push_str(&format!(
                                        "                    {dst_field_name}: {val},\n"
                                    ));
                                } else {
                                    out.push_str(&format!(
                                        "                    {dst_field_name}: 0,\n"
                                    ));
                                }
                            }
                        }

                        out.push_str("                };\n");
                    }
                } else {
                    out.push_str(&format!(
                        "                let mut new_state = Self::{dst_state_name};\n"
                    ));
                }

                emit_post_construction_actions(
                    out,
                    &trans.actions,
                    dst_state_name.as_str(),
                    &all_states,
                );
                out.push_str("                *self = new_state;\n");
                out.push_str("                Ok(())\n");
            }
            out.push_str("            }\n");
        }

        out.push_str("            _ => Err(Error::InvalidState),\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
    }

    out.push_str("}\n\n");
}

/// Convert a SemanticExpr from the state machine context to Rust code.
/// In SM context, TransitionPeerRef is used for src/dst/event-param fields.
#[allow(dead_code)]
fn sm_expr_to_rust(expr: &SemanticExpr) -> String {
    sm_expr_to_rust_ctx(expr, &[])
}

/// Convert a SemanticExpr with access to SM state definitions for InState/All lookups.
fn sm_expr_to_rust_ctx(expr: &SemanticExpr, sm_states: &[SemanticState]) -> String {
    sm_expr_to_rust_mode(expr, sm_states, SmExprMode::Value)
}

#[derive(Clone, Copy)]
enum SmExprMode {
    Value,
    Borrow,
}

fn sm_expr_to_rust_mode(
    expr: &SemanticExpr,
    sm_states: &[SemanticState],
    mode: SmExprMode,
) -> String {
    match expr {
        SemanticExpr::Literal { value } => match value {
            SemanticLiteral::Int(n) => {
                if *n < 0 {
                    format!("({n})")
                } else {
                    format!("{n}")
                }
            }
            SemanticLiteral::Bool(b) => format!("{b}"),
            SemanticLiteral::String(s) => format!("{s:?}"),
            SemanticLiteral::Null => "0".into(),
        },
        SemanticExpr::TransitionPeerRef { reference } => {
            // src.field -> *field (from match binding)
            // dst.field -> field (target assignment)
            // event_param -> *param_name (from match binding)
            if reference.path.is_empty() {
                "()".into()
            } else {
                let field_name = rust_ident(&reference.path[reference.path.len() - 1]);
                match reference.peer {
                    TransitionPeerKind::Src => match mode {
                        SmExprMode::Value => format!("{field_name}.clone()"),
                        SmExprMode::Borrow => field_name,
                    },
                    TransitionPeerKind::Dst => field_name,
                    TransitionPeerKind::EventParam => match mode {
                        SmExprMode::Value => format!("{field_name}.clone()"),
                        SmExprMode::Borrow => field_name,
                    },
                }
            }
        }
        SemanticExpr::ValueRef { reference } => rust_ident(&reference.value_id),
        SemanticExpr::Binary { op, left, right } => {
            let l = sm_expr_to_rust_mode(left, sm_states, SmExprMode::Value);
            let r = sm_expr_to_rust_mode(right, sm_states, SmExprMode::Value);
            let rust_op = match op.as_str() {
                "and" => "&&",
                "or" => "||",
                o => o,
            };
            format!("({l} {rust_op} {r})")
        }
        SemanticExpr::Unary { op, operand } => {
            let o = sm_expr_to_rust_mode(operand, sm_states, SmExprMode::Value);
            format!("({op}{o})")
        }
        SemanticExpr::Subscript { base, index } => {
            let b = sm_expr_to_rust_mode(base, sm_states, SmExprMode::Borrow);
            let i = sm_expr_to_rust_mode(index, sm_states, SmExprMode::Value);
            let idx_expr = format!("({i}) as usize");
            let get_expr = format!("{b}.get({idx_expr}).ok_or(Error::InvalidState)?");
            match mode {
                SmExprMode::Value => format!("({get_expr}).clone()"),
                SmExprMode::Borrow => get_expr,
            }
        }
        SemanticExpr::Coalesce { expr, default } => {
            let e = sm_expr_to_rust_mode(expr, sm_states, SmExprMode::Value);
            let d = sm_expr_to_rust_mode(default, sm_states, SmExprMode::Value);
            // Rust doesn't have ??, use unwrap_or pattern
            format!("{e}.unwrap_or({d})")
        }
        SemanticExpr::InState {
            expr,
            state_id,
            sm_name,
            state_name,
            ..
        } => {
            let expr_rs = sm_expr_to_rust_mode(expr, sm_states, SmExprMode::Borrow);
            let sm_pascal = to_pascal_case(sm_name);
            let state_pascal = to_pascal_case(state_name);
            // Look up whether this state has fields to emit correct match pattern.
            let has_fields = find_state_definition(sm_states, sm_name, state_id, state_name)
                .map(|s| !s.fields.is_empty())
                .unwrap_or(false); // default to false (unit variant) if not found
            if has_fields {
                format!("matches!({expr_rs}, {sm_pascal}::{state_pascal} {{ .. }})")
            } else {
                format!("matches!({expr_rs}, {sm_pascal}::{state_pascal})")
            }
        }
        SemanticExpr::StateConstructor {
            sm_name,
            state_id,
            state_name,
            args,
            ..
        } => {
            let sm_pascal = to_pascal_case(sm_name);
            let state_pascal = to_pascal_case(state_name);
            if args.is_empty() {
                format!("{sm_pascal}::{state_pascal}")
            } else {
                let field_names: Option<Vec<String>> =
                    find_state_definition(sm_states, sm_name, state_id, state_name)
                        .map(|s| s.fields.iter().map(|f| rust_ident(&f.name)).collect());

                let arg_strs: Vec<String> = args
                    .iter()
                    .map(|a| sm_expr_to_rust_mode(a, sm_states, SmExprMode::Value))
                    .collect();

                if let Some(names) = field_names {
                    // Pair field names with arg values
                    let fields: Vec<String> = names
                        .iter()
                        .zip(arg_strs.iter())
                        .map(|(name, val)| format!("{name}: {val}"))
                        .collect();
                    format!("{sm_pascal}::{state_pascal} {{ {} }}", fields.join(", "))
                } else {
                    // Fallback: emit positional args as comment
                    format!(
                        "{sm_pascal}::{state_pascal} {{ /* args: {} */ }}",
                        arg_strs.join(", ")
                    )
                }
            }
        }
        SemanticExpr::Fill { value, count } => {
            let val_rs = sm_expr_to_rust_mode(value, sm_states, SmExprMode::Value);
            let cnt_rs = sm_expr_to_rust_mode(count, sm_states, SmExprMode::Value);
            // Use vec![] for SM array fields (Vec<T>), since T may not be Copy.
            format!("vec![{val_rs}; {cnt_rs}]")
        }
        SemanticExpr::Slice { base, start, end } => {
            let b = sm_expr_to_rust_mode(base, sm_states, SmExprMode::Borrow);
            let s = sm_expr_to_rust_mode(start, sm_states, SmExprMode::Value);
            let e = sm_expr_to_rust_mode(end, sm_states, SmExprMode::Value);
            let slice =
                format!("{b}.get(({s}) as usize..({e}) as usize).ok_or(Error::InvalidState)?");
            match mode {
                SmExprMode::Value => format!("({slice}).to_vec()"),
                SmExprMode::Borrow => format!("&({slice})"),
            }
        }
        SemanticExpr::All {
            collection,
            state_id,
            sm_name,
            state_name,
            ..
        } => {
            let sm_pascal = to_pascal_case(sm_name);
            let state_pascal = to_pascal_case(state_name);
            let has_fields = find_state_definition(sm_states, sm_name, state_id, state_name)
                .map(|s| !s.fields.is_empty())
                .unwrap_or(false);
            let pattern = if has_fields {
                format!("{sm_pascal}::{state_pascal} {{ .. }}")
            } else {
                format!("{sm_pascal}::{state_pascal}")
            };
            let coll_rs = sm_expr_to_rust_mode(collection, sm_states, SmExprMode::Borrow);
            format!("({coll_rs}).iter().all(|_ai| matches!(_ai, {pattern}))")
        }
    }
}

/// Find an action that assigns to the given field name in the destination state.
/// Returns the Rust expression string for the assigned value.
fn find_action_for_field(
    actions: &[SemanticAction],
    field_name: &str,
    sm_states: &[SemanticState],
) -> Option<String> {
    for action in actions {
        // Check if the target is the destination field with matching name
        if let SemanticExpr::TransitionPeerRef { reference } = &action.target
            && reference.peer == TransitionPeerKind::Dst
            && !reference.path.is_empty()
            && reference.path[reference.path.len() - 1] == field_name
        {
            let value_str = sm_expr_to_rust_ctx(&action.value, sm_states);
            let field_ident = rust_ident(field_name);
            return match action.op.as_str() {
                "=" => Some(value_str),
                "+=" => Some(format!("(*{field_ident} + {value_str})")),
                "-=" => Some(format!("(*{field_ident} - {value_str})")),
                "*=" => Some(format!("(*{field_ident} * {value_str})")),
                "/=" => Some(format!("(*{field_ident} / {value_str})")),
                _ => Some(value_str),
            };
        }
    }
    None
}

fn emit_post_construction_actions(
    out: &mut String,
    actions: &[SemanticAction],
    dst_state_name: &str,
    sm_states: &[SemanticState],
) {
    for action in actions {
        let SemanticExpr::Subscript { base, index } = &action.target else {
            continue;
        };
        let SemanticExpr::TransitionPeerRef { reference } = base.as_ref() else {
            continue;
        };
        if reference.peer != TransitionPeerKind::Dst || reference.path.is_empty() {
            continue;
        }

        let field_name = rust_ident(&reference.path[reference.path.len() - 1]);
        let index_expr = sm_expr_to_rust_ctx(index, sm_states);
        let value_expr = sm_expr_to_rust_ctx(&action.value, sm_states);
        let assigned_expr = match action.op.as_str() {
            "=" => value_expr,
            "+=" => {
                format!("{field_name}.get(_idx).ok_or(Error::InvalidState)?.clone() + {value_expr}")
            }
            "-=" => {
                format!("{field_name}.get(_idx).ok_or(Error::InvalidState)?.clone() - {value_expr}")
            }
            "*=" => {
                format!("{field_name}.get(_idx).ok_or(Error::InvalidState)?.clone() * {value_expr}")
            }
            "/=" => {
                format!("{field_name}.get(_idx).ok_or(Error::InvalidState)?.clone() / {value_expr}")
            }
            _ => value_expr,
        };

        out.push_str(&format!(
            "                if let Self::{dst_state_name} {{ ref mut {field_name}, .. }} = new_state {{\n"
        ));
        out.push_str(&format!(
            "                    let _idx = ({index_expr}) as usize;\n"
        ));
        out.push_str(&format!(
            "                    *{field_name}.get_mut(_idx).ok_or(Error::InvalidState)? = {assigned_expr};\n"
        ));
        out.push_str("                }\n");
    }
}

fn find_state_definition<'a>(
    sm_states: &'a [SemanticState],
    sm_name: &str,
    state_id: &str,
    state_name: &str,
) -> Option<&'a SemanticState> {
    let expected_state_id = format!("sm:{sm_name}/state:{state_name}");
    sm_states
        .iter()
        .find(|s| !state_id.is_empty() && s.state_id == state_id)
        .or_else(|| sm_states.iter().find(|s| s.state_id == expected_state_id))
        .or_else(|| sm_states.iter().find(|s| s.name == state_name))
}

/// Check if a state machine field has a Copy type (primitive/bits).
/// Non-Copy types (Vec, child SM enums) need `.clone()` instead of `*field`.
fn is_sm_field_copy_type(field: &SemanticStateField) -> bool {
    use wirespec_sema::types::SemanticType;
    // If it's a child SM field or an array, it's not Copy
    if field.child_sm_name.is_some() {
        return false;
    }
    matches!(
        &field.ty,
        SemanticType::Primitive { .. } | SemanticType::Bits { .. }
    )
}

/// Extract the child field name from a delegate target expression.
/// Delegate target is typically `TransitionPeerRef { peer: Src, path: ["child_field"] }`
/// or a `Subscript` over such a ref (e.g., `src.paths[id]`).
fn extract_delegate_child_field(target: &SemanticExpr) -> Option<String> {
    match target {
        SemanticExpr::TransitionPeerRef { reference } => {
            if reference.peer == TransitionPeerKind::Src && !reference.path.is_empty() {
                Some(reference.path[0].clone())
            } else {
                None
            }
        }
        SemanticExpr::Subscript { base, .. } => extract_delegate_child_field(base),
        _ => None,
    }
}
