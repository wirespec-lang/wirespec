use wirespec_codec::ir::*;
use wirespec_codec::lower::lower;
use wirespec_sema::ComplianceProfile;

fn codec(src: &str) -> CodecModule {
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem =
        wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    lower(&layout).unwrap()
}

// ── Strategy assignment for every type ──

#[test]
fn strategy_u8_primitive() {
    let c = codec("packet P { x: u8 }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Primitive);
}

#[test]
fn strategy_u16_primitive() {
    let c = codec("packet P { x: u16 }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Primitive);
}

#[test]
fn strategy_u32_primitive() {
    let c = codec("packet P { x: u32 }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Primitive);
}

#[test]
fn strategy_u64_primitive() {
    let c = codec("packet P { x: u64 }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Primitive);
}

#[test]
fn strategy_i8_primitive() {
    let c = codec("packet P { x: i8 }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Primitive);
}

#[test]
fn strategy_i16_primitive() {
    let c = codec("packet P { x: i16 }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Primitive);
}

#[test]
fn strategy_i32_primitive() {
    let c = codec("packet P { x: i32 }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Primitive);
}

#[test]
fn strategy_i64_primitive() {
    let c = codec("packet P { x: i64 }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Primitive);
}

#[test]
fn strategy_bits_in_bitgroup() {
    let c = codec("packet P { a: bits[4], b: bits[4] }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::BitGroup);
    assert_eq!(c.packets[0].fields[1].strategy, FieldStrategy::BitGroup);
}

#[test]
fn strategy_bytes_fixed() {
    let c = codec("packet P { data: bytes[16] }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::BytesFixed);
    assert_eq!(c.packets[0].fields[0].memory_tier, Some(MemoryTier::A));
}

#[test]
fn strategy_bytes_remaining() {
    let c = codec("packet P { data: bytes[remaining] }");
    assert_eq!(
        c.packets[0].fields[0].strategy,
        FieldStrategy::BytesRemaining
    );
    assert_eq!(c.packets[0].fields[0].memory_tier, Some(MemoryTier::A));
}

#[test]
fn strategy_bytes_length() {
    let c = codec("packet P { len: u16, data: bytes[length: len] }");
    let data = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "data")
        .unwrap();
    assert_eq!(data.strategy, FieldStrategy::BytesLength);
    assert_eq!(data.memory_tier, Some(MemoryTier::A));
    assert!(data.bytes_spec.is_some());
}

#[test]
fn strategy_bytes_lor() {
    let c = codec(
        "packet P { flags: u8, len: if flags & 0x01 { u16 }, data: bytes[length_or_remaining: len] }",
    );
    let data = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "data")
        .unwrap();
    assert_eq!(data.strategy, FieldStrategy::BytesLor);
    assert_eq!(data.memory_tier, Some(MemoryTier::A));
}

#[test]
fn strategy_conditional() {
    let c = codec("packet P { flags: u8, x: if flags & 1 { u16 } }");
    let x = c.packets[0].fields.iter().find(|f| f.name == "x").unwrap();
    assert_eq!(x.strategy, FieldStrategy::Conditional);
    assert!(x.is_optional);
    assert!(x.condition.is_some());
}

#[test]
fn strategy_array_scalar_tier_b() {
    let c = codec("packet P { count: u8, items: [u8; count] }");
    let items = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "items")
        .unwrap();
    assert_eq!(items.strategy, FieldStrategy::Array);
    assert_eq!(items.memory_tier, Some(MemoryTier::B));
    assert!(items.array_spec.is_some());
}

#[test]
fn strategy_array_composite_tier_c() {
    let c = codec("packet Inner { x: u8 }\npacket Outer { count: u8, items: [Inner; count] }");
    let items = c.packets[1]
        .fields
        .iter()
        .find(|f| f.name == "items")
        .unwrap();
    assert_eq!(items.strategy, FieldStrategy::Array);
    assert_eq!(items.memory_tier, Some(MemoryTier::C));
}

#[test]
fn strategy_varint() {
    let src = r#"type V = { prefix: bits[2], value: match prefix { 0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62] } }
    packet P { x: V }"#;
    let c = codec(src);
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::VarInt);
}

#[test]
fn strategy_cont_varint() {
    let src = r#"type MqttLen = varint { continuation_bit: msb, value_bits: 7, max_bytes: 4, byte_order: little }
    packet P { x: MqttLen }"#;
    let c = codec(src);
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::ContVarInt);
}

#[test]
fn strategy_checksum() {
    let c = codec("packet P { data: u32, @checksum(internet) c: u16 }");
    let ck = c.packets[0].fields.iter().find(|f| f.name == "c").unwrap();
    assert_eq!(ck.strategy, FieldStrategy::Checksum);
}

#[test]
fn strategy_struct_ref() {
    let c = codec("packet Inner { x: u8 }\npacket Outer { inner: Inner }");
    assert_eq!(c.packets[1].fields[0].strategy, FieldStrategy::Struct);
    assert_eq!(
        c.packets[1].fields[0].ref_type_name,
        Some("Inner".to_string())
    );
}

#[test]
fn strategy_enum_ref() {
    let c = codec("enum E: u8 { A = 0, B = 1 }\npacket P { code: E }");
    assert_eq!(c.packets[0].fields[0].strategy, FieldStrategy::Struct);
    assert_eq!(c.packets[0].fields[0].ref_type_name, Some("E".to_string()));
}

// ── Wire type mapping ──

#[test]
fn wire_type_u8() {
    let c = codec("packet P { x: u8 }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::U8);
}

#[test]
fn wire_type_u16() {
    let c = codec("packet P { x: u16 }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::U16);
}

#[test]
fn wire_type_u24() {
    let c = codec("packet P { x: u24 }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::U24);
}

#[test]
fn wire_type_u32() {
    let c = codec("packet P { x: u32 }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::U32);
}

#[test]
fn wire_type_u64() {
    let c = codec("packet P { x: u64 }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::U64);
}

#[test]
fn wire_type_i8() {
    let c = codec("packet P { x: i8 }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::I8);
}

#[test]
fn wire_type_i16() {
    let c = codec("packet P { x: i16 }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::I16);
}

#[test]
fn wire_type_i32() {
    let c = codec("packet P { x: i32 }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::I32);
}

#[test]
fn wire_type_i64() {
    let c = codec("packet P { x: i64 }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::I64);
}

#[test]
fn wire_type_bits() {
    let c = codec("packet P { a: bits[4], b: bits[4] }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::Bits(4));
}

#[test]
fn wire_type_bytes() {
    let c = codec("packet P { data: bytes[remaining] }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::Bytes);
}

#[test]
fn wire_type_struct_ref() {
    let c = codec("packet Inner { x: u8 }\npacket Outer { inner: Inner }");
    assert_eq!(
        c.packets[1].fields[0].wire_type,
        WireType::Struct("Inner".into())
    );
}

#[test]
fn wire_type_enum_ref() {
    let c = codec("enum E: u8 { A = 0 }\npacket P { code: E }");
    assert_eq!(c.packets[0].fields[0].wire_type, WireType::Enum("E".into()));
}

#[test]
fn wire_type_array() {
    let c = codec("packet P { count: u8, items: [u8; count] }");
    assert_eq!(c.packets[0].fields[1].wire_type, WireType::Array);
}

// ── Checksum plan correctness ──

#[test]
fn checksum_plan_internet() {
    let c = codec("packet P { data: u32, @checksum(internet) c: u16 }");
    let plan = c.packets[0].checksum_plan.as_ref().unwrap();
    assert_eq!(plan.algorithm_id, "internet");
    assert_eq!(plan.verify_mode, ChecksumVerifyMode::ZeroSum);
    assert_eq!(plan.field_width_bytes, 2);
}

#[test]
fn checksum_plan_crc32() {
    let c = codec("packet P { data: u32, @checksum(crc32) c: u32 }");
    let plan = c.packets[0].checksum_plan.as_ref().unwrap();
    assert_eq!(plan.algorithm_id, "crc32");
    assert_eq!(plan.verify_mode, ChecksumVerifyMode::RecomputeCompare);
    assert_eq!(plan.field_width_bytes, 4);
}

#[test]
fn no_checksum_plan() {
    let c = codec("packet P { x: u8, y: u16 }");
    assert!(c.packets[0].checksum_plan.is_none());
}

#[test]
fn checksum_field_has_algorithm() {
    let c = codec("packet P { data: u32, @checksum(internet) c: u16 }");
    let ck = c.packets[0].fields.iter().find(|f| f.name == "c").unwrap();
    assert_eq!(ck.checksum_algorithm, Some("internet".to_string()));
}

// ── Field index consistency ──

#[test]
fn field_indices_sequential() {
    let c = codec("packet P { a: u8, b: u16, c: u32 }");
    for (i, f) in c.packets[0].fields.iter().enumerate() {
        assert_eq!(f.field_index, i as u32);
    }
}

#[test]
fn field_indices_sequential_many_fields() {
    let c = codec("packet P { a: u8, b: u16, c: u32, d: u64, e: i8, f: i16 }");
    for (i, f) in c.packets[0].fields.iter().enumerate() {
        assert_eq!(f.field_index, i as u32, "field {} has wrong index", f.name);
    }
}

// ── Items ordering ──

#[test]
fn items_match_declaration_order() {
    let c = codec("packet P { x: u8, require x > 0, let y: u64 = x + 1, z: u16 }");
    assert_eq!(c.packets[0].items.len(), 4);
    assert!(matches!(&c.packets[0].items[0], CodecItem::Field { .. }));
    assert!(matches!(&c.packets[0].items[1], CodecItem::Require(_)));
    assert!(matches!(&c.packets[0].items[2], CodecItem::Derived(_)));
    assert!(matches!(&c.packets[0].items[3], CodecItem::Field { .. }));
}

#[test]
fn items_multiple_requires() {
    let c = codec("packet P { x: u8, y: u8, require x > 0, require y < 100 }");
    let require_count = c.packets[0]
        .items
        .iter()
        .filter(|i| matches!(i, CodecItem::Require(_)))
        .count();
    assert_eq!(require_count, 2);
}

#[test]
fn items_multiple_derived() {
    let c = codec("packet P { x: u8, let a: bool = x != 0, let b: u64 = x + 1 }");
    let derived_count = c.packets[0]
        .items
        .iter()
        .filter(|i| matches!(i, CodecItem::Derived(_)))
        .count();
    assert_eq!(derived_count, 2);
}

// ── Bytes spec correctness ──

#[test]
fn bytes_spec_fixed_size() {
    let c = codec("packet P { data: bytes[6] }");
    let field = &c.packets[0].fields[0];
    assert!(matches!(
        field.bytes_spec,
        Some(BytesSpec::Fixed { size: 6 })
    ));
}

#[test]
fn bytes_spec_remaining() {
    let c = codec("packet P { data: bytes[remaining] }");
    let field = &c.packets[0].fields[0];
    assert!(matches!(field.bytes_spec, Some(BytesSpec::Remaining)));
}

#[test]
fn bytes_spec_length_has_expr() {
    let c = codec("packet P { len: u16, data: bytes[length: len] }");
    let data = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "data")
        .unwrap();
    assert!(matches!(data.bytes_spec, Some(BytesSpec::Length { .. })));
}

// ── Array spec correctness ──

#[test]
fn array_spec_element_wire_type() {
    let c = codec("packet P { count: u8, items: [u16; count] }");
    let items = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "items")
        .unwrap();
    let spec = items.array_spec.as_ref().unwrap();
    assert_eq!(spec.element_wire_type, WireType::U16);
}

#[test]
fn array_spec_element_strategy_primitive() {
    let c = codec("packet P { count: u8, items: [u8; count] }");
    let items = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "items")
        .unwrap();
    let spec = items.array_spec.as_ref().unwrap();
    assert_eq!(spec.element_strategy, FieldStrategy::Primitive);
}

#[test]
fn array_spec_composite_element() {
    let c = codec("packet Inner { x: u8 }\npacket P { count: u8, items: [Inner; count] }");
    let items = c.packets[1]
        .fields
        .iter()
        .find(|f| f.name == "items")
        .unwrap();
    let spec = items.array_spec.as_ref().unwrap();
    assert_eq!(spec.element_wire_type, WireType::Struct("Inner".into()));
    assert_eq!(spec.element_ref_type_name, Some("Inner".to_string()));
}

#[test]
fn array_max_elements() {
    let c = codec("packet P { count: u8, @max_len(256) items: [u8; count] }");
    let items = c.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "items")
        .unwrap();
    assert_eq!(items.max_elements, Some(256));
}

// ── Bitgroup member in codec ──

#[test]
fn bitgroup_member_populated() {
    let c = codec("packet P { a: bits[4], b: bits[4] }");
    let a = &c.packets[0].fields[0];
    let bg = a.bitgroup_member.as_ref().unwrap();
    assert_eq!(bg.total_bits, 8);
    assert_eq!(bg.member_width_bits, 4);
}

#[test]
fn bitgroup_member_offset_bits() {
    let c = codec("packet P { a: bits[4], b: bits[4] }");
    let a = &c.packets[0].fields[0];
    let b = &c.packets[0].fields[1];
    let bg_a = a.bitgroup_member.as_ref().unwrap();
    let bg_b = b.bitgroup_member.as_ref().unwrap();
    // Members should have different offsets
    assert_ne!(bg_a.member_offset_bits, bg_b.member_offset_bits);
}

// ── Pass-through declarations ──

#[test]
fn consts_preserved() {
    let c = codec("const MAX: u8 = 255\npacket P { x: u8 }");
    assert_eq!(c.consts.len(), 1);
}

#[test]
fn enums_preserved() {
    let c = codec("enum E: u8 { A = 0, B = 1 }\npacket P { x: u8 }");
    assert_eq!(c.enums.len(), 1);
}

#[test]
fn state_machines_preserved() {
    let c = codec(
        "state machine S { state A state B [terminal] initial A transition A -> B { on done } }\npacket P { x: u8 }",
    );
    assert_eq!(c.state_machines.len(), 1);
}

#[test]
fn varints_preserved() {
    let src = r#"type V = { prefix: bits[2], value: match prefix { 0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62] } }
    packet P { x: V }"#;
    let c = codec(src);
    assert_eq!(c.varints.len(), 1);
}

// ── Frame codec ──

#[test]
fn frame_tag_wire_type() {
    let c = codec(r#"frame F = match t: u8 { 0 => A {}, _ => B { data: bytes[remaining] } }"#);
    assert_eq!(c.frames[0].tag.wire_type, WireType::U8);
    assert_eq!(c.frames[0].tag.field_name, "t");
}

#[test]
fn frame_variant_patterns() {
    let c = codec(
        r#"frame F = match t: u8 {
        0x00 => A {},
        0x01..=0x03 => B { x: u8 },
        _ => C { data: bytes[remaining] },
    }"#,
    );
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
fn frame_variant_fields_strategies() {
    let c = codec(
        r#"frame F = match t: u8 {
        0 => A { x: u8, y: u16 },
        _ => B { data: bytes[remaining] },
    }"#,
    );
    assert_eq!(
        c.frames[0].variants[0].fields[0].strategy,
        FieldStrategy::Primitive
    );
    assert_eq!(
        c.frames[0].variants[1].fields[0].strategy,
        FieldStrategy::BytesRemaining
    );
}

// ── Capsule codec ──

#[test]
fn capsule_header_fields() {
    let c = codec(
        r#"capsule C {
        type_field: u8, length: u16,
        payload: match type_field within length {
            0 => D { data: bytes[remaining] },
            _ => U { data: bytes[remaining] },
        },
    }"#,
    );
    assert_eq!(c.capsules[0].header_fields.len(), 2);
    assert_eq!(c.capsules[0].header_fields[0].name, "type_field");
    assert_eq!(c.capsules[0].header_fields[1].name, "length");
}

#[test]
fn capsule_within_field() {
    let c = codec(
        r#"capsule C {
        type_field: u8, length: u16,
        payload: match type_field within length {
            0 => D { data: bytes[remaining] },
            _ => U { data: bytes[remaining] },
        },
    }"#,
    );
    assert_eq!(c.capsules[0].within_field, "length");
}

#[test]
fn capsule_tag_expr_from_expr_syntax() {
    let c = codec(
        r#"capsule C {
        header: u8, length: u16,
        payload: match (header >> 4) within length {
            1 => A { data: bytes[remaining] },
            _ => B { data: bytes[remaining] },
        },
    }"#,
    );
    assert!(c.capsules[0].tag_expr.is_some());
}

#[test]
fn capsule_tag_expr_none_for_field_syntax() {
    let c = codec(
        r#"capsule C {
        type_field: u8, length: u16,
        payload: match type_field within length {
            0 => D { data: bytes[remaining] },
            _ => U { data: bytes[remaining] },
        },
    }"#,
    );
    assert!(c.capsules[0].tag_expr.is_none());
}

// ── Schema version ──

#[test]
fn schema_version_correct() {
    let c = codec("packet P { x: u8 }");
    assert_eq!(c.schema_version, "codec-ir/v1");
}

// ── Module endianness ──

#[test]
fn module_endianness_default_big() {
    let c = codec("packet P { x: u8 }");
    assert_eq!(c.module_endianness, wirespec_sema::types::Endianness::Big);
}

#[test]
fn module_endianness_explicit_little() {
    let c = codec("@endian little\nmodule test\npacket P { x: u8 }");
    assert_eq!(
        c.module_endianness,
        wirespec_sema::types::Endianness::Little
    );
}

// ── Field endianness in codec ──

#[test]
fn field_endianness_u8_none() {
    let c = codec("packet P { x: u8 }");
    assert!(c.packets[0].fields[0].endianness.is_none());
}

#[test]
fn field_endianness_u16_big_default() {
    let c = codec("packet P { x: u16 }");
    assert_eq!(
        c.packets[0].fields[0].endianness,
        Some(wirespec_sema::types::Endianness::Big)
    );
}

#[test]
fn field_endianness_u16le_override() {
    let c = codec("@endian big\nmodule test\npacket P { x: u16le }");
    assert_eq!(
        c.packets[0].fields[0].endianness,
        Some(wirespec_sema::types::Endianness::Little)
    );
}

// ── Conditional field inner wire type ──

#[test]
fn conditional_inner_wire_type() {
    let c = codec("packet P { flags: u8, x: if flags & 1 { u32 } }");
    let x = c.packets[0].fields.iter().find(|f| f.name == "x").unwrap();
    assert_eq!(x.inner_wire_type, Some(WireType::U32));
}

#[test]
fn non_conditional_inner_wire_type_none() {
    let c = codec("packet P { x: u32 }");
    assert!(c.packets[0].fields[0].inner_wire_type.is_none());
}

// ── Multiple packets ──

#[test]
fn multiple_packets_correct_count() {
    let c = codec("packet A { x: u8 }\npacket B { y: u16 }");
    assert_eq!(c.packets.len(), 2);
    assert_eq!(c.packets[0].name, "A");
    assert_eq!(c.packets[1].name, "B");
}

// ── Empty module ──

#[test]
fn empty_module_no_packets() {
    let c = codec("module test");
    assert!(c.packets.is_empty());
    assert!(c.frames.is_empty());
    assert!(c.capsules.is_empty());
}
