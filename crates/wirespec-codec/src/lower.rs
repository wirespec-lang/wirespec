// crates/wirespec-codec/src/lower.rs
//
// Main lowering pass: LayoutModule -> CodecModule.
// Single-pass, scope-at-a-time. For each scope:
//   1. Flatten fields into CodecField (wire type, strategy, memory tier, etc.)
//   2. Build ordered CodecItem list from scope items
//   3. Synthesize ChecksumPlan if any checksum field exists

use crate::checksum::synthesize_checksum_plan;
use crate::ir::*;
use crate::strategy::*;
use wirespec_layout::ir::*;
use wirespec_sema::expr as sem_expr;
use wirespec_sema::ir::*;
use wirespec_sema::types::*;

// -- Error --

#[derive(Debug)]
pub struct CodecError {
    pub msg: String,
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "codec error: {}", self.msg)
    }
}

impl std::error::Error for CodecError {}

// -- Entry point --

pub fn lower(layout: &LayoutModule) -> Result<CodecModule, CodecError> {
    let ctx = LowerCtx {
        varints: &layout.varints,
    };

    let mut packets = Vec::new();
    for p in &layout.packets {
        packets.push(lower_packet(&ctx, p)?);
    }

    let mut frames = Vec::new();
    for f in &layout.frames {
        frames.push(lower_frame(&ctx, f)?);
    }

    let mut capsules = Vec::new();
    for c in &layout.capsules {
        capsules.push(lower_capsule(&ctx, c)?);
    }

    Ok(CodecModule {
        schema_version: "codec-ir/v1".to_string(),
        module_name: layout.module_name.clone(),
        module_endianness: layout.module_endianness,
        compliance_profile: layout.compliance_profile.clone(),
        imports: layout.imports.clone(),
        varints: layout.varints.clone(),
        consts: layout.consts.clone(),
        enums: layout.enums.clone(),
        state_machines: layout.state_machines.clone(),
        packets,
        frames,
        capsules,
    })
}

// -- Context --

struct LowerCtx<'a> {
    varints: &'a [SemanticVarInt],
}

impl LowerCtx<'_> {
    /// Look up a varint definition by varint_id, return its encoding.
    fn varint_encoding(&self, varint_id: &str) -> VarIntEncoding {
        for v in self.varints {
            if v.varint_id == varint_id {
                return v.encoding;
            }
        }
        // Default to PrefixMatch if not found
        VarIntEncoding::PrefixMatch
    }
}

// -- Packet --

fn lower_packet(ctx: &LowerCtx, p: &LayoutPacket) -> Result<CodecPacket, CodecError> {
    let scope_id = p.packet_id.clone();
    let fields = lower_fields(ctx, &p.fields, &scope_id, &p.bitgroups)?;
    let items = lower_items(&p.items, &p.derived, &p.requires, ctx)?;
    let checksum_plan = synthesize_checksum_plan(&fields, ScopeKind::Packet, &p.name);

    Ok(CodecPacket {
        scope_id,
        name: p.name.clone(),
        fields,
        items,
        checksum_plan,
    })
}

// -- Frame --

fn lower_frame(ctx: &LowerCtx, f: &LayoutFrame) -> Result<CodecFrame, CodecError> {
    let ref_type_name = match &f.tag_type {
        SemanticType::VarIntRef { name, .. } => Some(name.clone()),
        SemanticType::PacketRef { name, .. } => Some(name.clone()),
        SemanticType::EnumRef { name, .. } => Some(name.clone()),
        _ => None,
    };
    let tag = CodecTag {
        field_name: f.tag_name.clone(),
        wire_type: semantic_type_to_wire_type(ctx, &f.tag_type),
        endianness: f.tag_endianness,
        ref_type_name,
    };

    let mut variants = Vec::new();
    for v in &f.variants {
        variants.push(lower_variant_scope(ctx, v, ScopeKind::FrameVariant)?);
    }

    Ok(CodecFrame {
        frame_id: f.frame_id.clone(),
        name: f.name.clone(),
        tag,
        variants,
    })
}

// -- Capsule --

