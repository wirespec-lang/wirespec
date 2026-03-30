// crates/wirespec-backend-c/src/source.rs
//
// .c source file assembly: includes, parse, serialize, serialized_len implementations.

use wirespec_codec::ir::*;
use wirespec_sema::expr::{SemanticExpr, TransitionPeerKind};
use wirespec_sema::ir::{
    SemanticAction, SemanticDelegate, SemanticEnum, SemanticStateMachine, SemanticVarInt,
    VarIntEncoding,
};
use wirespec_sema::types::PrimitiveWireType;

use crate::expr::SmExprContext;
use crate::names::*;
use crate::parse_emit;
use crate::serialize_emit;

/// Resolve enum fields: change strategy from Struct to Primitive and set
/// wire_type to the enum's underlying primitive type. This allows the
/// standard primitive read/write codegen path to handle enum fields.
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
        _ => WireType::U8, // fallback for unexpected types
    }
}

/// Emit the complete .c source file content.
pub fn emit_source(module: &CodecModule, prefix: &str) -> String {
    let mut out = String::new();

    // Set the const prefix for expression generation
    crate::expr::set_const_prefix(prefix);

    // Include the generated header
    out.push_str(&format!("#include \"{prefix}.h\"\n\n"));

    // Forward declarations for static parse_cursor functions (needed when
    // type A references type B's parse_cursor but B is defined later in the file).
    for packet in &module.packets {
        let tname = c_type_name(prefix, &packet.name);
        let cursor_fn = c_func_name(prefix, &packet.name, "parse_cursor");
        let has_cksum = packet.checksum_plan.is_some()
            && packet
                .checksum_plan
                .as_ref()
                .is_some_and(|p| p.input_model == ChecksumInputModel::RecomputeWithSkippedField);
        if has_cksum {
            out.push_str(&format!(
                "static wirespec_result_t {cursor_fn}(wirespec_cursor_t *cur, {tname} *out, size_t *_cksum_offset_out);\n"
            ));
        } else {
            out.push_str(&format!(
                "static wirespec_result_t {cursor_fn}(wirespec_cursor_t *cur, {tname} *out);\n"
            ));
        }
    }
    for frame in &module.frames {
        let tname = c_type_name(prefix, &frame.name);
        let cursor_fn = c_func_name(prefix, &frame.name, "parse_cursor");
        out.push_str(&format!(
            "static wirespec_result_t {cursor_fn}(wirespec_cursor_t *cur, {tname} *out);\n"
        ));
    }
    for capsule in &module.capsules {
        let tname = c_type_name(prefix, &capsule.name);
        let cursor_fn = c_func_name(prefix, &capsule.name, "parse_cursor");
        out.push_str(&format!(
            "static wirespec_result_t {cursor_fn}(wirespec_cursor_t *cur, {tname} *out);\n"
        ));
    }
    out.push('\n');

    // VarInt functions
    for vi in &module.varints {
        emit_varint_source(&mut out, vi, prefix);
    }

    // Packets
    for packet in &module.packets {
        emit_packet_parse(&mut out, packet, prefix, &module.enums);
        emit_packet_serialize(&mut out, packet, prefix, &module.enums);
        emit_packet_serialized_len(&mut out, packet, prefix, &module.enums);
    }

    // Frames (resolve enum fields in variants)
    for frame in &module.frames {
        let frame = resolve_frame(frame, &module.enums);
        emit_frame_parse(&mut out, &frame, prefix, &module.enums);
        emit_frame_serialize(&mut out, &frame, prefix, &module.enums);
        emit_frame_serialized_len(&mut out, &frame, prefix, &module.enums);
    }

    // Capsules (resolve enum fields in header + variants)
    for capsule in &module.capsules {
        let capsule = resolve_capsule(capsule, &module.enums);
        emit_capsule_parse(&mut out, &capsule, prefix, &module.enums);
        emit_capsule_serialize(&mut out, &capsule, prefix, &module.enums);
        emit_capsule_serialized_len(&mut out, &capsule, prefix, &module.enums);
    }

    // State machines
    for sm in &module.state_machines {
        emit_sm_dispatch(&mut out, sm, prefix);
    }

    out
}

// ── Packet ──

fn emit_packet_parse(out: &mut String, packet: &CodecPacket, prefix: &str, enums: &[SemanticEnum]) {
    let tname = c_type_name(prefix, &packet.name);
    let cursor_fn = c_func_name(prefix, &packet.name, "parse_cursor");
    let public_fn = c_func_name(prefix, &packet.name, "parse");
    let resolved_fields = resolve_enum_fields(&packet.fields, enums);

    let has_cksum = packet.checksum_plan.is_some();
    let needs_offset = has_cksum
        && packet
            .checksum_plan
            .as_ref()
            .is_some_and(|p| p.input_model == ChecksumInputModel::RecomputeWithSkippedField);

    // Static cursor-based parse
    if needs_offset {
        out.push_str(&format!(
            "static wirespec_result_t {cursor_fn}(wirespec_cursor_t *cur, {tname} *out, size_t *_cksum_offset_out) {{\n"
        ));
        out.push_str("    wirespec_result_t r;\n");
        out.push_str("    size_t _cksum_offset = 0;\n");
    } else {
        out.push_str(&format!(
            "static wirespec_result_t {cursor_fn}(wirespec_cursor_t *cur, {tname} *out) {{\n"
        ));
        out.push_str("    wirespec_result_t r;\n");
    }

    // Emit parse items, tracking checksum field offset
    if needs_offset {
        let cksum_plan = packet.checksum_plan.as_ref().unwrap();
        emit_parse_items_with_cksum(
            out,
            &resolved_fields,
            &packet.items,
            prefix,
            "    ",
            "out->",
            &cksum_plan.field_name,
        );
        out.push_str("    *_cksum_offset_out = _cksum_offset;\n");
    } else {
        parse_emit::emit_parse_items(
            out,
            &resolved_fields,
            &packet.items,
            prefix,
            "    ",
            "out->",
        );
    }

    out.push_str("    (void)r;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");

    // Public parse function
    out.push_str(&format!(
        "wirespec_result_t {public_fn}(const uint8_t *buf, size_t len, {tname} *out, size_t *consumed) {{\n"
    ));
    out.push_str("    wirespec_cursor_t cur;\n");
    out.push_str("    wirespec_cursor_init(&cur, buf, len);\n");

    if needs_offset {
        out.push_str("    size_t _cksum_offset = 0;\n");
        out.push_str(&format!(
            "    wirespec_result_t r = {cursor_fn}(&cur, out, &_cksum_offset);\n"
        ));
    } else {
        out.push_str(&format!(
            "    wirespec_result_t r = {cursor_fn}(&cur, out);\n"
        ));
    }

    out.push_str("    if (r != WIRESPEC_OK) return r;\n");
    out.push_str("    *consumed = wirespec_cursor_consumed(&cur);\n");

    // Checksum verify after parse
    if let Some(ref plan) = packet.checksum_plan {
        emit_checksum_verify(out, plan, "    ");
    }

    out.push_str("    (void)r;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");
}

