// crates/wirespec-backend-c/src/header.rs
//
// .h header file emission: structs, enums, function declarations.

use wirespec_codec::ir::*;
use wirespec_sema::ir::{SemanticStateField, SemanticStateMachine};

use crate::names::*;
use crate::type_map::*;

const MAX_ARRAY_ELEMENTS: u32 = 256;

/// Build a map from imported type name to the source prefix that defines it.
/// Used to emit the correct C type name for imported types.
fn build_import_prefix_map(module: &CodecModule) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for imp in &module.imports {
        map.insert(imp.name.clone(), imp.source_prefix.clone());
    }
    map
}

/// Emit the complete .h header file content.
pub fn emit_header(module: &CodecModule, prefix: &str) -> String {
    let mut out = String::new();
    let guard = c_include_guard(prefix);
    let import_prefixes = build_import_prefix_map(module);

    // Include guard + standard includes
    out.push_str(&format!("#ifndef {guard}\n"));
    out.push_str(&format!("#define {guard}\n\n"));
    out.push_str("#include <stdint.h>\n");
    out.push_str("#include <stdbool.h>\n");
    out.push_str("#include <stddef.h>\n");
    out.push_str("#include \"wirespec_runtime.h\"\n");

    // Import includes
    emit_import_includes(&mut out, module);

    out.push('\n');

    // Const defines
    emit_const_defines(&mut out, module, prefix);

    // Enums (typedef + #define pattern)
    for e in &module.enums {
        emit_enum(&mut out, e, prefix);
    }

    // VarInt typedefs
    emit_varint_header(&mut out, module, prefix);

    // Forward declarations for all structs
    emit_forward_decls(&mut out, module, prefix);

    // Emit packets, frames, capsules in dependency order.
    // Types that reference other types must come after their dependencies.
    emit_types_in_dependency_order(&mut out, module, prefix);

    // State machines
    for sm in &module.state_machines {
        emit_state_machine_header_imp(&mut out, sm, prefix, &import_prefixes);
    }

    // Function declarations
    out.push_str("/* ---- Function declarations ---- */\n\n");

    // VarInt function declarations
    for vi in &module.varints {
        emit_varint_func_decls(&mut out, vi, prefix);
    }

    for packet in &module.packets {
        emit_func_decls(&mut out, &packet.name, prefix);
    }
    for frame in &module.frames {
        emit_func_decls(&mut out, &frame.name, prefix);
    }
    for capsule in &module.capsules {
        emit_func_decls(&mut out, &capsule.name, prefix);
    }

    out.push_str(&format!("#endif /* {guard} */\n"));
    out
}

