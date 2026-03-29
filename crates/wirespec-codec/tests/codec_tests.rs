// crates/wirespec-codec/tests/codec_tests.rs
use wirespec_codec::ir::*;
use wirespec_codec::lower::lower;
use wirespec_sema::ComplianceProfile;
use wirespec_syntax::parse;

fn full_pipeline(src: &str) -> CodecModule {
    let ast = parse(src).unwrap();
    let sem =
        wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    lower(&layout).unwrap()
}

#[test]
fn codec_empty_module() {
    let c = full_pipeline("module test");
    assert_eq!(c.schema_version, "codec-ir/v1");
    assert_eq!(c.module_name, "test");
}

#[test]
fn codec_simple_packet() {
    let c = full_pipeline("packet P { x: u8, y: u16 }");
    assert_eq!(c.packets.len(), 1);
    assert_eq!(c.packets[0].fields.len(), 2);
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Primitive);
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::U8);
    assert_eq!(c.packets[0].fields[1].wire_type, WireType::U16);
}

#[test]
fn codec_field_ids_stable() {
    let c = full_pipeline("packet P { x: u8, y: u16 }");
    assert_eq!(c.packets[0].fields[0].field_index, 0);
    assert_eq!(c.packets[0].fields[1].field_index, 1);
    // Field ID should contain something identifying the field
    assert!(!c.packets[0].fields[0].field_id.is_empty());
}

#[test]
fn codec_bytes_strategies() {
    let c = full_pipeline("packet P { a: bytes[6], b: bytes[remaining] }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::BytesFixed);
    assert_eq!(c.packets[0].fields[0].memory_tier, Some(MemoryTier::A));
    assert_eq!(
        c.packets[0].fields[1].strategy,
        FieldStrategy::BytesRemaining
    );
}

#[test]
fn codec_bitgroup_strategy() {
    let c = full_pipeline("packet P { a: bits[4], b: bits[4], c: u16 }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::BitGroup);
    assert_eq!(c.packets[0].fields[1].strategy, FieldStrategy::BitGroup);
    assert_eq!(c.packets[0].fields[2].strategy, FieldStrategy::Primitive);
    assert!(c.packets[0].fields[0].bitgroup_member.is_some());
}

#[test]
fn codec_conditional_strategy() {
    let c = full_pipeline("packet P { flags: u8, x: if flags & 0x01 { u16 } }");
    assert_eq!(c.packets[0].fields[1].strategy, FieldStrategy::Conditional);
    assert!(c.packets[0].fields[1].is_optional);
    assert!(c.packets[0].fields[1].condition.is_some());
}

#[test]
fn codec_array_strategy() {
    let c = full_pipeline("packet P { count: u16, items: [u8; count] }");
    assert_eq!(c.packets[0].fields[1].strategy, FieldStrategy::Array);
    assert!(c.packets[0].fields[1].array_spec.is_some());
    assert_eq!(c.packets[0].fields[1].memory_tier, Some(MemoryTier::B));
}

#[test]
fn codec_items_order() {
    let c = full_pipeline("packet P { x: u8, require x > 0, let y: bool = x != 0 }");
    assert_eq!(c.packets[0].items.len(), 3);
    assert!(matches!(&c.packets[0].items[0], CodecItem::Field { .. }));
    assert!(matches!(&c.packets[0].items[1], CodecItem::Require(_)));
    assert!(matches!(&c.packets[0].items[2], CodecItem::Derived(_)));
}

#[test]
fn codec_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x01 => B { x: u16 },
            _ => C { data: bytes[remaining] },
        }
    "#;
    let c = full_pipeline(src);
    assert_eq!(c.frames.len(), 1);
    assert_eq!(c.frames[0].tag.field_name, "tag");
    assert_eq!(c.frames[0].variants.len(), 3);
}

#[test]
fn codec_capsule() {
    let src = r#"
        capsule C {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => U { data: bytes[remaining] },
            },
        }
    "#;
    let c = full_pipeline(src);
    assert_eq!(c.capsules.len(), 1);
    assert_eq!(c.capsules[0].header_fields.len(), 2);
}

#[test]
fn codec_consts_enums_pass_through() {
    let c = full_pipeline("const MAX: u8 = 20\nenum E: u8 { A = 0 }");
    assert_eq!(c.consts.len(), 1);
    assert_eq!(c.enums.len(), 1);
}

#[test]
fn codec_checksum_plan() {
    let src = r#"
        packet P {
            data: u32,
            @checksum(internet)
            checksum: u16,
        }
    "#;
    let c = full_pipeline(src);
    let plan = c.packets[0].checksum_plan.as_ref().unwrap();
    assert_eq!(plan.algorithm_id, "internet");
    assert_eq!(plan.verify_mode, ChecksumVerifyMode::ZeroSum);
    assert_eq!(plan.field_width_bytes, 2);
}

#[test]
fn codec_varint() {
    let src = r#"
        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6], 0b01 => bits[14],
                0b10 => bits[30], 0b11 => bits[62],
            },
        }
        packet P { x: VarInt }
    "#;
    let c = full_pipeline(src);
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::VarInt);
}

#[test]
fn codec_struct_ref_strategy() {
    let src = r#"
        packet Inner { x: u8 }
        packet Outer { inner: Inner }
    "#;
    let c = full_pipeline(src);
    assert_eq!(c.packets[1].fields[0].strategy, FieldStrategy::Struct);
}