fn lower_capsule(ctx: &LowerCtx, c: &LayoutCapsule) -> Result<CodecCapsule, CodecError> {
    let ref_type_name = match &c.tag_type {
        SemanticType::VarIntRef { name, .. } => Some(name.clone()),
        SemanticType::PacketRef { name, .. } => Some(name.clone()),
        SemanticType::EnumRef { name, .. } => Some(name.clone()),
        _ => None,
    };
    let tag = CodecTag {
        field_name: capsule_tag_field_name(&c.tag_selector),
        wire_type: semantic_type_to_wire_type(ctx, &c.tag_type),
        endianness: None,
        ref_type_name,
    };

    let tag_expr = match &c.tag_selector {
        CapsuleTagSelector::Expr { expr } => Some(convert_semantic_expr(expr)),
        CapsuleTagSelector::Field { .. } => None,
    };

    let header_scope_id = format!("{}#header", c.capsule_id);
    let header_fields = lower_fields(ctx, &c.header_fields, &header_scope_id, &c.header_bitgroups)?;
    let header_items = lower_items(&c.header_items, &c.header_derived, &c.header_requires, ctx)?;
    let header_checksum_plan =
        synthesize_checksum_plan(&header_fields, ScopeKind::CapsuleHeader, &c.name);

    let mut variants = Vec::new();
    for v in &c.variants {
        variants.push(lower_variant_scope(
            ctx,
            v,
            ScopeKind::CapsulePayloadVariant,
        )?);
    }

    Ok(CodecCapsule {
        capsule_id: c.capsule_id.clone(),
        name: c.name.clone(),
        tag,
        within_field: c.within_field_name.clone(),
        tag_expr,
        header_fields,
        header_items,
        header_checksum_plan,
        variants,
    })
}

fn capsule_tag_field_name(selector: &CapsuleTagSelector) -> String {
    match selector {
        CapsuleTagSelector::Field { field_name, .. } => field_name.clone(),
        CapsuleTagSelector::Expr { .. } => "<expr>".to_string(),
    }
}

// -- Variant scope (shared by frame + capsule) --

fn lower_variant_scope(
    ctx: &LowerCtx,
    v: &LayoutVariantScope,
    scope_kind: ScopeKind,
) -> Result<CodecVariantScope, CodecError> {
    let fields = lower_fields(ctx, &v.fields, &v.scope_id, &v.bitgroups)?;
    let items = lower_items(&v.items, &v.derived, &v.requires, ctx)?;
    let scope_name = v.variant_name.clone();
    let checksum_plan = synthesize_checksum_plan(&fields, scope_kind, &scope_name);

    Ok(CodecVariantScope {
        scope_id: v.scope_id.clone(),
        owner: v.owner.clone(),
        ordinal: v.ordinal,
        name: v.variant_name.clone(),
        pattern: convert_pattern(&v.pattern),
        fields,
        items,
        checksum_plan,
    })
}

// -- Field lowering --

fn lower_fields(
    ctx: &LowerCtx,
    fields: &[LayoutField],
    scope_id: &str,
    bitgroups: &[LayoutBitGroup],
) -> Result<Vec<CodecField>, CodecError> {
    let mut result = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        result.push(lower_field(ctx, f, scope_id, i as u32, bitgroups)?);
    }
    Ok(result)
}