/// Categorized type definitions for dependency ordering.
enum TypeDef<'a> {
    Packet(&'a CodecPacket),
    Frame(&'a CodecFrame),
    Capsule(&'a CodecCapsule),
}

/// Collect type names referenced by a field list (struct, array element types).
fn collect_field_type_refs(fields: &[CodecField], refs: &mut std::collections::HashSet<String>) {
    for f in fields {
        if let Some(ref name) = f.ref_type_name {
            refs.insert(name.clone());
        }
        if let Some(ref arr) = f.array_spec
            && let Some(ref name) = arr.element_ref_type_name
        {
            refs.insert(name.clone());
        }
    }
}

/// Emit packets, frames, and capsules in topologically sorted order so that
/// types used as array elements or struct fields are defined before their users.
fn emit_types_in_dependency_order(out: &mut String, module: &CodecModule, prefix: &str) {
    use std::collections::{HashMap, HashSet, VecDeque};

    // Collect all types with their names and dependencies
    let mut types: Vec<(String, TypeDef)> = Vec::new();
    let mut deps: HashMap<String, HashSet<String>> = HashMap::new();
    let mut all_names: HashSet<String> = HashSet::new();

    for p in &module.packets {
        all_names.insert(p.name.clone());
        let mut refs = HashSet::new();
        collect_field_type_refs(&p.fields, &mut refs);
        deps.insert(p.name.clone(), refs);
        types.push((p.name.clone(), TypeDef::Packet(p)));
    }
    for f in &module.frames {
        all_names.insert(f.name.clone());
        let mut refs = HashSet::new();
        for v in &f.variants {
            collect_field_type_refs(&v.fields, &mut refs);
        }
        deps.insert(f.name.clone(), refs);
        types.push((f.name.clone(), TypeDef::Frame(f)));
    }
    for c in &module.capsules {
        all_names.insert(c.name.clone());
        let mut refs = HashSet::new();
        collect_field_type_refs(&c.header_fields, &mut refs);
        for v in &c.variants {
            collect_field_type_refs(&v.fields, &mut refs);
        }
        deps.insert(c.name.clone(), refs);
        types.push((c.name.clone(), TypeDef::Capsule(c)));
    }

    // Filter dependencies to only include types in this module
    for d in deps.values_mut() {
        d.retain(|n| all_names.contains(n));
    }

    // Topological sort (Kahn's algorithm)
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    for (name, _) in &types {
        in_degree.insert(name.clone(), 0);
    }
    for d in deps.values() {
        for dep in d {
            if let Some(count) = in_degree.get_mut(dep) {
                // dep is depended on by someone, but that doesn't change in_degree
                let _ = count;
            }
        }
    }
    // in_degree[x] = number of types that x depends on (that are in this module)
    for (name, d) in &deps {
        *in_degree.get_mut(name).unwrap() = d.len();
    }

    let mut queue: VecDeque<String> = VecDeque::new();
    for (name, deg) in &in_degree {
        if *deg == 0 {
            queue.push_back(name.clone());
        }
    }

    let mut ordered: Vec<String> = Vec::new();
    while let Some(name) = queue.pop_front() {
        ordered.push(name.clone());
        // For each type that depends on `name`, decrease its in_degree
        for (other_name, other_deps) in &deps {
            if other_deps.contains(&name) {
                let deg = in_degree.get_mut(other_name).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(other_name.clone());
                }
            }
        }
    }

    // Append any remaining types (cycles or missing)
    for (name, _) in &types {
        if !ordered.contains(name) {
            ordered.push(name.clone());
        }
    }

    // Emit in order
    let type_map: HashMap<String, &TypeDef> = types.iter().map(|(n, t)| (n.clone(), t)).collect();
    for name in &ordered {
        if let Some(td) = type_map.get(name) {
            match td {
                TypeDef::Packet(p) => emit_packet_struct(out, p, prefix),
                TypeDef::Frame(f) => emit_frame(out, f, prefix),
                TypeDef::Capsule(c) => emit_capsule_struct(out, c, prefix),
            }
        }
    }
}

fn emit_forward_decls(out: &mut String, module: &CodecModule, prefix: &str) {
    for packet in &module.packets {
        let tname = c_type_name(prefix, &packet.name);
        let sname = tname.trim_end_matches("_t");
        out.push_str(&format!("typedef struct {sname} {tname};\n"));
    }
    for frame in &module.frames {
        let tname = c_type_name(prefix, &frame.name);
        let sname = tname.trim_end_matches("_t");
        out.push_str(&format!("typedef struct {sname} {tname};\n"));
    }
    for capsule in &module.capsules {
        let tname = c_type_name(prefix, &capsule.name);
        let sname = tname.trim_end_matches("_t");
        out.push_str(&format!("typedef struct {sname} {tname};\n"));
    }
    out.push('\n');
}

/// Emit import #include directives.
fn emit_import_includes(out: &mut String, module: &CodecModule) {
    let mut seen = std::collections::HashSet::new();
    for imp in &module.imports {
        if seen.insert(&imp.source_prefix) {
            out.push_str(&format!("#include \"{}.h\"\n", imp.source_prefix));
        }
    }
}

/// Emit const #define directives.
fn emit_const_defines(out: &mut String, module: &CodecModule, prefix: &str) {
    for c in &module.consts {
        let ctype = semantic_type_to_c(&c.ty, None::<&str>);
        let val_str = semantic_literal_to_c(&c.value);
        let name_upper = to_snake_case(&c.name).to_uppercase();
        out.push_str(&format!(
            "#define {}_{} (({ctype}){val_str})\n",
            prefix.to_uppercase(),
            name_upper,
        ));
    }
    if !module.consts.is_empty() {
        out.push('\n');
    }
}

/// Map SemanticType to C type string.
/// For simple primitives, `prefix` is unused. For PacketRef/EnumRef/FrameRef etc,
/// `prefix` is prepended (pass "" if not needed).
/// `import_prefixes` maps imported type names to their source prefix, so
/// imported types use the correct prefix from the defining module.
fn semantic_type_to_c_imp(
    ty: &wirespec_sema::types::SemanticType,
    prefix: &str,
    import_prefixes: &std::collections::HashMap<String, String>,
) -> String {
    use wirespec_sema::types::{PrimitiveWireType, SemanticType};
    match ty {
        SemanticType::Primitive { wire, .. } => match wire {
            PrimitiveWireType::U8 => "uint8_t".into(),
            PrimitiveWireType::U16 => "uint16_t".into(),
            PrimitiveWireType::U24 => "uint32_t".into(),
            PrimitiveWireType::U32 => "uint32_t".into(),
            PrimitiveWireType::U64 => "uint64_t".into(),
            PrimitiveWireType::I8 => "int8_t".into(),
            PrimitiveWireType::I16 => "int16_t".into(),
            PrimitiveWireType::I32 => "int32_t".into(),
            PrimitiveWireType::I64 => "int64_t".into(),
            PrimitiveWireType::Bool => "bool".into(),
            PrimitiveWireType::Bit => "uint8_t".into(),
        },
        SemanticType::Bits { width_bits } => {
            if *width_bits <= 8 {
                "uint8_t".into()
            } else if *width_bits <= 16 {
                "uint16_t".into()
            } else if *width_bits <= 32 {
                "uint32_t".into()
            } else {
                "uint64_t".into()
            }
        }
        SemanticType::VarIntRef { name, .. } => {
            if prefix.is_empty() {
                "uint64_t".into()
            } else {
                let effective_prefix = import_prefixes
                    .get(name)
                    .map(|s| s.as_str())
                    .unwrap_or(prefix);
                c_type_name(effective_prefix, name)
            }
        }
        SemanticType::PacketRef { name, .. }
        | SemanticType::EnumRef { name, .. }
        | SemanticType::FrameRef { name, .. }
        | SemanticType::CapsuleRef { name, .. } => {
            if prefix.is_empty() {
                "uint32_t".into()
            } else {
                let effective_prefix = import_prefixes
                    .get(name)
                    .map(|s| s.as_str())
                    .unwrap_or(prefix);
                c_type_name(effective_prefix, name)
            }
        }
        SemanticType::Bytes { .. } => "wirespec_bytes_t".into(),
        SemanticType::Array { element_type, .. } => {
            // For arrays, return the element type
            semantic_type_to_c_imp(element_type, prefix, import_prefixes)
        }
    }
}

/// Convenience wrapper: no import prefixes.
fn semantic_type_to_c<'a>(
    ty: &wirespec_sema::types::SemanticType,
    _prefix: impl Into<Option<&'a str>>,
) -> String {
    let prefix = _prefix.into().unwrap_or("");
    semantic_type_to_c_imp(ty, prefix, &std::collections::HashMap::new())
}

