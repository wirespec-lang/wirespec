// crates/wirespec-codec/tests/strategy_tests.rs
use wirespec_codec::ir::*;
use wirespec_codec::strategy::*;
use wirespec_sema::types::*;

#[test]
fn strategy_primitive_u8() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Primitive {
            wire: PrimitiveWireType::U8,
            endianness: None,
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Primitive);
}

#[test]
fn strategy_bitgroup_overrides_primitive() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Bits { width_bits: 4 },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: true,
    });
    assert_eq!(s, FieldStrategy::BitGroup);
}

#[test]
fn strategy_checksum_overrides_primitive() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Primitive {
            wire: PrimitiveWireType::U16,
            endianness: None,
        },
        is_optional: false,
        has_checksum: true,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Checksum);
}

#[test]
fn strategy_conditional() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Primitive {
            wire: PrimitiveWireType::U16,
            endianness: None,
        },
        is_optional: true,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Conditional);
}

#[test]
fn strategy_varint_ref() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::VarIntRef {
            varint_id: "varint:V".into(),
            name: "V".into(),
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::VarInt);
}

#[test]
fn strategy_bytes_remaining() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Bytes {
            bytes_kind: SemanticBytesKind::Remaining,
            fixed_size: None,
            size_expr: None,
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::BytesRemaining);
}

#[test]
fn strategy_bytes_fixed() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Bytes {
            bytes_kind: SemanticBytesKind::Fixed,
            fixed_size: Some(6),
            size_expr: None,
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::BytesFixed);
}

#[test]
fn strategy_struct_ref() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::PacketRef {
            packet_id: "packet:P".into(),
            name: "P".into(),
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Struct);
}

#[test]
fn strategy_array() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Array {
            element_type: Box::new(SemanticType::Primitive {
                wire: PrimitiveWireType::U8,
                endianness: None,
            }),
            count_expr: None,
            within_expr: None,
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Array);
}

#[test]
fn tier_bytes_is_a() {
    assert_eq!(
        assign_memory_tier(FieldStrategy::BytesFixed),
        Some(MemoryTier::A)
    );
    assert_eq!(
        assign_memory_tier(FieldStrategy::BytesLength),
        Some(MemoryTier::A)
    );
    assert_eq!(
        assign_memory_tier(FieldStrategy::BytesRemaining),
        Some(MemoryTier::A)
    );
    assert_eq!(
        assign_memory_tier(FieldStrategy::BytesLor),
        Some(MemoryTier::A)
    );
}

#[test]
fn tier_primitive_is_none() {
    assert_eq!(assign_memory_tier(FieldStrategy::Primitive), None);
    assert_eq!(assign_memory_tier(FieldStrategy::VarInt), None);
}

#[test]
fn strategy_enum_ref() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::EnumRef {
            enum_id: "enum:E".into(),
            name: "E".into(),
            is_flags: false,
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Struct);
}

#[test]
fn strategy_frame_ref() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::FrameRef {
            frame_id: "frame:F".into(),
            name: "F".into(),
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Struct);
}

#[test]
fn strategy_capsule_ref() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::CapsuleRef {
            capsule_id: "capsule:C".into(),
            name: "C".into(),
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Struct);
}

#[test]
fn strategy_bytes_length() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Bytes {
            bytes_kind: SemanticBytesKind::Length,
            fixed_size: None,
            size_expr: None,
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::BytesLength);
}

#[test]
fn strategy_bytes_lor() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Bytes {
            bytes_kind: SemanticBytesKind::LengthOrRemaining,
            fixed_size: None,
            size_expr: None,
        },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::BytesLor);
}

#[test]
fn strategy_bits() {
    let s = assign_strategy(&StrategyInput {
        ty: &SemanticType::Bits { width_bits: 4 },
        is_optional: false,
        has_checksum: false,
        has_bitgroup: false,
    });
    assert_eq!(s, FieldStrategy::Primitive);
}

#[test]
fn tier_array_scalar_is_b() {
    assert_eq!(
        assign_array_memory_tier(&SemanticType::Primitive {
            wire: PrimitiveWireType::U8,
            endianness: None
        }),
        MemoryTier::B
    );
}

#[test]
fn tier_array_composite_is_c() {
    assert_eq!(
        assign_array_memory_tier(&SemanticType::PacketRef {
            packet_id: "p".into(),
            name: "P".into()
        }),
        MemoryTier::C
    );
}

#[test]
fn tier_array_enum_is_c() {
    assert_eq!(
        assign_array_memory_tier(&SemanticType::EnumRef {
            enum_id: "e".into(),
            name: "E".into(),
            is_flags: false
        }),
        MemoryTier::C
    );
}