fn lower_field(
    ctx: &LowerCtx,
    f: &LayoutField,
    scope_id: &str,
    field_index: u32,
    bitgroups: &[LayoutBitGroup],
) -> Result<CodecField, CodecError> {
    let is_optional = matches!(f.presence, FieldPresence::Conditional { .. });
    let has_checksum = f.checksum_algorithm.is_some();
    let has_bitgroup = f.bitgroup_member.is_some();

    // Determine the "inner" type (for optional fields, the type inside the if)
    let inner_ty = extract_inner_type(&f.ty);

    // Wire type from the inner type
    let wire_type = semantic_type_to_wire_type(ctx, inner_ty);

    // Strategy assignment
    let strategy_input = StrategyInput {
        ty: inner_ty,
        is_optional,
        has_checksum,
        has_bitgroup,
    };
    let mut strategy = assign_strategy(&strategy_input);

    // For VarInt: check encoding to distinguish VarInt vs ContVarInt
    if strategy == FieldStrategy::VarInt
        && let SemanticType::VarIntRef { varint_id, .. } = inner_ty
        && ctx.varint_encoding(varint_id) == VarIntEncoding::ContinuationBit
    {
        strategy = FieldStrategy::ContVarInt;
    }

    // Memory tier
    let memory_tier = if strategy == FieldStrategy::Array {
        if let SemanticType::Array { element_type, .. } = inner_ty {
            Some(assign_array_memory_tier(element_type))
        } else {
            assign_memory_tier(strategy)
        }
    } else {
        assign_memory_tier(strategy)
    };

    // Condition expression (for optional fields)
    let condition = match &f.presence {
        FieldPresence::Conditional { condition } => Some(convert_semantic_expr(condition)),
        FieldPresence::Always => None,
    };

    // Inner wire type (only for optional fields)
    let inner_wire_type = if is_optional {
        Some(wire_type.clone())
    } else {
        None
    };

    // Ref type name
    let ref_type_name = extract_ref_type_name(inner_ty);

    // Bit width
    let bit_width = f.wire_width_bits;

    // Bytes spec
    let bytes_spec = extract_bytes_spec(inner_ty);

    // Array spec
    let array_spec = extract_array_spec(ctx, inner_ty);

    // Bitgroup member
    let bitgroup_member = extract_bitgroup_member(f, bitgroups);

    // Field ID
    let field_id = if f.field_id.is_empty() {
        format!("{scope_id}.field[{field_index}]")
    } else {
        f.field_id.clone()
    };

    Ok(CodecField {
        field_id,
        scope_id: scope_id.to_string(),
        field_index,
        name: f.name.clone(),
        wire_type,
        strategy,
        memory_tier,
        endianness: f.endianness,
        is_optional,
        inner_wire_type,
        condition,
        ref_type_name,
        bit_width,
        bytes_spec,
        array_spec,
        bitgroup_member,
        max_elements: f.max_elements,
        checksum_algorithm: f.checksum_algorithm.clone(),
        asn1_hint: f.asn1_hint.clone(),
        span: f.span,
    })
}

// -- Type helpers --

/// For conditional fields, extract the inner type.
/// For non-conditional, return the type as-is.
fn extract_inner_type(ty: &SemanticType) -> &SemanticType {
    // SemanticType doesn't wrap optionality -- the optionality is in FieldPresence.
    // So we just return the type directly.
    ty
}

/// Map SemanticType to WireType.
fn semantic_type_to_wire_type(ctx: &LowerCtx, ty: &SemanticType) -> WireType {
    match ty {
        SemanticType::Primitive { wire, .. } => primitive_to_wire(*wire),
        SemanticType::Bits { width_bits } => WireType::Bits(*width_bits),
        SemanticType::VarIntRef { varint_id, .. } => {
            if ctx.varint_encoding(varint_id) == VarIntEncoding::ContinuationBit {
                WireType::ContVarInt
            } else {
                WireType::VarInt
            }
        }
        SemanticType::Bytes { .. } => WireType::Bytes,
        SemanticType::PacketRef { name, .. } => WireType::Struct(name.clone()),
        SemanticType::EnumRef { name, .. } => WireType::Enum(name.clone()),
        SemanticType::FrameRef { name, .. } => WireType::Frame(name.clone()),
        SemanticType::CapsuleRef { name, .. } => WireType::Capsule(name.clone()),
        SemanticType::Array { .. } => WireType::Array,
    }
}

fn primitive_to_wire(p: PrimitiveWireType) -> WireType {
    match p {
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
    }
}