/// Map SemanticLiteral to a C literal string.
fn semantic_literal_to_c(lit: &wirespec_sema::expr::SemanticLiteral) -> String {
    use wirespec_sema::expr::SemanticLiteral;
    match lit {
        SemanticLiteral::Int(v) => format!("{v}"),
        SemanticLiteral::Bool(b) => {
            if *b {
                "1".into()
            } else {
                "0".into()
            }
        }
        SemanticLiteral::String(s) => format!("\"{s}\""),
        SemanticLiteral::Null => "0".into(),
    }
}

fn emit_enum(out: &mut String, e: &wirespec_sema::ir::SemanticEnum, prefix: &str) {
    let tname = c_type_name(prefix, &e.name);
    let underlying_ctype = semantic_type_to_c(&e.underlying_type, None::<&str>);
    // Use typedef + #define pattern instead of C enum
    out.push_str(&format!("typedef {underlying_ctype} {tname};\n"));
    for member in &e.members {
        let mname = c_enum_member(prefix, &e.name, &member.name);
        out.push_str(&format!(
            "#define {mname} (({tname}){val})\n",
            val = member.value,
        ));
    }
    out.push('\n');
}

/// Emit VarInt typedefs and parse_cursor declarations.
fn emit_varint_header(out: &mut String, module: &CodecModule, prefix: &str) {
    for vi in &module.varints {
        let snake = to_snake_case(&vi.name);
        let tname = format!("{prefix}_{snake}_t");
        out.push_str(&format!("typedef uint64_t {tname};\n\n"));
    }
}