#[test]
fn codec_enum_ref_strategy() {
    let src = r#"
        enum E: u8 { A = 0, B = 1 }
        packet P { code: E }
    "#;
    let c = full_pipeline(src);
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Struct);
}

#[test]
fn codec_bytes_lor_strategy() {
    let src = r#"
        packet P {
            flags: u8,
            len: if flags & 0x01 { u16 },
            data: bytes[length_or_remaining: len],
        }
    "#;
    let c = full_pipeline(src);
    // bytes[length_or_remaining:] -> BytesLor strategy
    let data_field = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "data")
        .unwrap();
    assert_eq!(data_field.strategy, FieldStrategy::BytesLor);
    assert_eq!(data_field.memory_tier, Some(MemoryTier::A));
}

#[test]
fn codec_bytes_length_strategy() {
    let src = "packet P { len: u16, data: bytes[length: len] }";
    let c = full_pipeline(src);
    let data_field = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "data")
        .unwrap();
    assert_eq!(data_field.strategy, FieldStrategy::BytesLength);
    assert_eq!(data_field.memory_tier, Some(MemoryTier::A));
    assert!(data_field.bytes_spec.is_some());
}

#[test]
fn codec_optional_has_condition() {
    let src = "packet P { flags: u8, x: if flags & 1 { u16 } }";
    let c = full_pipeline(src);
    let x = c.packets[0].fields.iter().find(|f| f.name == "x").unwrap();
    assert!(x.is_optional);
    assert!(x.condition.is_some());
    assert_eq!(x.strategy, FieldStrategy::Conditional);
}

#[test]
fn codec_array_with_composite_element_tier_c() {
    let src = r#"
        packet Item { x: u8 }
        packet P { count: u8, items: [Item; count] }
    "#;
    let c = full_pipeline(src);
    let items_field = c.packets[1]
        .fields
        .iter()
        .find(|f| f.name == "items")
        .unwrap();
    assert_eq!(items_field.strategy, FieldStrategy::Array);
    assert_eq!(items_field.memory_tier, Some(MemoryTier::C));
}

#[test]
fn codec_array_with_scalar_element_tier_b() {
    let src = "packet P { count: u8, items: [u8; count] }";
    let c = full_pipeline(src);
    let items_field = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "items")
        .unwrap();
    assert_eq!(items_field.strategy, FieldStrategy::Array);
    assert_eq!(items_field.memory_tier, Some(MemoryTier::B));
}

#[test]
fn codec_checksum_crc32() {
    let src = r#"
        packet P {
            data: u32,
            @checksum(crc32)
            checksum: u32,
        }
    "#;
    let c = full_pipeline(src);
    let plan = c.packets[0].checksum_plan.as_ref().unwrap();
    assert_eq!(plan.algorithm_id, "crc32");
    assert_eq!(plan.verify_mode, ChecksumVerifyMode::RecomputeCompare);
    assert_eq!(plan.field_width_bytes, 4);
}

#[test]
fn codec_no_checksum_plan_without_annotation() {
    let src = "packet P { x: u8, y: u16 }";
    let c = full_pipeline(src);
    assert!(c.packets[0].checksum_plan.is_none());
}

#[test]
fn codec_derived_in_items() {
    let src = "packet P { x: u8, let y: bool = x != 0 }";
    let c = full_pipeline(src);
    assert!(
        c.packets[0]
            .items
            .iter()
            .any(|i| matches!(i, CodecItem::Derived(_)))
    );
}

#[test]
fn codec_require_in_items() {
    let src = "packet P { x: u8, require x > 0 }";
    let c = full_pipeline(src);
    assert!(
        c.packets[0]
            .items
            .iter()
            .any(|i| matches!(i, CodecItem::Require(_)))
    );
}

#[test]
fn codec_frame_variant_patterns() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x01..=0x03 => B { x: u8 },
            _ => C { data: bytes[remaining] },
        }
    "#;
    let c = full_pipeline(src);
    assert!(matches!(
        c.frames[0].variants[0].pattern,
        VariantPattern::Exact { value: 0 }
    ));
    assert!(matches!(
        c.frames[0].variants[1].pattern,
        VariantPattern::RangeInclusive { start: 1, end: 3 }
    ));
    assert!(matches!(
        c.frames[0].variants[2].pattern,
        VariantPattern::Wildcard
    ));
}

#[test]
fn codec_capsule_tag_expr() {
    let src = r#"
        capsule C {
            header: u8,
            length: u16,
            payload: match (header >> 4) within length {
                1 => A { data: bytes[remaining] },
                _ => B { data: bytes[remaining] },
            },
        }
    "#;
    let c = full_pipeline(src);
    assert!(c.capsules[0].tag_expr.is_some());
}

#[test]
fn codec_state_machine_pass_through() {
    let src = r#"
        state machine S {
            state A
            state B [terminal]
            initial A
            transition A -> B { on done }
        }
    "#;
    let c = full_pipeline(src);
    assert_eq!(c.state_machines.len(), 1);
}

#[test]
fn codec_max_elements_on_array() {
    let src = r#"
        packet P {
            count: u8,
            @max_len(256)
            items: [u8; count],
        }
    "#;
    let c = full_pipeline(src);
    let items = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "items")
        .unwrap();
    assert_eq!(items.max_elements, Some(256));
}