/// Extract the ref type name from a SemanticType for PacketRef/EnumRef/FrameRef/CapsuleRef.
fn extract_ref_type_name(ty: &SemanticType) -> Option<String> {
    match ty {
        SemanticType::PacketRef { name, .. }
        | SemanticType::EnumRef { name, .. }
        | SemanticType::FrameRef { name, .. }
        | SemanticType::CapsuleRef { name, .. } => Some(name.clone()),
        SemanticType::VarIntRef { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// Extract BytesSpec from a Bytes SemanticType.
fn extract_bytes_spec(ty: &SemanticType) -> Option<BytesSpec> {
    match ty {
        SemanticType::Bytes {
            bytes_kind,
            fixed_size,
            size_expr,
        } => {
            let spec = match bytes_kind {
                SemanticBytesKind::Fixed => BytesSpec::Fixed {
                    size: fixed_size.unwrap_or(0),
                },
                SemanticBytesKind::Length => BytesSpec::Length {
                    expr: convert_semantic_expr(
                        size_expr
                            .as_ref()
                            .expect("Length bytes kind must have a size expression"),
                    ),
                },
                SemanticBytesKind::Remaining => BytesSpec::Remaining,
                SemanticBytesKind::LengthOrRemaining => BytesSpec::LengthOrRemaining {
                    expr: convert_semantic_expr(
                        size_expr
                            .as_ref()
                            .expect("LengthOrRemaining bytes kind must have a size expression"),
                    ),
                },
            };
            Some(spec)
        }
        _ => None,
    }
}

/// Extract ArraySpec from an Array SemanticType.
fn extract_array_spec(ctx: &LowerCtx, ty: &SemanticType) -> Option<ArraySpec> {
    match ty {
        SemanticType::Array {
            element_type,
            count_expr,
            within_expr,
        } => {
            let element_wire_type = semantic_type_to_wire_type(ctx, element_type);
            let element_strategy = assign_element_strategy(element_type);
            let element_ref_type_name = extract_ref_type_name(element_type);
            let count = count_expr.as_ref().map(|e| convert_semantic_expr(e));
            let within = within_expr.as_ref().map(|e| convert_semantic_expr(e));

            Some(ArraySpec {
                element_wire_type,
                element_strategy,
                element_ref_type_name,
                count_expr: count,
                within_expr: within,
            })
        }
        _ => None,
    }
}

/// Extract BitgroupMember from a LayoutField's bitgroup_member ref + lookup the bitgroup.
fn extract_bitgroup_member(
    f: &LayoutField,
    bitgroups: &[LayoutBitGroup],
) -> Option<BitgroupMember> {
    let member_ref = f.bitgroup_member.as_ref()?;

    // Find the bitgroup by ID
    let group = bitgroups
        .iter()
        .enumerate()
        .find(|(_, bg)| bg.bitgroup_id == member_ref.bitgroup_id);

    if let Some((group_index, bg)) = group {
        Some(BitgroupMember {
            group_id: group_index as u32,
            total_bits: bg.total_bits,
            member_offset_bits: member_ref.offset_bits,
            member_width_bits: member_ref.width_bits,
            group_endianness: bg.endianness,
        })
    } else {
        None
    }
}

// -- Item lowering --

fn lower_items(
    items: &[SemanticScopeItem],
    derived_list: &[SemanticDerived],
    requires_list: &[SemanticRequire],
    ctx: &LowerCtx,
) -> Result<Vec<CodecItem>, CodecError> {
    let mut result = Vec::new();

    for item in items {
        match item {
            SemanticScopeItem::Field { field_id } => {
                result.push(CodecItem::Field {
                    field_id: field_id.clone(),
                });
            }
            SemanticScopeItem::Derived { derived_id } => {
                // Look up the derived definition
                let derived = derived_list
                    .iter()
                    .find(|d| d.derived_id == *derived_id)
                    .ok_or_else(|| CodecError {
                        msg: format!("derived not found: {derived_id}"),
                    })?;
                result.push(CodecItem::Derived(CodecDerivedItem {
                    item_id: derived.derived_id.clone(),
                    name: derived.name.clone(),
                    wire_type: semantic_type_to_wire_type(ctx, &derived.ty),
                    expr: convert_semantic_expr(&derived.expr),
                    span: derived.span,
                }));
            }
            SemanticScopeItem::Require { require_id } => {
                let require = requires_list
                    .iter()
                    .find(|r| r.require_id == *require_id)
                    .ok_or_else(|| CodecError {
                        msg: format!("require not found: {require_id}"),
                    })?;
                result.push(CodecItem::Require(CodecRequireItem {
                    item_id: require.require_id.clone(),
                    expr: convert_semantic_expr(&require.expr),
                    span: require.span,
                }));
            }
        }
    }

    Ok(result)
}

// -- Expression conversion --

/// Convert SemanticExpr -> CodecExpr (structural mapping).
fn convert_semantic_expr(expr: &sem_expr::SemanticExpr) -> CodecExpr {
    match expr {
        sem_expr::SemanticExpr::Literal { value } => CodecExpr::Literal {
            value: convert_literal(value),
        },
        sem_expr::SemanticExpr::ValueRef { reference } => CodecExpr::ValueRef {
            reference: convert_value_ref(reference),
        },
        sem_expr::SemanticExpr::TransitionPeerRef { reference } => {
            // Flatten TransitionPeerRef into a ValueRef with joined path
            let joined = format!(
                "{}.{}",
                match reference.peer {
                    sem_expr::TransitionPeerKind::Src => "src",
                    sem_expr::TransitionPeerKind::Dst => "dst",
                    sem_expr::TransitionPeerKind::EventParam => "event",
                },
                reference.path.join(".")
            );
            CodecExpr::ValueRef {
                reference: ValueRef {
                    value_id: reference
                        .event_param_id
                        .clone()
                        .unwrap_or_else(|| joined.clone()),
                    kind: ValueRefKind::Field,
                },
            }
        }
        sem_expr::SemanticExpr::Binary { op, left, right } => CodecExpr::Binary {
            op: op.clone(),
            left: Box::new(convert_semantic_expr(left)),
            right: Box::new(convert_semantic_expr(right)),
        },
        sem_expr::SemanticExpr::Unary { op, operand } => CodecExpr::Unary {
            op: op.clone(),
            operand: Box::new(convert_semantic_expr(operand)),
        },
        sem_expr::SemanticExpr::Coalesce { expr, default } => CodecExpr::Coalesce {
            expr: Box::new(convert_semantic_expr(expr)),
            default: Box::new(convert_semantic_expr(default)),
        },
        sem_expr::SemanticExpr::InState {
            expr,
            sm_id,
            sm_name,
            state_id,
            state_name,
        } => CodecExpr::InState {
            expr: Box::new(convert_semantic_expr(expr)),
            sm_id: sm_id.clone(),
            sm_name: sm_name.clone(),
            state_id: state_id.clone(),
            state_name: state_name.clone(),
        },
        sem_expr::SemanticExpr::Subscript { base, index } => CodecExpr::Subscript {
            base: Box::new(convert_semantic_expr(base)),
            index: Box::new(convert_semantic_expr(index)),
        },
        sem_expr::SemanticExpr::StateConstructor {
            sm_id,
            sm_name,
            state_id,
            state_name,
            args,
        } => CodecExpr::StateConstructor {
            sm_id: sm_id.clone(),
            sm_name: sm_name.clone(),
            state_id: state_id.clone(),
            state_name: state_name.clone(),
            args: args.iter().map(convert_semantic_expr).collect(),
        },
        sem_expr::SemanticExpr::Fill { value, count } => CodecExpr::Fill {
            value: Box::new(convert_semantic_expr(value)),
            count: Box::new(convert_semantic_expr(count)),
        },
        sem_expr::SemanticExpr::Slice { base, start, end } => CodecExpr::Slice {
            base: Box::new(convert_semantic_expr(base)),
            start: Box::new(convert_semantic_expr(start)),
            end: Box::new(convert_semantic_expr(end)),
        },
        sem_expr::SemanticExpr::All {
            collection,
            sm_id,
            sm_name,
            state_id,
            state_name,
        } => CodecExpr::All {
            collection: Box::new(convert_semantic_expr(collection)),
            sm_id: sm_id.clone(),
            sm_name: sm_name.clone(),
            state_id: state_id.clone(),
            state_name: state_name.clone(),
        },
    }
}

fn convert_value_ref(vr: &sem_expr::ValueRef) -> ValueRef {
    ValueRef {
        value_id: vr.value_id.clone(),
        kind: match vr.kind {
            sem_expr::ValueRefKind::Field => ValueRefKind::Field,
            sem_expr::ValueRefKind::Derived => ValueRefKind::Derived,
            sem_expr::ValueRefKind::Const => ValueRefKind::Const,
            sem_expr::ValueRefKind::StateField => ValueRefKind::Field,
        },
    }
}

fn convert_literal(lit: &sem_expr::SemanticLiteral) -> LiteralValue {
    match lit {
        sem_expr::SemanticLiteral::Int(v) => LiteralValue::Int(*v),
        sem_expr::SemanticLiteral::Bool(v) => LiteralValue::Bool(*v),
        sem_expr::SemanticLiteral::String(s) => LiteralValue::String(s.clone()),
        sem_expr::SemanticLiteral::Null => LiteralValue::Null,
    }
}

// -- Pattern conversion --

fn convert_pattern(p: &SemanticPattern) -> VariantPattern {
    match p {
        SemanticPattern::Exact { value } => VariantPattern::Exact { value: *value },
        SemanticPattern::RangeInclusive { start, end } => VariantPattern::RangeInclusive {
            start: *start,
            end: *end,
        },
        SemanticPattern::Wildcard => VariantPattern::Wildcard,
    }
}