/// Emit VarInt function declarations.
fn emit_varint_func_decls(out: &mut String, vi: &wirespec_sema::ir::SemanticVarInt, prefix: &str) {
    let snake = to_snake_case(&vi.name);
    let tname = format!("{prefix}_{snake}_t");
    let parse_fn = format!("{prefix}_{snake}_parse");
    let serialize_fn = format!("{prefix}_{snake}_serialize");
    let wire_size_fn = format!("{prefix}_{snake}_wire_size");

    out.push_str(&format!(
        "wirespec_result_t {parse_fn}(\n    const uint8_t *buf, size_t len,\n    {tname} *out, size_t *consumed);\n"
    ));
    out.push_str(&format!(
        "wirespec_result_t {serialize_fn}(\n    {tname} val,\n    uint8_t *buf, size_t cap, size_t *written);\n"
    ));
    out.push_str(&format!("size_t {wire_size_fn}({tname} val);\n"));
    out.push('\n');
}

fn emit_packet_struct(out: &mut String, packet: &CodecPacket, prefix: &str) {
    let tname = c_type_name(prefix, &packet.name);
    let sname = tname.trim_end_matches("_t");
    out.push_str(&format!("struct {sname} {{\n"));
    emit_struct_fields(out, &packet.fields, &packet.items, prefix);
    out.push_str("};\n\n");
}

fn emit_struct_fields(out: &mut String, fields: &[CodecField], items: &[CodecItem], prefix: &str) {
    // Emit fields in item order so derived fields appear where declared
    for item in items {
        match item {
            CodecItem::Field { field_id } => {
                if let Some(f) = fields.iter().find(|f| &f.field_id == field_id) {
                    emit_single_field(out, f, prefix);
                }
            }
            CodecItem::Derived(d) => {
                let ctype = wire_type_to_c(&d.wire_type, prefix);
                out.push_str(&format!("    {ctype} {};\n", d.name));
            }
            CodecItem::Require(_) => {
                // require clauses don't produce struct fields
            }
        }
    }
}

fn emit_single_field(out: &mut String, f: &CodecField, prefix: &str) {
    match f.strategy {
        FieldStrategy::BitGroup => {
            // Bitgroup members get their own field in the struct
            let ctype = wire_type_to_c(&f.wire_type, prefix);
            out.push_str(&format!("    {ctype} {};\n", f.name));
        }
        FieldStrategy::Conditional => {
            // Optional field: has_X bool + the actual field
            out.push_str(&format!("    bool has_{};\n", f.name));
            if let Some(ref inner) = f.inner_wire_type {
                let ctype = wire_type_to_c(inner, prefix);
                out.push_str(&format!("    {ctype} {};\n", f.name));
            } else {
                let ctype = wire_type_to_c(&f.wire_type, prefix);
                out.push_str(&format!("    {ctype} {};\n", f.name));
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                let elem_type = wire_type_to_c(&arr.element_wire_type, prefix);
                let max_elems = f.max_elements.unwrap_or(MAX_ARRAY_ELEMENTS);
                out.push_str(&format!("    {elem_type} {}[{max_elems}];\n", f.name));
                out.push_str(&format!("    uint32_t {}_count;\n", f.name));
            }
        }
        FieldStrategy::BytesFixed
        | FieldStrategy::BytesLength
        | FieldStrategy::BytesRemaining
        | FieldStrategy::BytesLor => {
            out.push_str(&format!("    wirespec_bytes_t {};\n", f.name));
        }
        FieldStrategy::Struct => {
            if let Some(ref name) = f.ref_type_name {
                let ctype = c_type_name(prefix, name);
                out.push_str(&format!("    {ctype} {};\n", f.name));
            } else {
                let ctype = wire_type_to_c(&f.wire_type, prefix);
                out.push_str(&format!("    {ctype} {};\n", f.name));
            }
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            // VarInt stored as uint64_t
            out.push_str(&format!("    uint64_t {};\n", f.name));
        }
        _ => {
            // Primitive / Checksum
            let ctype = wire_type_to_c(&f.wire_type, prefix);
            out.push_str(&format!("    {ctype} {};\n", f.name));
        }
    }
}