fn emit_packet_serialize(
    out: &mut String,
    packet: &CodecPacket,
    prefix: &str,
    enums: &[SemanticEnum],
) {
    let tname = c_type_name(prefix, &packet.name);
    let fn_name = c_func_name(prefix, &packet.name, "serialize");
    let resolved_fields = resolve_enum_fields(&packet.fields, enums);

    out.push_str(&format!(
        "wirespec_result_t {fn_name}(const {tname} *val, uint8_t *buf, size_t cap, size_t *written) {{\n"
    ));
    out.push_str("    wirespec_result_t r;\n");
    out.push_str("    size_t pos = 0;\n");

    let has_cksum = packet.checksum_plan.is_some();
    if has_cksum {
        out.push_str("    size_t _cksum_offset = 0;\n");
    }

    if has_cksum {
        let cksum_plan = packet.checksum_plan.as_ref().unwrap();
        emit_serialize_items_with_cksum(
            out,
            &resolved_fields,
            &packet.items,
            prefix,
            "    ",
            &cksum_plan.field_name,
        );
    } else {
        serialize_emit::emit_serialize_items(out, &resolved_fields, &packet.items, prefix, "    ");
    }

    // Checksum compute after all fields written
    if let Some(ref plan) = packet.checksum_plan {
        emit_checksum_compute(out, plan, "    ");
    }

    out.push_str("    *written = pos;\n");
    out.push_str("    (void)r;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");
}

fn emit_packet_serialized_len(
    out: &mut String,
    packet: &CodecPacket,
    prefix: &str,
    enums: &[SemanticEnum],
) {
    let tname = c_type_name(prefix, &packet.name);
    let fn_name = c_func_name(prefix, &packet.name, "serialized_len");
    let resolved_fields = resolve_enum_fields(&packet.fields, enums);

    out.push_str(&format!("size_t {fn_name}(const {tname} *val) {{\n"));
    out.push_str("    (void)val;\n");
    out.push_str("    size_t len = 0;\n");

    serialize_emit::emit_serialized_len_items(
        out,
        &resolved_fields,
        &packet.items,
        prefix,
        "    ",
        "val->",
    );

    out.push_str("    return len;\n");
    out.push_str("}\n\n");
}

// ── Frame ──

fn resolve_frame(frame: &CodecFrame, enums: &[SemanticEnum]) -> CodecFrame {
    let mut resolved = frame.clone();
    for variant in &mut resolved.variants {
        variant.fields = resolve_enum_fields(&variant.fields, enums);
    }
    resolved
}

fn resolve_capsule(capsule: &CodecCapsule, enums: &[SemanticEnum]) -> CodecCapsule {
    let mut resolved = capsule.clone();
    resolved.header_fields = resolve_enum_fields(&resolved.header_fields, enums);
    for variant in &mut resolved.variants {
        variant.fields = resolve_enum_fields(&variant.fields, enums);
    }
    resolved
}

fn emit_frame_parse(out: &mut String, frame: &CodecFrame, prefix: &str, _enums: &[SemanticEnum]) {
    let tname = c_type_name(prefix, &frame.name);
    let cursor_fn = c_func_name(prefix, &frame.name, "parse_cursor");
    let public_fn = c_func_name(prefix, &frame.name, "parse");

    // Static cursor-based parse
    out.push_str(&format!(
        "static wirespec_result_t {cursor_fn}(wirespec_cursor_t *cur, {tname} *out) {{\n"
    ));
    out.push_str("    wirespec_result_t r;\n");

    parse_emit::emit_frame_parse_body(out, frame, prefix, "    ");

    out.push_str("    (void)r;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");

    // Public parse function
    out.push_str(&format!(
        "wirespec_result_t {public_fn}(const uint8_t *buf, size_t len, {tname} *out, size_t *consumed) {{\n"
    ));
    out.push_str("    wirespec_cursor_t cur;\n");
    out.push_str("    wirespec_cursor_init(&cur, buf, len);\n");
    out.push_str(&format!(
        "    wirespec_result_t r = {cursor_fn}(&cur, out);\n"
    ));
    out.push_str("    if (r != WIRESPEC_OK) return r;\n");
    out.push_str("    *consumed = wirespec_cursor_consumed(&cur);\n");
    out.push_str("    (void)r;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");
}

fn emit_frame_serialize(
    out: &mut String,
    frame: &CodecFrame,
    prefix: &str,
    _enums: &[SemanticEnum],
) {
    let tname = c_type_name(prefix, &frame.name);
    let fn_name = c_func_name(prefix, &frame.name, "serialize");

    out.push_str(&format!(
        "wirespec_result_t {fn_name}(const {tname} *val, uint8_t *buf, size_t cap, size_t *written) {{\n"
    ));
    out.push_str("    wirespec_result_t r;\n");
    out.push_str("    size_t pos = 0;\n");

    serialize_emit::emit_frame_serialize_body(out, frame, prefix, "    ");

    out.push_str("    *written = pos;\n");
    out.push_str("    (void)r;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");
}

fn emit_frame_serialized_len(
    out: &mut String,
    frame: &CodecFrame,
    prefix: &str,
    _enums: &[SemanticEnum],
) {
    let tname = c_type_name(prefix, &frame.name);
    let fn_name = c_func_name(prefix, &frame.name, "serialized_len");
    let tag_type = c_frame_tag_type(prefix, &frame.name);

    out.push_str(&format!("size_t {fn_name}(const {tname} *val) {{\n"));
    out.push_str("    (void)val;\n");
    out.push_str("    size_t len = 0;\n");

    // Tag length
    match &frame.tag.wire_type {
        WireType::VarInt | WireType::ContVarInt => {
            if let Some(ref ref_name) = frame.tag.ref_type_name {
                let wire_size_fn = c_func_name(prefix, ref_name, "wire_size");
                out.push_str(&format!("    len += {wire_size_fn}(val->frame_type);\n"));
            }
        }
        _ => {
            if let Some(w) = crate::type_map::wire_type_byte_width(&frame.tag.wire_type) {
                out.push_str(&format!("    len += {w};\n"));
            }
        }
    }

    // Switch on tag for variant-specific lengths
    out.push_str("    switch (val->tag) {\n");
    for variant in &frame.variants {
        let tag_val = c_frame_tag_value(prefix, &frame.name, &variant.name);
        let vname = to_snake_case(&variant.name);
        out.push_str(&format!("    case {tag_val}:\n"));
        let val_prefix = format!("val->value.{vname}.");
        serialize_emit::emit_serialized_len_items(
            out,
            &variant.fields,
            &variant.items,
            prefix,
            "        ",
            &val_prefix,
        );
        out.push_str("        break;\n");
    }
    // Need a default case to avoid warnings
    let _ = tag_type;
    out.push_str("    default: break;\n");
    out.push_str("    }\n");

    out.push_str("    return len;\n");
    out.push_str("}\n\n");
}

