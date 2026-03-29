// crates/wirespec-layout/tests/lower_tests.rs
use wirespec_layout::lower::lower;
use wirespec_sema::ComplianceProfile;
use wirespec_sema::analyze;
use wirespec_sema::types::Endianness;

fn analyze_and_lower(src: &str) -> wirespec_layout::ir::LayoutModule {
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    lower(&sem).unwrap()
}

#[test]
fn lower_empty_module() {
    let layout = analyze_and_lower("module test");
    assert_eq!(layout.module_name, "test");
    assert_eq!(layout.schema_version, "layout-ir/v1");
}

#[test]
fn lower_simple_packet() {
    let layout = analyze_and_lower("packet P { x: u8, y: u16 }");
    assert_eq!(layout.packets.len(), 1);
    assert_eq!(layout.packets[0].fields.len(), 2);
    // u8 has no endianness, 8-bit width
    assert_eq!(layout.packets[0].fields[0].wire_width_bits, Some(8));
    assert!(layout.packets[0].fields[0].endianness.is_none());
    // u16 gets module default endianness (big)
    assert_eq!(layout.packets[0].fields[1].wire_width_bits, Some(16));
    assert_eq!(
        layout.packets[0].fields[1].endianness,
        Some(Endianness::Big)
    );
}

#[test]
fn lower_explicit_endian() {
    let layout = analyze_and_lower("packet P { x: u16le, y: u32be }");
    assert_eq!(
        layout.packets[0].fields[0].endianness,
        Some(Endianness::Little)
    );
    assert_eq!(
        layout.packets[0].fields[1].endianness,
        Some(Endianness::Big)
    );
}

#[test]
fn lower_module_endian_little() {
    let layout = analyze_and_lower("@endian little\nmodule test\npacket P { x: u16 }");
    assert_eq!(layout.module_endianness, Endianness::Little);
    assert_eq!(
        layout.packets[0].fields[0].endianness,
        Some(Endianness::Little)
    );
}

#[test]
fn lower_bitgroup_packet() {
    let layout = analyze_and_lower("packet P { a: bits[4], b: bits[4], c: u16 }");
    assert_eq!(layout.packets[0].bitgroups.len(), 1);
    assert_eq!(layout.packets[0].bitgroups[0].total_bits, 8);
    assert_eq!(layout.packets[0].bitgroups[0].members.len(), 2);
    // Fields a, b should have bitgroup_member set
    assert!(layout.packets[0].fields[0].bitgroup_member.is_some());
    assert!(layout.packets[0].fields[1].bitgroup_member.is_some());
    assert!(layout.packets[0].fields[2].bitgroup_member.is_none());
}

#[test]
fn lower_bit_single() {
    let layout =
        analyze_and_lower("packet P { a: bits[4], b: bits[4], c: bit, d: bit, e: bits[6] }");
    // All fields are bit-type: bits[4]+bits[4]+bit+bit+bits[6] = 16 bits -> 1 group
    // (bit parses as Bits { width_bits: 1 }, so no non-bit field breaks the run)
    assert_eq!(layout.packets[0].bitgroups.len(), 1);
    assert_eq!(layout.packets[0].bitgroups[0].total_bits, 16);
    assert_eq!(layout.packets[0].bitgroups[0].members.len(), 5);
}

#[test]
fn lower_bytes_no_width() {
    let layout = analyze_and_lower("packet P { data: bytes[remaining] }");
    // bytes has no fixed wire width
    assert!(layout.packets[0].fields[0].wire_width_bits.is_none());
    assert!(layout.packets[0].fields[0].endianness.is_none());
}

#[test]
fn lower_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x01 => B { x: u16 },
            _ => C { data: bytes[remaining] },
        }
    "#;
    let layout = analyze_and_lower(src);
    assert_eq!(layout.frames.len(), 1);
    assert_eq!(layout.frames[0].variants.len(), 3);
    // tag_endianness: u8 has no endianness
    assert!(layout.frames[0].tag_endianness.is_none());
}

