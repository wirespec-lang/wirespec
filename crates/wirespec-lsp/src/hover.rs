use tower_lsp::lsp_types::*;

use crate::position::position_to_offset;
use wirespec_sema::ir::*;
use wirespec_sema::types::*;

// ── Public entry point ──

pub fn compute_hover(source: &str, position: Position) -> Option<Hover> {
    let offset = position_to_offset(source, position);
    let (_, _, word) = crate::position::word_at_offset(source, offset);
    if word.is_empty() {
        return None;
    }

    let ast = wirespec_syntax::parse(source).ok()?;
    let module = wirespec_sema::analyze(
        &ast,
        wirespec_sema::ComplianceProfile::default(),
        &Default::default(),
    )
    .ok()?;

    let markdown = hover_for_word(word, &module)?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: None,
    })
}

// ── Look up a word in the module IR ──

fn hover_for_word(word: &str, module: &SemanticModule) -> Option<String> {
    // Packet
    if let Some(p) = module.packets.iter().find(|p| p.name == word) {
        return Some(format_packet(p));
    }

    // Enum / Flags
    if let Some(e) = module.enums.iter().find(|e| e.name == word) {
        return Some(format_enum(e));
    }

    // Const
    if let Some(c) = module.consts.iter().find(|c| c.name == word) {
        return Some(format_const(c));
    }

    // Frame
    if let Some(f) = module.frames.iter().find(|f| f.name == word) {
        return Some(format_frame(f));
    }

    // Capsule
    if let Some(c) = module.capsules.iter().find(|c| c.name == word) {
        return Some(format_capsule(c));
    }

    // VarInt
    if let Some(v) = module.varints.iter().find(|v| v.name == word) {
        return Some(format_varint(v));
    }

    // Field name — search inside packets, frames, capsules
    if let Some(md) = find_field_hover(word, module) {
        return Some(md);
    }

    None
}

// ── Formatters ──

fn format_packet(p: &SemanticPacket) -> String {
    let mut lines = Vec::new();
    lines.push(format!("packet {} {{", p.name));
    for field in &p.fields {
        lines.push(format!("    {}: {}", field.name, format_type(&field.ty)));
    }
    lines.push("}".to_owned());
    let body = lines.join("\n");
    format!("```wirespec\n{}\n```", body)
}

fn format_enum(e: &SemanticEnum) -> String {
    let kw = if e.is_flags { "flags" } else { "enum" };
    let underlying = format_type(&e.underlying_type);
    let mut lines = Vec::new();
    lines.push(format!("{} {}: {} {{", kw, e.name, underlying));
    for m in &e.members {
        lines.push(format!("    {} = {}", m.name, m.value));
    }
    lines.push("}".to_owned());
    let body = lines.join("\n");
    format!("```wirespec\n{}\n```", body)
}

fn format_const(c: &SemanticConst) -> String {
    let ty = format_type(&c.ty);
    let val = format_literal(&c.value);
    format!("```wirespec\nconst {}: {} = {}\n```", c.name, ty, val)
}

fn format_frame(f: &SemanticFrame) -> String {
    format!("```wirespec\nframe {}\n```", f.name)
}

fn format_capsule(c: &SemanticCapsule) -> String {
    format!("```wirespec\ncapsule {}\n```", c.name)
}

fn format_varint(v: &SemanticVarInt) -> String {
    format!("```wirespec\ntype {} (varint)\n```", v.name)
}

fn find_field_hover(word: &str, module: &SemanticModule) -> Option<String> {
    // Packets
    for p in &module.packets {
        if let Some(f) = p.fields.iter().find(|f| f.name == word) {
            return Some(format!(
                "```wirespec\n{}: {}\n```",
                f.name,
                format_type(&f.ty)
            ));
        }
    }
    // Frame variants
    for fr in &module.frames {
        for variant in &fr.variants {
            if let Some(f) = variant.fields.iter().find(|f| f.name == word) {
                return Some(format!(
                    "```wirespec\n{}: {}\n```",
                    f.name,
                    format_type(&f.ty)
                ));
            }
        }
    }
    // Capsule header + variants
    for cap in &module.capsules {
        if let Some(f) = cap.header_fields.iter().find(|f| f.name == word) {
            return Some(format!(
                "```wirespec\n{}: {}\n```",
                f.name,
                format_type(&f.ty)
            ));
        }
        for variant in &cap.variants {
            if let Some(f) = variant.fields.iter().find(|f| f.name == word) {
                return Some(format!(
                    "```wirespec\n{}: {}\n```",
                    f.name,
                    format_type(&f.ty)
                ));
            }
        }
    }
    None
}

// ── Type / literal display ──

fn format_type(ty: &SemanticType) -> String {
    match ty {
        SemanticType::Primitive { wire, endianness } => {
            let base = match wire {
                PrimitiveWireType::U8 => "u8",
                PrimitiveWireType::U16 => "u16",
                PrimitiveWireType::U24 => "u24",
                PrimitiveWireType::U32 => "u32",
                PrimitiveWireType::U64 => "u64",
                PrimitiveWireType::I8 => "i8",
                PrimitiveWireType::I16 => "i16",
                PrimitiveWireType::I32 => "i32",
                PrimitiveWireType::I64 => "i64",
                PrimitiveWireType::Bool => "bool",
                PrimitiveWireType::Bit => "bit",
            };
            match endianness {
                Some(Endianness::Big) => format!("{} be", base),
                Some(Endianness::Little) => format!("{} le", base),
                None => base.to_owned(),
            }
        }
        SemanticType::Bits { width_bits } => format!("bits {}", width_bits),
        SemanticType::VarIntRef { name, .. } => name.clone(),
        SemanticType::Bytes {
            bytes_kind,
            fixed_size,
            ..
        } => match bytes_kind {
            SemanticBytesKind::Fixed => {
                if let Some(n) = fixed_size {
                    format!("bytes[{}]", n)
                } else {
                    "bytes".to_owned()
                }
            }
            SemanticBytesKind::Remaining => "bytes remaining".to_owned(),
            SemanticBytesKind::Length | SemanticBytesKind::LengthOrRemaining => "bytes".to_owned(),
        },
        SemanticType::Array { element_type, .. } => {
            format!("[{}]", format_type(element_type))
        }
        SemanticType::PacketRef { name, .. } => name.clone(),
        SemanticType::EnumRef { name, .. } => name.clone(),
        SemanticType::FrameRef { name, .. } => name.clone(),
        SemanticType::CapsuleRef { name, .. } => name.clone(),
    }
}

fn format_literal(lit: &wirespec_sema::expr::SemanticLiteral) -> String {
    use wirespec_sema::expr::SemanticLiteral;
    match lit {
        SemanticLiteral::Int(n) => n.to_string(),
        SemanticLiteral::Bool(b) => b.to_string(),
        SemanticLiteral::String(s) => format!("\"{}\"", s),
        SemanticLiteral::Null => "null".to_owned(),
    }
}