// ── Capsule ──

fn emit_capsule_parse(
    out: &mut String,
    capsule: &CodecCapsule,
    prefix: &str,
    _enums: &[SemanticEnum],
) {
    let tname = c_type_name(prefix, &capsule.name);
    let cursor_fn = c_func_name(prefix, &capsule.name, "parse_cursor");
    let public_fn = c_func_name(prefix, &capsule.name, "parse");

    // Static cursor-based parse
    out.push_str(&format!(
        "static wirespec_result_t {cursor_fn}(wirespec_cursor_t *cur, {tname} *out) {{\n"
    ));
    out.push_str("    wirespec_result_t r;\n");

    parse_emit::emit_capsule_parse_body(out, capsule, prefix, "    ");

    out.push_str("    (void)r;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");

    // Public parse function
    out.push_str(&format!(
        "wirespec_result_t {public_fn}(const uint8_t *buf, size_t len, {tname} *out, size_t *consumed) {{\n"
    ));
    out.push_str("    wirespec_cursor_t cur;\n");
    out.push_str("    wirespec_cursor_init(&cur, buf, len);\n");
    out.push_str(&format!(
        "    wirespec_result_t r = {cursor_fn}(&cur, out);\n"
    ));
    out.push_str("    if (r != WIRESPEC_OK) return r;\n");
    out.push_str("    *consumed = wirespec_cursor_consumed(&cur);\n");
    out.push_str("    (void)r;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");
}

fn emit_capsule_serialize(
    out: &mut String,
    capsule: &CodecCapsule,
    prefix: &str,
    _enums: &[SemanticEnum],
) {
    let tname = c_type_name(prefix, &capsule.name);
    let fn_name = c_func_name(prefix, &capsule.name, "serialize");

    out.push_str(&format!(
        "wirespec_result_t {fn_name}(const {tname} *val, uint8_t *buf, size_t cap, size_t *written) {{\n"
    ));
    out.push_str("    wirespec_result_t r;\n");
    out.push_str("    size_t pos = 0;\n");

    // Serialize header fields
    serialize_emit::emit_serialize_items(
        out,
        &capsule.header_fields,
        &capsule.header_items,
        prefix,
        "    ",
    );

    // Serialize payload variants
    out.push_str("    switch (val->tag) {\n");
    for variant in &capsule.variants {
        let tag_val = c_frame_tag_value(prefix, &capsule.name, &variant.name);
        let vname = to_snake_case(&variant.name);
        out.push_str(&format!("    case {tag_val}: {{\n"));

        let val_prefix = format!("val->value.{vname}.");
        serialize_emit::emit_variant_serialize_items_public(
            out,
            &variant.fields,
            &variant.items,
            prefix,
            "        ",
            &val_prefix,
        );

        out.push_str("        break;\n");
        out.push_str("    }\n");
    }
    out.push_str("    default: break;\n");
    out.push_str("    }\n");

    out.push_str("    *written = pos;\n");
    out.push_str("    (void)r;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");
}

fn emit_capsule_serialized_len(
    out: &mut String,
    capsule: &CodecCapsule,
    prefix: &str,
    _enums: &[SemanticEnum],
) {
    let tname = c_type_name(prefix, &capsule.name);
    let fn_name = c_func_name(prefix, &capsule.name, "serialized_len");

    out.push_str(&format!("size_t {fn_name}(const {tname} *val) {{\n"));
    out.push_str("    (void)val;\n");
    out.push_str("    size_t len = 0;\n");

    // Header field lengths
    serialize_emit::emit_serialized_len_items(
        out,
        &capsule.header_fields,
        &capsule.header_items,
        prefix,
        "    ",
        "val->",
    );

    // Payload variant lengths
    out.push_str("    switch (val->tag) {\n");
    for variant in &capsule.variants {
        let tag_val = c_frame_tag_value(prefix, &capsule.name, &variant.name);
        let vname = to_snake_case(&variant.name);
        out.push_str(&format!("    case {tag_val}:\n"));
        let val_prefix = format!("val->value.{vname}.");
        serialize_emit::emit_serialized_len_items(
            out,
            &variant.fields,
            &variant.items,
            prefix,
            "        ",
            &val_prefix,
        );
        out.push_str("        break;\n");
    }
    out.push_str("    default: break;\n");
    out.push_str("    }\n");

    out.push_str("    return len;\n");
    out.push_str("}\n\n");
}

// ── State machine dispatch ──