fn emit_frame(out: &mut String, frame: &CodecFrame, prefix: &str) {
    // Tag enum
    let tag_type = c_frame_tag_type(prefix, &frame.name);
    out.push_str("typedef enum {\n");
    for variant in &frame.variants {
        let tag_val = c_frame_tag_value(prefix, &frame.name, &variant.name);
        out.push_str(&format!("    {tag_val} = {},\n", variant.ordinal));
    }
    out.push_str(&format!("}} {tag_type};\n\n"));

    // Frame struct with union
    let tname = c_type_name(prefix, &frame.name);
    let sname = tname.trim_end_matches("_t");
    let raw_tag_ctype = wire_type_to_c(&frame.tag.wire_type, prefix);
    out.push_str(&format!("struct {sname} {{\n"));
    out.push_str(&format!("    {raw_tag_ctype} frame_type;\n"));
    out.push_str(&format!("    {tag_type} tag;\n"));
    out.push_str("    union {\n");
    for variant in &frame.variants {
        let vname = to_snake_case(&variant.name);
        if variant.fields.is_empty() {
            // Empty variant still needs a placeholder for the union
            out.push_str(&format!("        struct {{ uint8_t _dummy; }} {vname};\n"));
        } else {
            out.push_str("        struct {\n");
            for f in &variant.fields {
                emit_variant_field(out, f, prefix, "            ");
            }
            // Also emit derived fields
            for item in &variant.items {
                if let CodecItem::Derived(d) = item {
                    let ctype = wire_type_to_c(&d.wire_type, prefix);
                    out.push_str(&format!("            {ctype} {};\n", d.name));
                }
            }
            out.push_str(&format!("        }} {vname};\n"));
        }
    }
    out.push_str("    } value;\n");
    out.push_str("};\n\n");
}

fn emit_variant_field(out: &mut String, f: &CodecField, prefix: &str, indent: &str) {
    match f.strategy {
        FieldStrategy::Conditional => {
            out.push_str(&format!("{indent}bool has_{};\n", f.name));
            if let Some(ref inner) = f.inner_wire_type {
                let ctype = wire_type_to_c(inner, prefix);
                out.push_str(&format!("{indent}{ctype} {};\n", f.name));
            } else {
                let ctype = wire_type_to_c(&f.wire_type, prefix);
                out.push_str(&format!("{indent}{ctype} {};\n", f.name));
            }
        }
        FieldStrategy::Array => {
            if let Some(ref arr) = f.array_spec {
                let elem_type = wire_type_to_c(&arr.element_wire_type, prefix);
                let max_elems = f.max_elements.unwrap_or(MAX_ARRAY_ELEMENTS);
                out.push_str(&format!("{indent}{elem_type} {}[{max_elems}];\n", f.name));
                out.push_str(&format!("{indent}uint32_t {}_count;\n", f.name));
            }
        }
        FieldStrategy::BytesFixed
        | FieldStrategy::BytesLength
        | FieldStrategy::BytesRemaining
        | FieldStrategy::BytesLor => {
            out.push_str(&format!("{indent}wirespec_bytes_t {};\n", f.name));
        }
        FieldStrategy::Struct => {
            if let Some(ref name) = f.ref_type_name {
                let ctype = c_type_name(prefix, name);
                out.push_str(&format!("{indent}{ctype} {};\n", f.name));
            } else {
                let ctype = wire_type_to_c(&f.wire_type, prefix);
                out.push_str(&format!("{indent}{ctype} {};\n", f.name));
            }
        }
        FieldStrategy::VarInt | FieldStrategy::ContVarInt => {
            out.push_str(&format!("{indent}uint64_t {};\n", f.name));
        }
        FieldStrategy::BitGroup => {
            let ctype = wire_type_to_c(&f.wire_type, prefix);
            out.push_str(&format!("{indent}{ctype} {};\n", f.name));
        }
        _ => {
            let ctype = wire_type_to_c(&f.wire_type, prefix);
            out.push_str(&format!("{indent}{ctype} {};\n", f.name));
        }
    }
}

