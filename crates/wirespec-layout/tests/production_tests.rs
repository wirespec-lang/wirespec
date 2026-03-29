use wirespec_layout::lower::lower;
use wirespec_sema::types::Endianness;
use wirespec_sema::{ComplianceProfile, analyze};

fn layout(src: &str) -> wirespec_layout::ir::LayoutModule {
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    lower(&sem).unwrap()
}

// ── Wire width for all primitives ──

#[test]
fn wire_width_u8() {
    let l = layout("packet P { x: u8 }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(8));
}

#[test]
fn wire_width_u16() {
    let l = layout("packet P { x: u16 }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(16));
}

#[test]
fn wire_width_u24() {
    let l = layout("packet P { x: u24 }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(24));
}

#[test]
fn wire_width_u32() {
    let l = layout("packet P { x: u32 }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(32));
}

#[test]
fn wire_width_u64() {
    let l = layout("packet P { x: u64 }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(64));
}

#[test]
fn wire_width_bits_1() {
    let l = layout("packet P { a: bits[1], b: bits[7] }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(1));
    assert_eq!(l.packets[0].fields[1].wire_width_bits, Some(7));
}

#[test]
fn wire_width_bytes_none() {
    let l = layout("packet P { data: bytes[remaining] }");
    assert!(l.packets[0].fields[0].wire_width_bits.is_none());
}

#[test]
fn wire_width_varint_none() {
    let src = r#"type V = { prefix: bits[2], value: match prefix { 0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62] } }
    packet P { x: V }"#;
    let l = layout(src);
    assert!(l.packets[0].fields[0].wire_width_bits.is_none());
}

// ── Wire width for signed types ──

#[test]
fn wire_width_i8() {
    let l = layout("packet P { x: i8 }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(8));
}

#[test]
fn wire_width_i16() {
    let l = layout("packet P { x: i16 }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(16));
}

#[test]
fn wire_width_i32() {
    let l = layout("packet P { x: i32 }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(32));
}

#[test]
fn wire_width_i64() {
    let l = layout("packet P { x: i64 }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(64));
}

#[test]
fn wire_width_bit_is_1() {
    let l = layout("packet P { a: bit, b: bits[7] }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(1));
}

// ── Wire width for dynamic types returns None ──

#[test]
fn wire_width_array_none() {
    let l = layout("packet P { count: u8, items: [u8; count] }");
    assert!(l.packets[0].fields[1].wire_width_bits.is_none());
}

#[test]
fn wire_width_bytes_fixed_none() {
    // bytes[N] is still dynamically sized in layout IR (no wire_width_bits)
    let l = layout("packet P { mac: bytes[6] }");
    assert!(l.packets[0].fields[0].wire_width_bits.is_none());
}

#[test]
fn wire_width_bytes_length_none() {
    let l = layout("packet P { len: u16, data: bytes[length: len] }");
    assert!(l.packets[0].fields[1].wire_width_bits.is_none());
}

#[test]
fn wire_width_packet_ref_none() {
    let l = layout("packet Inner { x: u8 }\npacket Outer { inner: Inner }");
    assert!(l.packets[1].fields[0].wire_width_bits.is_none());
}

// ── Endianness per-field override ──

#[test]
fn endianness_u16le_in_big_module() {
    let l = layout("@endian big\nmodule test\npacket P { x: u16le }");
    assert_eq!(l.packets[0].fields[0].endianness, Some(Endianness::Little));
}

#[test]
fn endianness_u32be_in_little_module() {
    let l = layout("@endian little\nmodule test\npacket P { x: u32be }");
    assert_eq!(l.packets[0].fields[0].endianness, Some(Endianness::Big));
}

#[test]
fn endianness_u8_none() {
    let l = layout("packet P { x: u8 }");
    assert!(l.packets[0].fields[0].endianness.is_none());
}

#[test]
fn endianness_i8_none() {
    let l = layout("packet P { x: i8 }");
    assert!(l.packets[0].fields[0].endianness.is_none());
}

#[test]
fn endianness_bits_none() {
    let l = layout("packet P { a: bits[4], b: bits[4] }");
    assert!(l.packets[0].fields[0].endianness.is_none());
    assert!(l.packets[0].fields[1].endianness.is_none());
}

#[test]
fn endianness_u16_inherits_module_big() {
    let l = layout("@endian big\nmodule test\npacket P { x: u16 }");
    assert_eq!(l.packets[0].fields[0].endianness, Some(Endianness::Big));
}

#[test]
fn endianness_u16_inherits_module_little() {
    let l = layout("@endian little\nmodule test\npacket P { x: u16 }");
    assert_eq!(l.packets[0].fields[0].endianness, Some(Endianness::Little));
}

#[test]
fn endianness_u64_has_some() {
    let l = layout("packet P { x: u64 }");
    assert!(l.packets[0].fields[0].endianness.is_some());
}

// ── Module-level endianness ──

#[test]
fn module_default_endianness_is_big() {
    let l = layout("packet P { x: u8 }");
    assert_eq!(l.module_endianness, Endianness::Big);
}

#[test]
fn module_explicit_little_endianness() {
    let l = layout("@endian little\nmodule test\npacket P { x: u8 }");
    assert_eq!(l.module_endianness, Endianness::Little);
}

// ── Bitgroup boundary cases ──

#[test]
fn bitgroup_exactly_8_bits() {
    let l = layout("packet P { a: bits[3], b: bits[5] }");
    assert_eq!(l.packets[0].bitgroups.len(), 1);
    assert_eq!(l.packets[0].bitgroups[0].total_bits, 8);
}

#[test]
fn bitgroup_exactly_16_bits() {
    let l = layout("packet P { a: bits[10], b: bits[6] }");
    assert_eq!(l.packets[0].bitgroups.len(), 1);
    assert_eq!(l.packets[0].bitgroups[0].total_bits, 16);
}

#[test]
fn bitgroup_exactly_32_bits() {
    let l = layout("packet P { a: bits[1], b: bits[31] }");
    assert_eq!(l.packets[0].bitgroups.len(), 1);
    assert_eq!(l.packets[0].bitgroups[0].total_bits, 32);
}

#[test]
fn bitgroup_exactly_64_bits() {
    let l = layout("packet P { a: bits[32], b: bits[32] }");
    assert_eq!(l.packets[0].bitgroups.len(), 1);
    assert_eq!(l.packets[0].bitgroups[0].total_bits, 64);
}

#[test]
fn bitgroup_three_groups() {
    let l = layout("packet P { a: bits[8], mid: u8, b: bits[8], mid2: u16, c: bits[8] }");
    assert_eq!(l.packets[0].bitgroups.len(), 3);
}

#[test]
fn bitgroup_member_count_matches_fields() {
    let l = layout("packet P { a: bits[2], b: bits[3], c: bits[3] }");
    assert_eq!(l.packets[0].bitgroups[0].members.len(), 3);
}

#[test]
fn bitgroup_fields_have_member_ref() {
    let l = layout("packet P { a: bits[4], b: bits[4], c: u16 }");
    assert!(l.packets[0].fields[0].bitgroup_member.is_some());
    assert!(l.packets[0].fields[1].bitgroup_member.is_some());
    assert!(l.packets[0].fields[2].bitgroup_member.is_none());
}

#[test]
fn bitgroup_in_frame_variant() {
    let l = layout(
        r#"frame F = match t: u8 {
        0 => A { x: bits[4], y: bits[4], z: u16 },
        _ => B { data: bytes[remaining] },
    }"#,
    );
    assert_eq!(l.frames[0].variants[0].bitgroups.len(), 1);
}

#[test]
fn bitgroup_in_capsule_header() {
    let l = layout(
        r#"capsule C {
        flags: bits[4],
        type_field: bits[4],
        length: u16,
        payload: match type_field within length {
            0 => D { data: bytes[remaining] },
            _ => U { data: bytes[remaining] },
        },
    }"#,
    );
    assert!(!l.capsules[0].header_bitgroups.is_empty());
    assert_eq!(l.capsules[0].header_bitgroups[0].total_bits, 8);
}

// ── Bitgroup error: not byte-aligned ──

#[test]
fn bitgroup_unaligned_error() {
    let ast = wirespec_syntax::parse("packet P { a: bits[5], b: u8 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let result = lower(&sem);
    assert!(result.is_err());
}

// ── Frame layout ──

#[test]
fn frame_tag_u8_no_endianness() {
    let l = layout(
        r#"frame F = match t: u8 {
        0 => A {},
        _ => B { data: bytes[remaining] },
    }"#,
    );
    assert!(l.frames[0].tag_endianness.is_none());
}

#[test]
fn frame_tag_u16_has_endianness() {
    let l = layout(
        r#"frame F = match t: u16 {
        0 => A {},
        _ => B { data: bytes[remaining] },
    }"#,
    );
    assert!(l.frames[0].tag_endianness.is_some());
}

#[test]
fn frame_variant_count() {
    let l = layout(
        r#"frame F = match t: u8 {
        0 => A {},
        1 => B { x: u8 },
        2 => C { y: u16 },
        _ => D { data: bytes[remaining] },
    }"#,
    );
    assert_eq!(l.frames[0].variants.len(), 4);
}

#[test]
fn frame_variant_fields_correct() {
    let l = layout(
        r#"frame F = match t: u8 {
        0 => A {},
        1 => B { x: u8, y: u16 },
        _ => C { data: bytes[remaining] },
    }"#,
    );
    assert_eq!(l.frames[0].variants[0].fields.len(), 0);
    assert_eq!(l.frames[0].variants[1].fields.len(), 2);
    assert_eq!(l.frames[0].variants[2].fields.len(), 1);
}

// ── Capsule layout ──

#[test]
fn capsule_header_and_variant_fields() {
    let l = layout(
        r#"capsule C {
        type_field: u8, length: u16,
        payload: match type_field within length {
            0 => D { x: u8, y: u16 },
            _ => U { data: bytes[remaining] },
        },
    }"#,
    );
    assert_eq!(l.capsules[0].header_fields.len(), 2);
    assert_eq!(l.capsules[0].variants[0].fields.len(), 2);
    assert_eq!(l.capsules[0].variants[1].fields.len(), 1);
}

#[test]
fn capsule_variant_count() {
    let l = layout(
        r#"capsule C {
        type_field: u8, length: u16,
        payload: match type_field within length {
            0 => A { data: bytes[remaining] },
            1 => B { data: bytes[remaining] },
            2 => C { data: bytes[remaining] },
            _ => D { data: bytes[remaining] },
        },
    }"#,
    );
    assert_eq!(l.capsules[0].variants.len(), 4);
}

// ── Derived/require preserved ──

#[test]
fn layout_preserves_items_count() {
    let l = layout("packet P { x: u8, require x > 0, let y: u64 = x + 1 }");
    assert_eq!(l.packets[0].items.len(), 3);
}

#[test]
fn layout_preserves_derived_count() {
    let l = layout("packet P { x: u8, let a: bool = x != 0, let b: u64 = x + 1 }");
    assert_eq!(l.packets[0].derived.len(), 2);
}

#[test]
fn layout_preserves_require_count() {
    let l = layout("packet P { x: u8, y: u8, require x > 0, require y < 100 }");
    assert_eq!(l.packets[0].requires.len(), 2);
}

// ── Pass-through declarations ──

#[test]
fn consts_preserved_in_layout() {
    let l = layout("const MAX: u8 = 255\nconst MIN: u8 = 0\npacket P { x: u8 }");
    assert_eq!(l.consts.len(), 2);
}

#[test]
fn enums_preserved_in_layout() {
    let l = layout("enum E: u8 { A = 0, B = 1 }\nenum F: u16 { X = 10 }\npacket P { x: u8 }");
    assert_eq!(l.enums.len(), 2);
}

#[test]
fn varints_preserved_in_layout() {
    let src = r#"type V = { prefix: bits[2], value: match prefix { 0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62] } }
    packet P { x: V }"#;
    let l = layout(src);
    assert_eq!(l.varints.len(), 1);
}

#[test]
fn state_machines_preserved_in_layout() {
    let src = r#"state machine S {
        state A
        state B [terminal]
        initial A
        transition A -> B { on done }
    }"#;
    let l = layout(src);
    assert_eq!(l.state_machines.len(), 1);
}

// ── Schema version ──

#[test]
fn schema_version_correct() {
    let l = layout("packet P { x: u8 }");
    assert_eq!(l.schema_version, "layout-ir/v1");
}

// ── Multiple packets ──

#[test]
fn multiple_packets_lowered() {
    let l = layout("packet A { x: u8 }\npacket B { y: u16 }\npacket C { z: u32 }");
    assert_eq!(l.packets.len(), 3);
    assert_eq!(l.packets[0].name, "A");
    assert_eq!(l.packets[1].name, "B");
    assert_eq!(l.packets[2].name, "C");
}

// ── Optional field wire width ──

#[test]
fn optional_field_retains_wire_width() {
    let l = layout("packet P { flags: u8, extra: if flags & 0x01 { u16 } }");
    assert_eq!(l.packets[0].fields[1].wire_width_bits, Some(16));
}

// ── Mixed field types in single packet ──

#[test]
fn mixed_field_types() {
    let l = layout("packet P { a: u8, b: u16, c: u32, d: bytes[remaining] }");
    assert_eq!(l.packets[0].fields[0].wire_width_bits, Some(8));
    assert_eq!(l.packets[0].fields[1].wire_width_bits, Some(16));
    assert_eq!(l.packets[0].fields[2].wire_width_bits, Some(32));
    assert!(l.packets[0].fields[3].wire_width_bits.is_none());
}

// ── Empty packet ──

#[test]
fn empty_frame_variant() {
    let l = layout(
        r#"frame F = match t: u8 {
        0 => Empty {},
        _ => Other { data: bytes[remaining] },
    }"#,
    );
    assert_eq!(l.frames[0].variants[0].fields.len(), 0);
}