fn emit_sm_dispatch(out: &mut String, sm: &SemanticStateMachine, prefix: &str) {
    let sm_snake = to_snake_case(&sm.name);
    let sm_upper = sm_snake.to_uppercase();
    let prefix_upper = prefix.to_uppercase();

    let sm_type = format!("{prefix}_{sm_snake}_t");
    let event_type = format!("{prefix}_{sm_snake}_event_t");
    let dispatch_fn = format!("{prefix}_{sm_snake}_dispatch");

    out.push_str(&format!(
        "wirespec_result_t {dispatch_fn}(\n    {sm_type} *sm,\n    const {event_type} *event)\n{{\n"
    ));
    out.push_str(&format!(
        "    {prefix}_{sm_snake}_state_tag_t src_tag = sm->tag;\n\n"
    ));

    // Separate transitions into specific (non-wildcard) and wildcard
    let specific_transitions: Vec<_> = sm
        .transitions
        .iter()
        .filter(|t| t.src_state_name != "*")
        .collect();
    let wildcard_transitions: Vec<_> = sm
        .transitions
        .iter()
        .filter(|t| t.src_state_name == "*")
        .collect();

    // Group specific transitions by source state
    let mut state_groups: std::collections::BTreeMap<String, Vec<_>> =
        std::collections::BTreeMap::new();
    for t in &specific_transitions {
        state_groups
            .entry(t.src_state_name.clone())
            .or_default()
            .push(*t);
    }

    // Emit specific state transitions
    let mut first_state = true;
    for (src_state_name, transitions) in &state_groups {
        let src_upper = to_snake_case(src_state_name).to_uppercase();
        let src_snake = to_snake_case(src_state_name);
        let if_kw = if first_state { "if" } else { "} else if" };
        out.push_str(&format!(
            "    {if_kw} (src_tag == {prefix_upper}_{sm_upper}_{src_upper}) {{\n"
        ));

        let mut first_event = true;
        for t in transitions {
            let event_upper = to_snake_case(&t.event_name).to_uppercase();
            let dst_snake = to_snake_case(&t.dst_state_name);
            let dst_upper = dst_snake.to_uppercase();

            let ev_if_kw = if first_event { "if" } else { "} else if" };
            out.push_str(&format!(
                "        {ev_if_kw} (event->tag == {prefix_upper}_{sm_upper}_EVENT_{event_upper}) {{\n"
            ));

            let expr_ctx = SmExprContext {
                src_state_snake: &src_snake,
                dst_state_snake: &dst_snake,
                event_snake: &to_snake_case(&t.event_name),
                sm: Some(sm),
                prefix,
            };

            // Guard check (with special handling for All expressions)
            if let Some(ref guard) = t.guard {
                emit_guard(out, guard, &expr_ctx, "            ");
            }

            // Build destination state
            out.push_str(&format!("            {sm_type} dst;\n"));
            out.push_str(&format!(
                "            dst.tag = {prefix_upper}_{sm_upper}_{dst_upper};\n"
            ));

            // Actions: assign fields (with special handling for Fill)
            for action in &t.actions {
                emit_action(out, action, &expr_ctx, "            ");
            }

            // Delegate: child SM dispatch
            if let Some(ref delegate) = t.delegate {
                emit_delegate(
                    out,
                    delegate,
                    sm,
                    &expr_ctx,
                    prefix,
                    &src_snake,
                    "            ",
                );
            }

            out.push_str("            *sm = dst;\n");
            out.push_str("            return WIRESPEC_OK;\n");

            first_event = false;
        }
        if !first_event {
            out.push_str("        }\n");
        }

        first_state = false;
    }

    // Wildcard transitions: apply to all non-terminal states as a fallback
    if !wildcard_transitions.is_empty() {
        // If we had specific states, close the else-if chain first
        if !first_state {
            out.push_str("    }\n\n");
            out.push_str("    /* Wildcard transitions */\n");
        }

        for t in &wildcard_transitions {
            let event_upper = to_snake_case(&t.event_name).to_uppercase();
            let dst_snake = to_snake_case(&t.dst_state_name);
            let dst_upper = dst_snake.to_uppercase();

            out.push_str(&format!(
                "    if (event->tag == {prefix_upper}_{sm_upper}_EVENT_{event_upper}) {{\n"
            ));

            // Wildcard transitions apply to any non-terminal state.
            // Since the src state is dynamic, we can't dereference specific src fields
            // without a switch. For wildcards with actions, we'd need per-state handling.
            // For simple wildcards (no actions referencing src), just set the tag.
            out.push_str(&format!("        {sm_type} dst;\n"));
            out.push_str(&format!(
                "        dst.tag = {prefix_upper}_{sm_upper}_{dst_upper};\n"
            ));

            for action in &t.actions {
                // For wildcard actions, use empty src context
                let expr_ctx = SmExprContext {
                    src_state_snake: "_wildcard",
                    dst_state_snake: &dst_snake,
                    event_snake: &to_snake_case(&t.event_name),
                    sm: Some(sm),
                    prefix,
                };
                emit_action(out, action, &expr_ctx, "        ");
            }

            out.push_str("        *sm = dst;\n");
            out.push_str("        return WIRESPEC_OK;\n");
            out.push_str("    }\n");
        }
    } else if !first_state {
        // Close the final else-if brace for specific states
        out.push_str("    }\n");
    }

    out.push_str("\n    return WIRESPEC_ERR_INVALID_STATE;\n");
    out.push_str("}\n\n");
}

// ── SM helper: emit guard ──

/// Emit a guard check. If the guard is an `All` expression, emit it as a
/// statement-level block instead of an inline expression.
fn emit_guard(out: &mut String, guard: &SemanticExpr, ctx: &SmExprContext, indent: &str) {
    match guard {
        SemanticExpr::All {
            collection,
            sm_name,
            state_name,
            ..
        } => {
            let sm_snake = to_snake_case(sm_name);
            let state_snake = to_snake_case(state_name);
            let prefix_upper = ctx.prefix.to_uppercase();
            let sm_upper = sm_snake.to_uppercase();
            let state_upper = state_snake.to_uppercase();
            let tag = format!("{prefix_upper}_{sm_upper}_{state_upper}");

            match collection.as_ref() {
                SemanticExpr::Slice { base, start, end } => {
                    let base_c = crate::expr::sema_expr_to_c(base, ctx);
                    let start_c = crate::expr::sema_expr_to_c(start, ctx);
                    let end_c = crate::expr::sema_expr_to_c(end, ctx);
                    out.push_str(&format!("{indent}{{\n"));
                    out.push_str(&format!("{indent}    bool _all_ok = true;\n"));
                    out.push_str(&format!(
                        "{indent}    for (size_t _aj = (size_t)({start_c}); _aj < (size_t)({end_c}); _aj++) {{\n"
                    ));
                    out.push_str(&format!(
                        "{indent}        if ({base_c}[_aj].tag != {tag}) {{ _all_ok = false; break; }}\n"
                    ));
                    out.push_str(&format!("{indent}    }}\n"));
                    out.push_str(&format!(
                        "{indent}    if (!_all_ok) return WIRESPEC_ERR_CONSTRAINT;\n"
                    ));
                    out.push_str(&format!("{indent}}}\n"));
                }
                _ => {
                    let coll_c = crate::expr::sema_expr_to_c(collection, ctx);
                    out.push_str(&format!(
                        "{indent}/* all() guard: non-slice collection ({coll_c}) — treating as no-op */\n"
                    ));
                }
            }
        }
        _ => {
            let guard_c = crate::expr::sema_expr_to_c(guard, ctx);
            out.push_str(&format!(
                "{indent}if (!({guard_c})) return WIRESPEC_ERR_INVALID_STATE;\n"
            ));
        }
    }
}

// ── SM helper: emit action ──

