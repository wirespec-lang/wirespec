// crates/wirespec-layout/tests/bitgroup_tests.rs
use wirespec_layout::bitgroup::*;
use wirespec_layout::ir::*;
use wirespec_sema::types::Endianness;

#[test]
fn no_bit_fields_no_groups() {
    let fields = vec![
        mock_layout_field("x", None, "scope"), // u8, no bit_width
        mock_layout_field("y", None, "scope"),
    ];
    let (groups, updated) = detect_bitgroups(&fields, "scope", Endianness::Big).unwrap();
    assert!(groups.is_empty());
    assert!(updated.iter().all(|f| f.bitgroup_member.is_none()));
}

#[test]
fn single_byte_bitgroup() {
    // bits[4] + bits[4] = 8 bits -> 1 bitgroup
    let fields = vec![
        mock_layout_field("a", Some(4), "scope"),
        mock_layout_field("b", Some(4), "scope"),
    ];
    let (groups, _updated) = detect_bitgroups(&fields, "scope", Endianness::Big).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].total_bits, 8);
    assert_eq!(groups[0].members.len(), 2);
    // Spec S10: members ordered by increasing offset_bits
    // Big-endian: field "b" at offset 0 (LSB), field "a" at offset 4 (MSB)
    assert_eq!(groups[0].members[0].offset_bits, 0); // "b"
    assert_eq!(groups[0].members[1].offset_bits, 4); // "a"
}

#[test]
fn little_endian_bitgroup() {
    let fields = vec![
        mock_layout_field("a", Some(4), "scope"),
        mock_layout_field("b", Some(4), "scope"),
    ];
    let (groups, _updated) = detect_bitgroups(&fields, "scope", Endianness::Little).unwrap();
    assert_eq!(groups.len(), 1);
    // Little-endian: first field at offset 0 (LSB), second at offset 4
    assert_eq!(groups[0].members[0].offset_bits, 0);
    assert_eq!(groups[0].members[1].offset_bits, 4);
}

#[test]
fn two_byte_bitgroup() {
    // bits[6] + bits[2] + bits[8] = 16 bits
    let fields = vec![
        mock_layout_field("a", Some(6), "scope"),
        mock_layout_field("b", Some(2), "scope"),
        mock_layout_field("c", Some(8), "scope"),
    ];
    let (groups, _) = detect_bitgroups(&fields, "scope", Endianness::Big).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].total_bits, 16);
}

#[test]
fn non_bit_field_breaks_group() {
    // bits[4] + bits[4] | u16 | bits[8]
    let fields = vec![
        mock_layout_field("a", Some(4), "scope"),
        mock_layout_field("b", Some(4), "scope"),
        mock_layout_field("middle", None, "scope"), // u16 breaks
        mock_layout_field("c", Some(8), "scope"),
    ];
    let (groups, _) = detect_bitgroups(&fields, "scope", Endianness::Big).unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].total_bits, 8);
    assert_eq!(groups[1].total_bits, 8);
}

#[test]
fn unaligned_bitgroup_error() {
    // bits[3] alone = 3 bits (not multiple of 8)
    let fields = vec![
        mock_layout_field("a", Some(3), "scope"),
        mock_layout_field("next", None, "scope"),
    ];
    let result = detect_bitgroups(&fields, "scope", Endianness::Big);
    assert!(result.is_err());
}

#[test]
fn bitgroup_member_refs_set() {
    let fields = vec![
        mock_layout_field("a", Some(4), "scope"),
        mock_layout_field("b", Some(4), "scope"),
    ];
    let (groups, updated) = detect_bitgroups(&fields, "scope", Endianness::Big).unwrap();
    // Fields should have bitgroup_member set
    assert!(updated[0].bitgroup_member.is_some());
    assert!(updated[1].bitgroup_member.is_some());
    let ref0 = updated[0].bitgroup_member.as_ref().unwrap();
    assert_eq!(ref0.bitgroup_id, groups[0].bitgroup_id);
    assert_eq!(ref0.width_bits, 4);
}

#[test]
fn four_byte_bitgroup() {
    // bits[4] + bits[12] + bits[16] = 32 bits
    let fields = vec![
        mock_layout_field("a", Some(4), "scope"),
        mock_layout_field("b", Some(12), "scope"),
        mock_layout_field("c", Some(16), "scope"),
    ];
    let (groups, _) = detect_bitgroups(&fields, "scope", Endianness::Big).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].total_bits, 32);
}