#[test]
fn lower_capsule() {
    let src = r#"
        capsule C {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let layout = analyze_and_lower(src);
    assert_eq!(layout.capsules.len(), 1);
    assert_eq!(layout.capsules[0].header_fields.len(), 2);
    assert_eq!(layout.capsules[0].variants.len(), 2);
}

#[test]
fn lower_consts_enums_pass_through() {
    let src = r#"
        const MAX: u8 = 20
        enum E: u8 { A = 0, B = 1 }
        packet P { x: u8 }
    "#;
    let layout = analyze_and_lower(src);
    assert_eq!(layout.consts.len(), 1);
    assert_eq!(layout.enums.len(), 1);
}

#[test]
fn lower_derived_requires_pass_through() {
    let src = "packet P { flags: u8, let is_set: bool = (flags & 1) != 0, require flags > 0 }";
    let layout = analyze_and_lower(src);
    assert_eq!(layout.packets[0].derived.len(), 1);
    assert_eq!(layout.packets[0].requires.len(), 1);
    assert_eq!(layout.packets[0].items.len(), 3); // field + derived + require
}

// ── Optional/Conditional field lowering ──
#[test]
fn lower_optional_field() {
    let layout = analyze_and_lower("packet P { flags: u8, extra: if flags & 0x01 { u16 } }");
    assert_eq!(layout.packets[0].fields.len(), 2);
    // Optional field should still have wire_width (it's u16 when present)
    assert_eq!(layout.packets[0].fields[1].wire_width_bits, Some(16));
}

#[test]
fn lower_array_field() {
    let layout = analyze_and_lower("packet P { count: u16, items: [u8; count] }");
    assert_eq!(layout.packets[0].fields.len(), 2);
    // Array has no fixed wire width
    assert!(layout.packets[0].fields[1].wire_width_bits.is_none());
}

#[test]
fn lower_bytes_length_field() {
    let layout = analyze_and_lower("packet P { len: u16, data: bytes[length: len] }");
    assert!(layout.packets[0].fields[1].wire_width_bits.is_none());
    assert!(layout.packets[0].fields[1].endianness.is_none());
}

#[test]
fn lower_bytes_fixed_field() {
    let layout = analyze_and_lower("packet P { mac: bytes[6] }");
    assert!(layout.packets[0].fields[0].wire_width_bits.is_none());
}

#[test]
fn lower_u8_no_endianness() {
    let layout = analyze_and_lower("packet P { x: u8 }");
    assert!(layout.packets[0].fields[0].endianness.is_none());
    assert_eq!(layout.packets[0].fields[0].wire_width_bits, Some(8));
}

#[test]
fn lower_u32_has_endianness() {
    let layout = analyze_and_lower("packet P { x: u32 }");
    assert!(layout.packets[0].fields[0].endianness.is_some());
    assert_eq!(layout.packets[0].fields[0].wire_width_bits, Some(32));
}

#[test]
fn lower_i8_no_endianness() {
    let layout = analyze_and_lower("packet P { x: i8 }");
    assert!(layout.packets[0].fields[0].endianness.is_none());
}

#[test]
fn lower_i32_has_endianness() {
    let layout = analyze_and_lower("packet P { x: i32 }");
    assert!(layout.packets[0].fields[0].endianness.is_some());
}

// ── Bitgroup edge cases ──
#[test]
fn lower_two_separate_bitgroups() {
    // bits[4]+bits[4] | u8 | bits[8] → 2 groups
    let layout = analyze_and_lower("packet P { a: bits[4], b: bits[4], middle: u8, c: bits[8] }");
    assert_eq!(layout.packets[0].bitgroups.len(), 2);
    assert_eq!(layout.packets[0].bitgroups[0].total_bits, 8);
    assert_eq!(layout.packets[0].bitgroups[1].total_bits, 8);
}

#[test]
fn lower_16bit_bitgroup() {
    let layout = analyze_and_lower("packet P { a: bits[4], b: bits[12] }");
    assert_eq!(layout.packets[0].bitgroups.len(), 1);
    assert_eq!(layout.packets[0].bitgroups[0].total_bits, 16);
}

#[test]
fn lower_32bit_bitgroup() {
    let layout = analyze_and_lower("packet P { a: bits[4], b: bits[12], c: bits[16] }");
    assert_eq!(layout.packets[0].bitgroups.len(), 1);
    assert_eq!(layout.packets[0].bitgroups[0].total_bits, 32);
}

#[test]
fn lower_frame_variant_fields() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A { x: u8, y: u16 },
            0x01 => B { data: bytes[remaining] },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let layout = analyze_and_lower(src);
    assert_eq!(layout.frames[0].variants[0].fields.len(), 2);
    assert_eq!(layout.frames[0].variants[1].fields.len(), 1);
}

#[test]
fn lower_frame_variant_with_bitgroup() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A { a: bits[4], b: bits[4], c: u16 },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let layout = analyze_and_lower(src);
    assert_eq!(layout.frames[0].variants[0].bitgroups.len(), 1);
}

#[test]
fn lower_capsule_header_bitgroup() {
    let src = r#"
        capsule C {
            flags: bits[4],
            type_field: bits[4],
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => U { data: bytes[remaining] },
            },
        }
    "#;
    let layout = analyze_and_lower(src);
    assert!(!layout.capsules[0].header_bitgroups.is_empty());
}

// ── Derived/Require preservation ──
#[test]
fn lower_packet_items_order_preserved() {
    let src = "packet P { x: u8, require x > 0, let y: bool = x != 0 }";
    let layout = analyze_and_lower(src);
    assert_eq!(layout.packets[0].items.len(), 3);
}

// ── Varint / enum pass through ──
#[test]
fn lower_varint_preserved() {
    let src = r#"
        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6], 0b01 => bits[14],
                0b10 => bits[30], 0b11 => bits[62],
            },
        }
    "#;
    let layout = analyze_and_lower(src);
    assert_eq!(layout.varints.len(), 1);
}
