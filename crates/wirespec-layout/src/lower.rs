// crates/wirespec-layout/src/lower.rs
use crate::bitgroup;
use crate::ir::*;
use wirespec_sema::ir::*;
use wirespec_sema::types::*;

#[derive(Debug)]
pub struct LayoutError {
    pub msg: String,
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "layout error: {}", self.msg)
    }
}

impl std::error::Error for LayoutError {}

pub fn lower(sem: &SemanticModule) -> Result<LayoutModule, LayoutError> {
    let mut packets = Vec::new();
    let mut frames = Vec::new();
    let mut capsules = Vec::new();

    for p in &sem.packets {
        packets.push(lower_packet(p, sem.module_endianness)?);
    }
    for f in &sem.frames {
        frames.push(lower_frame(f, sem.module_endianness)?);
    }
    for c in &sem.capsules {
        capsules.push(lower_capsule(c, sem.module_endianness)?);
    }

    Ok(LayoutModule {
        schema_version: "layout-ir/v1".to_string(),
        compliance_profile: sem.compliance_profile.clone(),
        module_name: sem.module_name.clone(),
        module_endianness: sem.module_endianness,
        imports: sem.imports.clone(),
        varints: sem.varints.clone(),
        consts: sem.consts.clone(),
        enums: sem.enums.clone(),
        packets,
        frames,
        capsules,
        state_machines: sem.state_machines.clone(),
    })
}

fn lower_packet(
    p: &SemanticPacket,
    module_endianness: Endianness,
) -> Result<LayoutPacket, LayoutError> {
    let fields = lower_fields(&p.fields, module_endianness);
    let (bitgroups, fields) = bitgroup::detect_bitgroups(&fields, &p.packet_id, module_endianness)
        .map_err(|e| LayoutError { msg: e.msg })?;

    Ok(LayoutPacket {
        packet_id: p.packet_id.clone(),
        name: p.name.clone(),
        derive_traits: p.derive_traits.clone(),
        fields,
        derived: p.derived.clone(),
        requires: p.requires.clone(),
        items: p.items.clone(),
        bitgroups,
        span: p.span,
    })
}

fn lower_frame(
    f: &SemanticFrame,
    module_endianness: Endianness,
) -> Result<LayoutFrame, LayoutError> {
    let tag_endianness = resolve_type_endianness(&f.tag_type, module_endianness);
    let mut variants = Vec::new();
    for v in &f.variants {
        variants.push(lower_variant_scope(v, module_endianness)?);
    }

    Ok(LayoutFrame {
        frame_id: f.frame_id.clone(),
        name: f.name.clone(),
        derive_traits: f.derive_traits.clone(),
        tag_name: f.tag_name.clone(),
        tag_type: f.tag_type.clone(),
        tag_endianness,
        variants,
        span: f.span,
    })
}

fn lower_capsule(
    c: &SemanticCapsule,
    module_endianness: Endianness,
) -> Result<LayoutCapsule, LayoutError> {
    let header_fields = lower_fields(&c.header_fields, module_endianness);
    let header_scope_id = format!("{}#header", c.capsule_id);
    let (header_bitgroups, header_fields) =
        bitgroup::detect_bitgroups(&header_fields, &header_scope_id, module_endianness)
            .map_err(|e| LayoutError { msg: e.msg })?;

    let mut variants = Vec::new();
    for v in &c.variants {
        variants.push(lower_variant_scope(v, module_endianness)?);
    }

    Ok(LayoutCapsule {
        capsule_id: c.capsule_id.clone(),
        name: c.name.clone(),
        derive_traits: c.derive_traits.clone(),
        tag_type: c.tag_type.clone(),
        tag_selector: c.tag_selector.clone(),
        within_field_id: c.within_field_id.clone(),
        within_field_name: c.within_field_name.clone(),
        header_fields,
        header_derived: c.header_derived.clone(),
        header_requires: c.header_requires.clone(),
        header_items: c.header_items.clone(),
        header_bitgroups,
        variants,
        span: c.span,
    })
}

