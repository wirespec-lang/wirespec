// crates/wirespec-codec/src/strategy.rs
//
// Field strategy and memory tier assignment per spec S14-S15.

use crate::ir::*;
use wirespec_sema::types::*;

/// Input for strategy assignment -- carries all the flags needed
/// to determine the correct FieldStrategy.
pub struct StrategyInput<'a> {
    pub ty: &'a SemanticType,
    pub is_optional: bool,
    pub has_checksum: bool,
    pub has_bitgroup: bool,
}

/// Assign field strategy per spec S14 priority order.
///
/// Priority: BitGroup > Checksum > Conditional > type-based (varint, bytes, struct, array, primitive)
pub fn assign_strategy(input: &StrategyInput) -> FieldStrategy {
    // Priority 1: bitgroup
    if input.has_bitgroup {
        return FieldStrategy::BitGroup;
    }
    // Priority 2: checksum
    if input.has_checksum {
        return FieldStrategy::Checksum;
    }
    // Priority 3: conditional
    if input.is_optional {
        return FieldStrategy::Conditional;
    }
    // Priority 4-9: type-based
    type_based_strategy(input.ty)
}

/// Determine strategy purely from the SemanticType.
/// Used for both top-level fields and array element types.
fn type_based_strategy(ty: &SemanticType) -> FieldStrategy {
    match ty {
        SemanticType::VarIntRef { .. } => FieldStrategy::VarInt,
        SemanticType::Bytes { bytes_kind, .. } => match bytes_kind {
            SemanticBytesKind::Fixed => FieldStrategy::BytesFixed,
            SemanticBytesKind::Length => FieldStrategy::BytesLength,
            SemanticBytesKind::Remaining => FieldStrategy::BytesRemaining,
            SemanticBytesKind::LengthOrRemaining => FieldStrategy::BytesLor,
        },
        SemanticType::PacketRef { .. }
        | SemanticType::FrameRef { .. }
        | SemanticType::CapsuleRef { .. }
        | SemanticType::EnumRef { .. } => FieldStrategy::Struct,
        SemanticType::Array { .. } => FieldStrategy::Array,
        SemanticType::Primitive { .. } | SemanticType::Bits { .. } => FieldStrategy::Primitive,
    }
}

/// Assign memory tier per spec S15.
/// Returns None for scalars/primitives (no memory allocation needed).
pub fn assign_memory_tier(strategy: FieldStrategy) -> Option<MemoryTier> {
    match strategy {
        FieldStrategy::BytesFixed
        | FieldStrategy::BytesLength
        | FieldStrategy::BytesRemaining
        | FieldStrategy::BytesLor => Some(MemoryTier::A),
        // Array tier is determined by element type (B for scalar, C for composite)
        // -- handled in lower.rs where element type is known.
        _ => None,
    }
}

/// Assign array memory tier based on element type.
/// Scalar arrays -> B (materialized scalar), composite arrays -> C (materialized composite).
pub fn assign_array_memory_tier(element_type: &SemanticType) -> MemoryTier {
    match element_type {
        SemanticType::PacketRef { .. }
        | SemanticType::FrameRef { .. }
        | SemanticType::CapsuleRef { .. }
        | SemanticType::EnumRef { .. } => MemoryTier::C,
        _ => MemoryTier::B,
    }
}

/// Determine element strategy for array elements (uses type_based_strategy).
pub fn assign_element_strategy(element_type: &SemanticType) -> FieldStrategy {
    type_based_strategy(element_type)
}