/// Emit an action assignment. If the value is a `Fill` expression, emit a
/// for-loop instead of a simple assignment.
fn emit_action(out: &mut String, action: &SemanticAction, ctx: &SmExprContext, indent: &str) {
    match &action.value {
        SemanticExpr::Fill { value, count } => {
            let target_c = crate::expr::sema_expr_to_c(&action.target, ctx);
            let value_c = crate::expr::sema_expr_to_c(value, ctx);
            let count_c = crate::expr::sema_expr_to_c(count, ctx);
            out.push_str(&format!(
                "{indent}for (size_t _fi = 0; _fi < (size_t)({count_c}); _fi++) {{\n"
            ));
            out.push_str(&format!("{indent}    {target_c}[_fi] = {value_c};\n"));
            out.push_str(&format!("{indent}}}\n"));
        }
        _ => {
            let target_c = crate::expr::sema_expr_to_c(&action.target, ctx);
            let value_c = crate::expr::sema_expr_to_c(&action.value, ctx);
            let op = &action.op;
            // Check if the target is an array field — C can't assign arrays
            // directly, so use memcpy instead.
            let is_array_field =
                if let SemanticExpr::TransitionPeerRef { reference } = &action.target {
                    if let Some(field_name) = reference.path.first() {
                        ctx.sm.is_some_and(|sm| {
                            sm.states.iter().any(|s| {
                                s.fields.iter().any(|f| {
                                    &f.name == field_name
                                        && matches!(
                                            &f.ty,
                                            wirespec_sema::types::SemanticType::Array { .. }
                                        )
                                })
                            })
                        })
                    } else {
                        false
                    }
                } else {
                    false
                };
            if is_array_field {
                out.push_str(&format!(
                    "{indent}memcpy({target_c}, {value_c}, sizeof({target_c}));\n"
                ));
            } else {
                out.push_str(&format!("{indent}{target_c} {op} {value_c};\n"));
            }
        }
    }
}

// ── SM helper: emit delegate (child dispatch) ──

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

/// Emit delegate child dispatch code. This auto-copies dst from sm,
/// dispatches to the child SM, and re-dispatches the parent if the child's
/// state changed.
fn emit_delegate(
    out: &mut String,
    delegate: &SemanticDelegate,
    sm: &SemanticStateMachine,
    _ctx: &SmExprContext,
    prefix: &str,
    src_state_snake: &str,
    indent: &str,
) {
    // Extract the child field name from TransitionPeerRef or Subscript(TransitionPeerRef, index).
    let field_name = extract_delegate_child_field(&delegate.target);
    let is_indexed = matches!(delegate.target, SemanticExpr::Subscript { .. });

    // Look up the child SM type from the source state's field definitions
    let child_sm_name = field_name.as_ref().and_then(|fname| {
        sm.states
            .iter()
            .find(|s| to_snake_case(&s.name) == src_state_snake)
            .and_then(|state| state.fields.iter().find(|f| &f.name == fname))
            .and_then(|f| f.child_sm_name.clone())
    });

    if let (Some(field_name), Some(child_sm)) = (&field_name, &child_sm_name) {
        let child_snake = to_snake_case(child_sm);
        let _child_type = format!("{prefix}_{child_snake}_t");
        let child_event_type = format!("{prefix}_{child_snake}_event_t");
        let child_dispatch_fn = format!("{prefix}_{child_snake}_dispatch");
        let child_tag_type = format!("{prefix}_{child_snake}_state_tag_t");
        let field_snake = to_snake_case(field_name);

        let prefix_upper = prefix.to_uppercase();
        let sm_snake = to_snake_case(&sm.name);
        let sm_upper = sm_snake.to_uppercase();

        out.push_str(&format!(
            "{indent}/* delegate: child dispatch to {child_sm} */\n"
        ));
        out.push_str(&format!("{indent}dst = *sm;\n"));

        // Build child event: the delegate event_name is the parent event
        // parameter that carries the child event tag value (a u8), so cast it
        // to the child event tag type.
        let delegate_param_snake = to_snake_case(&delegate.event_name);
        let child_event_tag_type = format!("{prefix}_{child_snake}_event_tag_t");
        let parent_event_snake = to_snake_case(_ctx.event_snake);
        out.push_str(&format!("{indent}{child_event_type} _child_ev;\n"));
        out.push_str(&format!(
            "{indent}_child_ev.tag = ({child_event_tag_type})event->{parent_event_snake}.{delegate_param_snake};\n"
        ));

        if is_indexed {
            // Indexed delegate: src.field[index] <- ev
            let index_expr = if let SemanticExpr::Subscript { index, .. } = &delegate.target {
                crate::expr::sema_expr_to_c(index, _ctx)
            } else {
                "0".to_string()
            };
            let child_ref = format!("dst.{src_state_snake}.{field_snake}[_idx]");

            out.push_str(&format!("{indent}{{\n"));
            out.push_str(&format!(
                "{indent}    size_t _idx = (size_t)({index_expr});\n"
            ));
            out.push_str(&format!(
                "{indent}    {child_tag_type} _old_tag = {child_ref}.tag;\n"
            ));
            out.push_str(&format!(
                "{indent}    wirespec_result_t _rc = {child_dispatch_fn}(\n{indent}        &{child_ref}, &_child_ev);\n"
            ));
            out.push_str(&format!(
                "{indent}    if (_rc != WIRESPEC_OK) return _rc;\n"
            ));

            if !sm.has_child_state_changed {
                out.push_str(&format!("{indent}    (void)_old_tag;\n"));
            }
            if sm.has_child_state_changed {
                out.push_str(&format!(
                    "{indent}    if ({child_ref}.tag != _old_tag) {{\n"
                ));
                let event_type = format!("{prefix}_{sm_snake}_event_t");
                out.push_str(&format!("{indent}        {event_type} _csc_ev;\n"));
                out.push_str(&format!(
                    "{indent}        _csc_ev.tag = {prefix_upper}_{sm_upper}_EVENT_CHILD_STATE_CHANGED;\n"
                ));
                out.push_str(&format!("{indent}        *sm = dst;\n"));
                out.push_str(&format!(
                    "{indent}        wirespec_result_t _csc_rc = {prefix}_{sm_snake}_dispatch(sm, &_csc_ev);\n"
                ));
                out.push_str(&format!(
                    "{indent}        if (_csc_rc != WIRESPEC_OK && _csc_rc != WIRESPEC_ERR_INVALID_STATE)\n"
                ));
                out.push_str(&format!("{indent}            return _csc_rc;\n"));
                out.push_str(&format!("{indent}        return WIRESPEC_OK;\n"));
                out.push_str(&format!("{indent}    }}\n"));
            }
            out.push_str(&format!("{indent}}}\n"));
        } else {
            // Simple delegate: src.field <- ev
            out.push_str(&format!(
                "{indent}{child_tag_type} _old_tag = dst.{src_state_snake}.{field_snake}.tag;\n"
            ));

            // Dispatch to child
            out.push_str(&format!(
                "{indent}wirespec_result_t _rc = {child_dispatch_fn}(\n{indent}    &dst.{src_state_snake}.{field_snake}, &_child_ev);\n"
            ));
            out.push_str(&format!("{indent}if (_rc != WIRESPEC_OK) return _rc;\n"));

            // Re-dispatch if child tag changed and parent handles child_state_changed
            if !sm.has_child_state_changed {
                out.push_str(&format!("{indent}(void)_old_tag;\n"));
            }
            if sm.has_child_state_changed {
                out.push_str(&format!(
                    "{indent}if (dst.{src_state_snake}.{field_snake}.tag != _old_tag) {{\n"
                ));
                let event_type = format!("{prefix}_{sm_snake}_event_t");
                out.push_str(&format!("{indent}    {event_type} _csc_ev;\n"));
                out.push_str(&format!(
                    "{indent}    _csc_ev.tag = {prefix_upper}_{sm_upper}_EVENT_CHILD_STATE_CHANGED;\n"
                ));
                out.push_str(&format!("{indent}    *sm = dst;\n"));
                out.push_str(&format!(
                    "{indent}    wirespec_result_t _csc_rc = {prefix}_{sm_snake}_dispatch(sm, &_csc_ev);\n"
                ));
                out.push_str(&format!(
                    "{indent}    if (_csc_rc != WIRESPEC_OK && _csc_rc != WIRESPEC_ERR_INVALID_STATE)\n"
                ));
                out.push_str(&format!("{indent}        return _csc_rc;\n"));
                out.push_str(&format!("{indent}    return WIRESPEC_OK;\n"));
                out.push_str(&format!("{indent}}}\n"));
            }
        }
    } else {
        // Cannot resolve child SM type — emit documented comment
        out.push_str(&format!(
            "{indent}/* delegate: auto-copy + child dispatch */\n"
        ));
        out.push_str(&format!(
            "{indent}/* TODO: child SM dispatch requires runtime type resolution */\n"
        ));
        out.push_str(&format!("{indent}dst = *sm; /* auto-copy (rule 2b) */\n"));
    }
}