fn emit_capsule_struct(out: &mut String, capsule: &CodecCapsule, prefix: &str) {
    // Tag enum for capsule payload variants
    let tag_type = c_frame_tag_type(prefix, &capsule.name);
    out.push_str("typedef enum {\n");
    for variant in &capsule.variants {
        let tag_val = c_frame_tag_value(prefix, &capsule.name, &variant.name);
        out.push_str(&format!("    {tag_val} = {},\n", variant.ordinal));
    }
    out.push_str(&format!("}} {tag_type};\n\n"));

    // Capsule struct: header fields + tag + union
    let tname = c_type_name(prefix, &capsule.name);
    let sname = tname.trim_end_matches("_t");
    out.push_str(&format!("struct {sname} {{\n"));

    // Header fields
    for f in &capsule.header_fields {
        emit_single_field(out, f, prefix);
    }
    // Derived in header
    for item in &capsule.header_items {
        if let CodecItem::Derived(d) = item {
            let ctype = wire_type_to_c(&d.wire_type, prefix);
            out.push_str(&format!("    {ctype} {};\n", d.name));
        }
    }

    // Payload tag + union
    out.push_str(&format!("    {tag_type} tag;\n"));
    out.push_str("    union {\n");
    for variant in &capsule.variants {
        let vname = to_snake_case(&variant.name);
        if variant.fields.is_empty() {
            out.push_str(&format!("        struct {{ uint8_t _dummy; }} {vname};\n"));
        } else {
            out.push_str("        struct {\n");
            for f in &variant.fields {
                emit_variant_field(out, f, prefix, "            ");
            }
            for item in &variant.items {
                if let CodecItem::Derived(d) = item {
                    let ctype = wire_type_to_c(&d.wire_type, prefix);
                    out.push_str(&format!("            {ctype} {};\n", d.name));
                }
            }
            out.push_str(&format!("        }} {vname};\n"));
        }
    }
    out.push_str("    } value;\n");
    out.push_str("};\n\n");
}

fn emit_func_decls(out: &mut String, name: &str, prefix: &str) {
    let tname = c_type_name(prefix, name);
    let parse_fn = c_func_name(prefix, name, "parse");
    let serialize_fn = c_func_name(prefix, name, "serialize");
    let len_fn = c_func_name(prefix, name, "serialized_len");

    out.push_str(&format!(
        "wirespec_result_t {parse_fn}(const uint8_t *buf, size_t len, {tname} *out, size_t *consumed);\n"
    ));
    out.push_str(&format!(
        "wirespec_result_t {serialize_fn}(const {tname} *val, uint8_t *buf, size_t cap, size_t *written);\n"
    ));
    out.push_str(&format!("size_t {len_fn}(const {tname} *val);\n"));
    out.push('\n');
}

// ── State machine header emission ──

/// Emit complete header declarations for a state machine:
/// state tag enum, state data struct (tagged union), event tag enum,
/// event data struct (tagged union), dispatch declaration, init helper.
fn _emit_state_machine_header(out: &mut String, sm: &SemanticStateMachine, prefix: &str) {
    emit_state_machine_header_imp(out, sm, prefix, &std::collections::HashMap::new());
}