// Helper
fn mock_layout_field(name: &str, wire_width_bits: Option<u16>, _scope: &str) -> LayoutField {
    // Use SemanticType::Bits for bit-width fields so is_bit_field() recognizes them,
    // and SemanticType::Primitive { wire: U8 } for non-bit fields.
    let ty = if let Some(w) = wire_width_bits {
        wirespec_sema::types::SemanticType::Bits { width_bits: w }
    } else {
        wirespec_sema::types::SemanticType::Primitive {
            wire: wirespec_sema::types::PrimitiveWireType::U8,
            endianness: None,
        }
    };
    LayoutField {
        field_id: format!("test.field:{name}"),
        name: name.to_string(),
        ty,
        presence: wirespec_sema::types::FieldPresence::Always,
        max_elements: None,
        checksum_algorithm: None,
        wire_width_bits,
        endianness: None,
        bitgroup_member: None,
        asn1_hint: None,
        span: None,
    }
}

/// Helper that creates a field using Primitive { Bit } type (single-bit field)
/// when `is_bit` is true, and normal Bits { width_bits } otherwise.
fn mock_layout_field_with_type(
    name: &str,
    wire_width_bits: Option<u16>,
    _scope: &str,
    is_bit: bool,
) -> LayoutField {
    let ty = if is_bit {
        wirespec_sema::types::SemanticType::Primitive {
            wire: wirespec_sema::types::PrimitiveWireType::Bit,
            endianness: None,
        }
    } else if let Some(w) = wire_width_bits {
        wirespec_sema::types::SemanticType::Bits { width_bits: w }
    } else {
        wirespec_sema::types::SemanticType::Primitive {
            wire: wirespec_sema::types::PrimitiveWireType::U8,
            endianness: None,
        }
    };
    LayoutField {
        field_id: format!("test.field:{name}"),
        name: name.to_string(),
        ty,
        presence: wirespec_sema::types::FieldPresence::Always,
        max_elements: None,
        checksum_algorithm: None,
        wire_width_bits,
        endianness: None,
        bitgroup_member: None,
        asn1_hint: None,
        span: None,
    }
}

#[test]
fn single_bit_field_in_group() {
    // bit + bits[7] = 8 bits
    let fields = vec![
        mock_layout_field_with_type("flag", Some(1), "scope", true),
        mock_layout_field("rest", Some(7), "scope"),
    ];
    let (groups, _) = detect_bitgroups(&fields, "scope", Endianness::Big).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].total_bits, 8);
}

#[test]
fn exactly_64_bits() {
    let fields = vec![
        mock_layout_field("a", Some(32), "scope"),
        mock_layout_field("b", Some(32), "scope"),
    ];
    let (groups, _) = detect_bitgroups(&fields, "scope", Endianness::Big).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].total_bits, 64);
}

#[test]
fn over_64_bits_error() {
    let fields = vec![
        mock_layout_field("a", Some(32), "scope"),
        mock_layout_field("b", Some(33), "scope"),
        mock_layout_field("next", None, "scope"),
    ];
    let result = detect_bitgroups(&fields, "scope", Endianness::Big);
    assert!(result.is_err());
}

#[test]
fn bitgroup_id_format() {
    let fields = vec![
        mock_layout_field("a", Some(4), "packet:Foo"),
        mock_layout_field("b", Some(4), "packet:Foo"),
    ];
    let (groups, _) = detect_bitgroups(&fields, "packet:Foo", Endianness::Big).unwrap();
    assert_eq!(groups[0].bitgroup_id, "packet:Foo.bitgroup[0]");
    assert_eq!(groups[0].scope_id, "packet:Foo");
}

#[test]
fn multiple_groups_sequential_ids() {
    let fields = vec![
        mock_layout_field("a", Some(8), "scope"),
        mock_layout_field("mid", None, "scope"),
        mock_layout_field("b", Some(8), "scope"),
    ];
    let (groups, _) = detect_bitgroups(&fields, "scope", Endianness::Big).unwrap();
    assert_eq!(groups.len(), 2);
    assert!(groups[0].bitgroup_id.contains("bitgroup[0]"));
    assert!(groups[1].bitgroup_id.contains("bitgroup[1]"));
}