// ── VarInt ──

fn emit_varint_source(out: &mut String, vi: &SemanticVarInt, prefix: &str) {
    match vi.encoding {
        VarIntEncoding::PrefixMatch => emit_varint_prefix_match(out, vi, prefix),
        VarIntEncoding::ContinuationBit => emit_varint_continuation_bit(out, vi, prefix),
    }
}

fn emit_varint_prefix_match(out: &mut String, vi: &SemanticVarInt, prefix: &str) {
    let snake = to_snake_case(&vi.name);
    let tname = format!("{prefix}_{snake}_t");
    let parse_fn = format!("{prefix}_{snake}_parse");
    let serialize_fn = format!("{prefix}_{snake}_serialize");
    let wire_size_fn = format!("{prefix}_{snake}_wire_size");
    let prefix_bits = vi.prefix_bits.unwrap_or(2);

    // ── Parse ──
    out.push_str(&format!(
        "wirespec_result_t\n{parse_fn}(const uint8_t *buf, size_t len,\n    {tname} *out, size_t *consumed)\n{{\n"
    ));
    out.push_str("    if (len < 1) return WIRESPEC_ERR_SHORT_BUFFER;\n\n");
    out.push_str(&format!(
        "    uint8_t prefix = buf[0] >> {};\n",
        8 - prefix_bits
    ));
    out.push_str("    switch (prefix) {\n");

    for branch in &vi.branches {
        out.push_str(&format!("    case {}: {{\n", branch.prefix_value));
        let total = branch.total_bytes as usize;
        if total > 1 {
            out.push_str(&format!(
                "        if (len < {total}) return WIRESPEC_ERR_SHORT_BUFFER;\n"
            ));
        }
        // First byte: mask off prefix bits to get value bits
        let first_mask = (1u64 << (8 - prefix_bits)) - 1;
        if total == 1 {
            out.push_str(&format!("        *out = buf[0] & 0x{first_mask:X};\n"));
        } else {
            // Multi-byte: assemble value from all bytes
            out.push_str(&format!(
                "        *out = ((uint64_t)(buf[0] & 0x{first_mask:X}) << {})\n",
                (total - 1) * 8
            ));
            for i in 1..total {
                let shift = (total - 1 - i) * 8;
                if i == total - 1 {
                    out.push_str(&format!("             | (uint64_t)buf[{i}];\n"));
                } else {
                    out.push_str(&format!("             | ((uint64_t)buf[{i}] << {shift})\n"));
                }
            }
        }
        out.push_str(&format!("        *consumed = {total};\n"));

        // Strict noncanonical check
        if vi.strict && branch.prefix_value > 0 {
            let prev_branch = &vi.branches[(branch.prefix_value as usize) - 1];
            out.push_str(&format!(
                "        if (*out <= {}ULL) return WIRESPEC_ERR_NONCANONICAL;\n",
                prev_branch.max_value
            ));
        }

        out.push_str("        break;\n");
        out.push_str("    }\n");
    }

    out.push_str("    default:\n");
    out.push_str("        return WIRESPEC_ERR_INVALID_TAG;\n");
    out.push_str("    }\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");

    // ── Serialize ──
    out.push_str(&format!(
        "wirespec_result_t\n{serialize_fn}({tname} val,\n    uint8_t *buf, size_t cap, size_t *written)\n{{\n"
    ));

    for (i, branch) in vi.branches.iter().enumerate() {
        let total = branch.total_bytes as usize;
        let prefix_marker = branch.prefix_value << (8 - prefix_bits);
        if i == 0 {
            out.push_str(&format!("    if (val <= {}ULL) {{\n", branch.max_value));
        } else {
            out.push_str(&format!(
                "    }} else if (val <= {}ULL) {{\n",
                branch.max_value
            ));
        }

        out.push_str(&format!(
            "        if (cap < {total}) return WIRESPEC_ERR_SHORT_BUFFER;\n"
        ));

        if total == 1 {
            if prefix_marker == 0 {
                out.push_str("        buf[0] = (uint8_t)val;\n");
            } else {
                out.push_str(&format!(
                    "        buf[0] = 0x{prefix_marker:02X} | (uint8_t)val;\n"
                ));
            }
        } else {
            let top_shift = (total - 1) * 8;
            if prefix_marker == 0 {
                out.push_str(&format!(
                    "        buf[0] = (uint8_t)(val >> {top_shift});\n"
                ));
            } else {
                out.push_str(&format!(
                    "        buf[0] = 0x{prefix_marker:02X} | (uint8_t)(val >> {top_shift});\n"
                ));
            }
            for j in 1..total {
                let shift = (total - 1 - j) * 8;
                if shift == 0 {
                    out.push_str(&format!("        buf[{j}] = (uint8_t)val;\n"));
                } else {
                    out.push_str(&format!("        buf[{j}] = (uint8_t)(val >> {shift});\n"));
                }
            }
        }

        out.push_str(&format!("        *written = {total};\n"));
    }

    out.push_str("    } else {\n");
    out.push_str("        return WIRESPEC_ERR_OVERFLOW;\n");
    out.push_str("    }\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");

    // ── Wire size ──
    out.push_str(&format!("size_t\n{wire_size_fn}({tname} val)\n{{\n"));

    for (i, branch) in vi.branches.iter().enumerate() {
        let total = branch.total_bytes;
        if i < vi.branches.len() - 1 {
            out.push_str(&format!(
                "    if (val <= {}ULL) return {total};\n",
                branch.max_value
            ));
        } else {
            out.push_str(&format!("    return {total};\n"));
        }
    }

    out.push_str("}\n\n");

    // ── parse_cursor (static inline helper) ──
    let parse_cursor_fn = format!("{prefix}_{snake}_parse_cursor");
    out.push_str(&format!(
        "static inline wirespec_result_t {parse_cursor_fn}(\n    wirespec_cursor_t *cur, {tname} *out)\n{{\n"
    ));
    out.push_str("    size_t consumed = 0;\n");
    out.push_str(&format!(
        "    wirespec_result_t r = {parse_fn}(\n        cur->base + cur->pos, wirespec_cursor_remaining(cur),\n        out, &consumed);\n"
    ));
    out.push_str("    if (r != WIRESPEC_OK) return r;\n");
    out.push_str("    cur->pos += consumed;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");
}