fn emit_state_machine_header_imp(
    out: &mut String,
    sm: &SemanticStateMachine,
    prefix: &str,
    import_prefixes: &std::collections::HashMap<String, String>,
) {
    let sm_snake = to_snake_case(&sm.name);
    let sm_upper = sm_snake.to_uppercase();
    let prefix_upper = prefix.to_uppercase();

    out.push_str(&format!("/* ---- State machine: {} ---- */\n\n", sm.name));

    // 1. State tag enum
    emit_sm_state_tag_enum(out, sm, prefix, &sm_snake, &sm_upper, &prefix_upper);

    // 2. State data struct (tagged union)
    emit_sm_state_struct_imp(
        out,
        sm,
        prefix,
        &sm_snake,
        &sm_upper,
        &prefix_upper,
        import_prefixes,
    );

    // 3. Event tag enum
    emit_sm_event_tag_enum(out, sm, prefix, &sm_snake, &sm_upper, &prefix_upper);

    // 4. Event data struct (tagged union)
    emit_sm_event_struct_imp(
        out,
        sm,
        prefix,
        &sm_snake,
        &sm_upper,
        &prefix_upper,
        import_prefixes,
    );

    // 5. Dispatch function declaration
    let sm_type = format!("{prefix}_{sm_snake}_t");
    let event_type = format!("{prefix}_{sm_snake}_event_t");
    let dispatch_fn = format!("{prefix}_{sm_snake}_dispatch");
    out.push_str(&format!(
        "wirespec_result_t {dispatch_fn}(\n    {sm_type} *sm,\n    const {event_type} *event);\n\n"
    ));

    // 6. Init helper (static inline)
    emit_sm_init_helper(out, sm, prefix, &sm_snake, &sm_upper, &prefix_upper);
}

fn emit_sm_state_tag_enum(
    out: &mut String,
    sm: &SemanticStateMachine,
    prefix: &str,
    sm_snake: &str,
    sm_upper: &str,
    prefix_upper: &str,
) {
    let tag_type = format!("{prefix}_{sm_snake}_state_tag_t");

    out.push_str("typedef enum {\n");
    for state in &sm.states {
        let state_upper = to_snake_case(&state.name).to_uppercase();
        let comma = ",";
        out.push_str(&format!(
            "    {prefix_upper}_{sm_upper}_{state_upper}{comma}\n"
        ));
    }
    out.push_str(&format!("}} {tag_type};\n\n"));
}

fn _emit_sm_state_struct(
    out: &mut String,
    sm: &SemanticStateMachine,
    prefix: &str,
    sm_snake: &str,
    sm_upper: &str,
    prefix_upper: &str,
) {
    emit_sm_state_struct_imp(
        out,
        sm,
        prefix,
        sm_snake,
        sm_upper,
        prefix_upper,
        &std::collections::HashMap::new(),
    );
}

fn emit_sm_state_struct_imp(
    out: &mut String,
    sm: &SemanticStateMachine,
    prefix: &str,
    sm_snake: &str,
    _sm_upper: &str,
    _prefix_upper: &str,
    import_prefixes: &std::collections::HashMap<String, String>,
) {
    let tname = format!("{prefix}_{sm_snake}_t");
    let sname = format!("{prefix}_{sm_snake}");
    let tag_type = format!("{prefix}_{sm_snake}_state_tag_t");

    // Forward declare
    out.push_str(&format!("typedef struct {sname} {tname};\n"));
    out.push_str(&format!("struct {sname} {{\n"));
    out.push_str(&format!("    {tag_type} tag;\n"));
    out.push_str("    union {\n");

    for state in &sm.states {
        let state_snake = to_snake_case(&state.name);
        if state.fields.is_empty() {
            // Empty state: use _unused placeholder to avoid empty struct
            out.push_str(&format!(
                "        struct {{ uint8_t _unused; }} {state_snake};\n"
            ));
        } else {
            out.push_str("        struct {\n");
            for field in &state.fields {
                emit_sm_state_field_imp(out, field, prefix, "            ", import_prefixes);
            }
            out.push_str(&format!("        }} {state_snake};\n"));
        }
    }

    out.push_str("    };\n");
    out.push_str("};\n\n");
}

fn _emit_sm_state_field(out: &mut String, field: &SemanticStateField, prefix: &str, indent: &str) {
    emit_sm_state_field_imp(
        out,
        field,
        prefix,
        indent,
        &std::collections::HashMap::new(),
    )
}