fn lower_variant_scope(
    v: &SemanticVariantScope,
    module_endianness: Endianness,
) -> Result<LayoutVariantScope, LayoutError> {
    let fields = lower_fields(&v.fields, module_endianness);
    let (bitgroups, fields) = bitgroup::detect_bitgroups(&fields, &v.scope_id, module_endianness)
        .map_err(|e| LayoutError { msg: e.msg })?;

    Ok(LayoutVariantScope {
        scope_id: v.scope_id.clone(),
        owner: v.owner.clone(),
        variant_name: v.variant_name.clone(),
        ordinal: v.ordinal,
        pattern: v.pattern.clone(),
        fields,
        derived: v.derived.clone(),
        requires: v.requires.clone(),
        items: v.items.clone(),
        bitgroups,
        span: v.span,
    })
}

fn lower_fields(sem_fields: &[SemanticField], module_endianness: Endianness) -> Vec<LayoutField> {
    sem_fields
        .iter()
        .map(|f| lower_field(f, module_endianness))
        .collect()
}

fn lower_field(f: &SemanticField, module_endianness: Endianness) -> LayoutField {
    let wire_width_bits = compute_wire_width(&f.ty);
    let endianness = resolve_field_endianness(&f.ty, module_endianness);

    LayoutField {
        field_id: f.field_id.clone(),
        name: f.name.clone(),
        ty: f.ty.clone(),
        presence: f.presence.clone(),
        max_elements: f.max_elements,
        checksum_algorithm: f.checksum_algorithm.clone(),
        wire_width_bits,
        endianness,
        bitgroup_member: None, // Set by bitgroup detection
        span: f.span,
    }
}

/// Compute wire width in bits for a type.
/// Returns None for dynamically-sized types (bytes, arrays, struct refs, varints).
fn compute_wire_width(ty: &SemanticType) -> Option<u16> {
    match ty {
        SemanticType::Primitive { wire, .. } => Some(match wire {
            PrimitiveWireType::U8 | PrimitiveWireType::I8 => 8,
            PrimitiveWireType::U16 | PrimitiveWireType::I16 => 16,
            PrimitiveWireType::U24 => 24,
            PrimitiveWireType::U32 | PrimitiveWireType::I32 => 32,
            PrimitiveWireType::U64 | PrimitiveWireType::I64 => 64,
            PrimitiveWireType::Bit => 1,
            PrimitiveWireType::Bool => return None, // bool is semantic, not wire
        }),
        SemanticType::Bits { width_bits } => Some(*width_bits),
        // Dynamic types
        SemanticType::VarIntRef { .. }
        | SemanticType::Bytes { .. }
        | SemanticType::Array { .. }
        | SemanticType::PacketRef { .. }
        | SemanticType::EnumRef { .. }
        | SemanticType::FrameRef { .. }
        | SemanticType::CapsuleRef { .. } => None,
    }
}

/// Resolve field endianness from type.
/// Returns Some for multi-byte byte-aligned primitives, None otherwise.
fn resolve_field_endianness(
    ty: &SemanticType,
    module_endianness: Endianness,
) -> Option<Endianness> {
    resolve_type_endianness(ty, module_endianness)
}

/// Resolve endianness for a type.
fn resolve_type_endianness(ty: &SemanticType, module_endianness: Endianness) -> Option<Endianness> {
    match ty {
        SemanticType::Primitive {
            endianness: Some(e),
            ..
        } => Some(*e),
        SemanticType::Primitive {
            endianness: None,
            wire,
        } => {
            match wire {
                PrimitiveWireType::U16
                | PrimitiveWireType::I16
                | PrimitiveWireType::U24
                | PrimitiveWireType::U32
                | PrimitiveWireType::I32
                | PrimitiveWireType::U64
                | PrimitiveWireType::I64 => Some(module_endianness),
                // Single-byte or sub-byte: no endianness
                _ => None,
            }
        }
        // EnumRef underlying type may need endianness, but that's resolved
        // when the enum's wire type is lowered
        _ => None,
    }
}