fn emit_varint_continuation_bit(out: &mut String, vi: &SemanticVarInt, prefix: &str) {
    let snake = to_snake_case(&vi.name);
    let tname = format!("{prefix}_{snake}_t");
    let parse_fn = format!("{prefix}_{snake}_parse");
    let serialize_fn = format!("{prefix}_{snake}_serialize");
    let wire_size_fn = format!("{prefix}_{snake}_wire_size");

    let value_bits = vi.value_bits_per_byte.unwrap_or(7) as u32;
    let max_bytes = vi.max_bytes as u32;
    let cont_mask: u8 = 0x80; // MSB continuation bit
    let value_mask: u8 = (1u16 << value_bits) as u8 - 1;

    // ── Parse ──
    out.push_str(&format!(
        "wirespec_result_t\n{parse_fn}(const uint8_t *buf, size_t len,\n    {tname} *out, size_t *consumed)\n{{\n"
    ));
    out.push_str("    uint64_t result = 0;\n");
    out.push_str(&format!(
        "    for (size_t i = 0; i < {max_bytes}; i++) {{\n"
    ));
    out.push_str("        if (i >= len) return WIRESPEC_ERR_SHORT_BUFFER;\n");
    out.push_str("        uint8_t byte = buf[i];\n");

    match vi.byte_order {
        wirespec_sema::types::Endianness::Little => {
            out.push_str(&format!(
                "        result |= (uint64_t)(byte & 0x{value_mask:02X}) << (i * {value_bits});\n"
            ));
        }
        wirespec_sema::types::Endianness::Big => {
            out.push_str(&format!(
                "        result = (result << {value_bits}) | (uint64_t)(byte & 0x{value_mask:02X});\n"
            ));
        }
    }

    out.push_str(&format!(
        "        if ((byte & 0x{cont_mask:02X}) == 0) {{\n"
    ));
    out.push_str("            *out = result;\n");
    out.push_str("            *consumed = i + 1;\n");
    out.push_str("            return WIRESPEC_OK;\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    return WIRESPEC_ERR_OVERFLOW;\n");
    out.push_str("}\n\n");

    // ── Serialize ──
    out.push_str(&format!(
        "wirespec_result_t\n{serialize_fn}({tname} val,\n    uint8_t *buf, size_t cap, size_t *written)\n{{\n"
    ));

    match vi.byte_order {
        wirespec_sema::types::Endianness::Little => {
            out.push_str("    size_t i = 0;\n");
            out.push_str("    do {\n");
            out.push_str("        if (i >= cap) return WIRESPEC_ERR_SHORT_BUFFER;\n");
            out.push_str(&format!(
                "        if (i >= {max_bytes}) return WIRESPEC_ERR_OVERFLOW;\n"
            ));
            out.push_str(&format!(
                "        buf[i] = (uint8_t)(val & 0x{value_mask:02X});\n"
            ));
            out.push_str(&format!("        val >>= {value_bits};\n"));
            out.push_str("        if (val > 0) {\n");
            out.push_str(&format!("            buf[i] |= 0x{cont_mask:02X};\n"));
            out.push_str("        }\n");
            out.push_str("        i++;\n");
            out.push_str("    } while (val > 0);\n");
            out.push_str("    *written = i;\n");
        }
        wirespec_sema::types::Endianness::Big => {
            out.push_str(&format!("    uint8_t tmp[{max_bytes}];\n"));
            out.push_str("    size_t n = 0;\n");
            out.push_str("    do {\n");
            out.push_str(&format!(
                "        if (n >= {max_bytes}) return WIRESPEC_ERR_OVERFLOW;\n"
            ));
            out.push_str(&format!(
                "        tmp[n++] = (uint8_t)(val & 0x{value_mask:02X});\n"
            ));
            out.push_str(&format!("        val >>= {value_bits};\n"));
            out.push_str("    } while (val > 0);\n");
            out.push_str("    if (n > cap) return WIRESPEC_ERR_SHORT_BUFFER;\n");
            out.push_str("    for (size_t i = 0; i < n; i++) {\n");
            out.push_str("        buf[i] = tmp[n - 1 - i];\n");
            out.push_str("        if (i < n - 1) {\n");
            out.push_str(&format!("            buf[i] |= 0x{cont_mask:02X};\n"));
            out.push_str("        }\n");
            out.push_str("    }\n");
            out.push_str("    *written = n;\n");
        }
    }

    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");

    // ── Wire size ──
    out.push_str(&format!("size_t\n{wire_size_fn}({tname} val)\n{{\n"));
    out.push_str("    size_t n = 1;\n");
    out.push_str(&format!(
        "    while (val >> {value_bits}) {{ n++; val >>= {value_bits}; }}\n"
    ));
    out.push_str(&format!("    if (n > {max_bytes}) n = {max_bytes};\n"));
    out.push_str("    return n;\n");
    out.push_str("}\n\n");

    // ── parse_cursor (static inline helper) ──
    let parse_cursor_fn = format!("{prefix}_{snake}_parse_cursor");
    out.push_str(&format!(
        "static inline wirespec_result_t {parse_cursor_fn}(\n    wirespec_cursor_t *cur, {tname} *out)\n{{\n"
    ));
    out.push_str("    size_t consumed = 0;\n");
    out.push_str(&format!(
        "    wirespec_result_t r = {parse_fn}(\n        cur->base + cur->pos, wirespec_cursor_remaining(cur),\n        out, &consumed);\n"
    ));
    out.push_str("    if (r != WIRESPEC_OK) return r;\n");
    out.push_str("    cur->pos += consumed;\n");
    out.push_str("    return WIRESPEC_OK;\n");
    out.push_str("}\n\n");
}

// ── Checksum helpers ──