fn emit_sm_state_field_imp(
    out: &mut String,
    field: &SemanticStateField,
    prefix: &str,
    indent: &str,
    import_prefixes: &std::collections::HashMap<String, String>,
) {
    let ctype = semantic_type_to_c_imp(&field.ty, prefix, import_prefixes);
    // Handle array types specially
    if let wirespec_sema::types::SemanticType::Array { count_expr, .. } = &field.ty {
        // For fixed-size arrays in state fields
        if let Some(count) = count_expr
            && let wirespec_sema::expr::SemanticExpr::Literal {
                value: wirespec_sema::expr::SemanticLiteral::Int(n),
            } = count.as_ref()
        {
            out.push_str(&format!("{indent}{ctype} {}[{n}];\n", field.name));
            return;
        }
        // Variable-length array: use a reasonable max
        out.push_str(&format!("{indent}{ctype} {}[256];\n", field.name));
        out.push_str(&format!("{indent}uint32_t {}_count;\n", field.name));
        return;
    }
    out.push_str(&format!("{indent}{ctype} {};\n", field.name));
}

fn emit_sm_event_tag_enum(
    out: &mut String,
    sm: &SemanticStateMachine,
    prefix: &str,
    sm_snake: &str,
    sm_upper: &str,
    prefix_upper: &str,
) {
    let tag_type = format!("{prefix}_{sm_snake}_event_tag_t");

    out.push_str("typedef enum {\n");
    for event in &sm.events {
        let event_upper = to_snake_case(&event.name).to_uppercase();
        let comma = ",";
        out.push_str(&format!(
            "    {prefix_upper}_{sm_upper}_EVENT_{event_upper}{comma}\n"
        ));
    }
    out.push_str(&format!("}} {tag_type};\n\n"));
}

fn _emit_sm_event_struct(
    out: &mut String,
    sm: &SemanticStateMachine,
    prefix: &str,
    sm_snake: &str,
    sm_upper: &str,
    prefix_upper: &str,
) {
    emit_sm_event_struct_imp(
        out,
        sm,
        prefix,
        sm_snake,
        sm_upper,
        prefix_upper,
        &std::collections::HashMap::new(),
    );
}

fn emit_sm_event_struct_imp(
    out: &mut String,
    sm: &SemanticStateMachine,
    prefix: &str,
    sm_snake: &str,
    _sm_upper: &str,
    _prefix_upper: &str,
    import_prefixes: &std::collections::HashMap<String, String>,
) {
    let tname = format!("{prefix}_{sm_snake}_event_t");
    let tag_type = format!("{prefix}_{sm_snake}_event_tag_t");

    out.push_str("typedef struct {\n");
    out.push_str(&format!("    {tag_type} tag;\n"));
    out.push_str("    union {\n");

    for event in &sm.events {
        let event_snake = to_snake_case(&event.name);
        if event.params.is_empty() {
            out.push_str(&format!(
                "        struct {{ uint8_t _unused; }} {event_snake};\n"
            ));
        } else {
            out.push_str("        struct {\n");
            for param in &event.params {
                let ctype = semantic_type_to_c_imp(&param.ty, prefix, import_prefixes);
                out.push_str(&format!("            {ctype} {};\n", param.name));
            }
            out.push_str(&format!("        }} {event_snake};\n"));
        }
    }

    out.push_str("    };\n");
    out.push_str(&format!("}} {tname};\n\n"));
}

fn emit_sm_init_helper(
    out: &mut String,
    sm: &SemanticStateMachine,
    prefix: &str,
    sm_snake: &str,
    sm_upper: &str,
    prefix_upper: &str,
) {
    // Find the initial state
    let initial_state = sm.states.iter().find(|s| s.state_id == sm.initial_state_id);

    let initial_state = match initial_state {
        Some(s) => s,
        None => return, // no initial state found, skip init helper
    };

    let initial_upper = to_snake_case(&initial_state.name).to_uppercase();
    let initial_snake = to_snake_case(&initial_state.name);
    let sm_type = format!("{prefix}_{sm_snake}_t");

    let init_fn = format!("{prefix}_{sm_snake}_init");

    out.push_str(&format!("static inline void {init_fn}({sm_type} *sm) {{\n"));
    out.push_str(&format!(
        "    sm->tag = {prefix_upper}_{sm_upper}_{initial_upper};\n"
    ));

    // Set default values for initial state fields
    for field in &initial_state.fields {
        if let Some(ref default) = field.default_value {
            let val = semantic_literal_to_c(default);
            out.push_str(&format!(
                "    sm->{initial_snake}.{} = {val};\n",
                field.name
            ));
        }
    }

    out.push_str("}\n\n");
}