/// Emit parse items with checksum field offset tracking.
fn emit_parse_items_with_cksum(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    prefix: &str,
    indent: &str,
    struct_prefix: &str,
    cksum_field_name: &str,
) {
    for item in items {
        if let CodecItem::Field { field_id } = item
            && let Some(f) = fields.iter().find(|f| &f.field_id == field_id)
            && f.name == cksum_field_name
        {
            out.push_str(&format!(
                "{indent}_cksum_offset = wirespec_cursor_consumed(cur);\n"
            ));
        }
        let single_items = [item.clone()];
        parse_emit::emit_parse_items(out, fields, &single_items, prefix, indent, struct_prefix);
    }
}

/// Emit serialize items with checksum field offset tracking.
fn emit_serialize_items_with_cksum(
    out: &mut String,
    fields: &[CodecField],
    items: &[CodecItem],
    prefix: &str,
    indent: &str,
    cksum_field_name: &str,
) {
    for item in items {
        if let CodecItem::Field { field_id } = item
            && let Some(f) = fields.iter().find(|f| &f.field_id == field_id)
            && f.name == cksum_field_name
        {
            out.push_str(&format!("{indent}_cksum_offset = pos;\n"));
        }
        let single_items = [item.clone()];
        serialize_emit::emit_serialize_items(out, fields, &single_items, prefix, indent);
    }
}

/// Emit checksum verification in the parse wrapper.
fn emit_checksum_verify(out: &mut String, plan: &ChecksumPlan, indent: &str) {
    use wirespec_sema::checksum_catalog;

    let algo = &plan.algorithm_id;
    let spec = checksum_catalog::lookup(algo);

    match spec.map(|s| s.verify_mode) {
        Some(checksum_catalog::ChecksumVerifyMode::ZeroSum) => {
            let width = plan.field_width_bytes;
            let ctype = if width == 2 { "uint16_t" } else { "uint32_t" };
            out.push_str(&format!(
                "{indent}{{\n{indent}    {ctype} _cksum = wirespec_{algo}_checksum(buf, *consumed);\n"
            ));
            out.push_str(&format!(
                "{indent}    if (_cksum != 0) return WIRESPEC_ERR_CHECKSUM;\n"
            ));
            out.push_str(&format!("{indent}}}\n"));
        }
        Some(checksum_catalog::ChecksumVerifyMode::RecomputeCompare) => {
            let width = plan.field_width_bytes;
            let ctype = if width == 2 { "uint16_t" } else { "uint32_t" };
            out.push_str(&format!(
                "{indent}{{\n{indent}    {ctype} _computed = wirespec_{algo}_verify(\n"
            ));
            out.push_str(&format!(
                "{indent}        buf, *consumed, _cksum_offset, {width});\n"
            ));
            out.push_str(&format!(
                "{indent}    if (out->{} != _computed)\n",
                plan.field_name
            ));
            out.push_str(&format!("{indent}        return WIRESPEC_ERR_CHECKSUM;\n"));
            out.push_str(&format!("{indent}}}\n"));
        }
        None => {
            unreachable!("unknown checksum algorithm: {algo}");
        }
    }
}

/// Emit checksum compute+patch in the serialize function.
fn emit_checksum_compute(out: &mut String, plan: &ChecksumPlan, indent: &str) {
    use wirespec_sema::checksum_catalog;

    let algo = &plan.algorithm_id;
    let spec = checksum_catalog::lookup(algo);

    out.push_str(&format!(
        "{indent}/* @checksum({algo}): auto-compute and patch */\n"
    ));

    match spec.map(|s| s.verify_mode) {
        Some(checksum_catalog::ChecksumVerifyMode::ZeroSum) => {
            out.push_str(&format!(
                "{indent}wirespec_{algo}_checksum_compute(buf, pos, _cksum_offset);\n"
            ));
        }
        Some(checksum_catalog::ChecksumVerifyMode::RecomputeCompare) => {
            let width = plan.field_width_bytes;
            let ctype = if width == 2 { "uint16_t" } else { "uint32_t" };
            out.push_str(&format!("{indent}{{\n"));
            out.push_str(&format!(
                "{indent}    {ctype} _val = wirespec_{algo}_compute(buf, pos, _cksum_offset);\n"
            ));
            // Write bytes big-endian from MSB to LSB
            for i in 0..width {
                let shift = (width - 1 - i) * 8;
                out.push_str(&format!(
                    "{indent}    buf[_cksum_offset + {i}] = (uint8_t)((_val >> {shift}) & 0xFF);\n"
                ));
            }
            out.push_str(&format!("{indent}}}\n"));
        }
        None => {
            unreachable!("unknown checksum algorithm: {algo}");
        }
    }
}

/// Generate a libFuzzer fuzz harness that tests the round-trip property:
/// parse -> serialize -> re-parse -> re-serialize must be stable.
///
/// The harness targets the first frame, capsule, or packet in the module
/// (in that priority order). Returns `None` if the module has no parseable types.
pub fn emit_fuzz_source(module: &CodecModule, prefix: &str) -> Option<String> {
    // Select fuzz target: first frame > first capsule > first packet
    let type_snake = if let Some(f) = module.frames.first() {
        to_snake_case(&f.name)
    } else if let Some(c) = module.capsules.first() {
        to_snake_case(&c.name)
    } else if let Some(p) = module.packets.first() {
        to_snake_case(&p.name)
    } else {
        return None;
    };

    let tname = format!("{prefix}_{type_snake}_t");
    let parse_fn = format!("{prefix}_{type_snake}_parse");
    let serialize_fn = format!("{prefix}_{type_snake}_serialize");

    Some(format!(
        r#"/* Auto-generated fuzz harness -- DO NOT EDIT */
#include "{prefix}.h"
#include <stdint.h>
#include <stddef.h>
#include <string.h>

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {{
    /* Phase 1: Parse fuzz input */
    {tname} obj;
    size_t consumed;
    wirespec_result_t r = {parse_fn}(data, size, &obj, &consumed);
    if (r != WIRESPEC_OK) return 0;

    /* Phase 2: Serialize back */
    uint8_t buf[65536];
    size_t written;
    r = {serialize_fn}(&obj, buf, sizeof(buf), &written);
    if (r != WIRESPEC_OK) return 0;

    /* Phase 3: Re-parse our own output -- must succeed */
    {tname} obj2;
    size_t consumed2;
    r = {parse_fn}(buf, written, &obj2, &consumed2);
    if (r != WIRESPEC_OK) __builtin_trap();
    if (consumed2 != written) __builtin_trap();

    /* Phase 4: Re-serialize -- must be identical */
    uint8_t buf2[65536];
    size_t written2;
    r = {serialize_fn}(&obj2, buf2, sizeof(buf2), &written2);
    if (r != WIRESPEC_OK) __builtin_trap();
    if (written != written2 || memcmp(buf, buf2, written) != 0) __builtin_trap();

    return 0;
}}
"#
    ))
}
