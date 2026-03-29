//! Parser tests for wirespec-syntax.
//!
//! Covers all major language constructs against wirespec_spec_v1.0 grammar §6.1/§6.2.

use wirespec_syntax::ast::*;
use wirespec_syntax::parse;

// ═══════════════════════════════════════════════════════════════════════════
// Module / Import
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_empty() {
    let m = parse("").unwrap();
    assert!(m.module_decl.is_none());
    assert!(m.imports.is_empty());
    assert!(m.items.is_empty());
}

#[test]
fn parse_module_decl() {
    let m = parse("module quic.varint").unwrap();
    let decl = m.module_decl.unwrap();
    assert_eq!(decl.name, "quic.varint");
}

#[test]
fn parse_import() {
    let m = parse("import quic.varint.VarInt").unwrap();
    assert_eq!(m.imports.len(), 1);
    assert_eq!(m.imports[0].module, "quic.varint");
    assert_eq!(m.imports[0].name.as_deref(), Some("VarInt"));
}

#[test]
fn parse_import_whole_module() {
    let m = parse("import quic.varint").unwrap();
    assert_eq!(m.imports[0].module, "quic.varint");
    assert!(m.imports[0].name.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Annotations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_annotation_bare() {
    let m = parse("@strict\npacket Foo {}").unwrap();
    // @strict is a file-level annotation when before packet but with no args...
    // Actually it should attach to the packet. Let me check our parser logic.
    // In our parser, annotations are collected before top items.
    assert_eq!(m.items.len(), 1);
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.annotations.len(), 1);
            assert_eq!(p.annotations[0].name, "strict");
            assert!(p.annotations[0].args.is_empty());
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_annotation_with_name_arg() {
    // With module decl, @endian goes to file-level annotations
    let m = parse("@endian big\nmodule test\npacket Foo {}").unwrap();
    assert_eq!(m.annotations.len(), 1);
    assert_eq!(m.annotations[0].name, "endian");
    assert_eq!(
        m.annotations[0].args,
        vec![AstAnnotationArg::Identifier("big".into())]
    );
}

#[test]
fn parse_annotation_before_module() {
    // Without module decl, annotations flow to first item
    let m = parse("@endian big\npacket Foo {}").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.annotations.len(), 1);
            assert_eq!(p.annotations[0].name, "endian");
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_annotation_parenthesized() {
    let m = parse("@checksum(internet)\npacket Foo { x: u8 }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.annotations[0].name, "checksum");
            assert_eq!(
                p.annotations[0].args,
                vec![AstAnnotationArg::Identifier("internet".into())]
            );
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_annotation_max_len() {
    let m = parse("packet Foo { @max_len(1024)\nitems: u8 }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert_eq!(f.annotations[0].name, "max_len");
                assert_eq!(f.annotations[0].args, vec![AstAnnotationArg::Int(1024)]);
            } else {
                panic!("expected field");
            }
        }
        _ => panic!("expected packet"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Const / Enum / Flags / StaticAssert
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_const() {
    let m = parse("const MAX_CID_LENGTH: u8 = 20").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => {
            assert_eq!(c.name, "MAX_CID_LENGTH");
            assert_eq!(c.type_name, "u8");
            assert_eq!(c.value, AstLiteralValue::Int(20));
            assert!(!c.exported);
        }
        _ => panic!("expected const"),
    }
}

#[test]
fn parse_const_hex() {
    let m = parse("const VERSION: u32 = 0x00000001").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => {
            assert_eq!(c.value, AstLiteralValue::Int(1));
        }
        _ => panic!("expected const"),
    }
}

#[test]
fn parse_enum() {
    let m = parse(
        "enum FrameType: u8 {
            Padding = 0x00,
            Ping = 0x01,
            Crypto = 0x06,
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Enum(e) => {
            assert_eq!(e.name, "FrameType");
            assert_eq!(e.underlying_type, "u8");
            assert_eq!(e.members.len(), 3);
            assert_eq!(e.members[0].name, "Padding");
            assert_eq!(e.members[0].value, 0x00);
            assert_eq!(e.members[2].name, "Crypto");
            assert_eq!(e.members[2].value, 0x06);
        }
        _ => panic!("expected enum"),
    }
}

#[test]
fn parse_flags() {
    let m = parse(
        "flags PacketFlags: u8 {
            KeyPhase = 0x04,
            SpinBit = 0x20,
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Flags(f) => {
            assert_eq!(f.name, "PacketFlags");
            assert_eq!(f.members.len(), 2);
        }
        _ => panic!("expected flags"),
    }
}

#[test]
fn parse_static_assert() {
    let m = parse("static_assert MAX_CID_LENGTH <= 255").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary { op, .. } = &sa.expr {
                assert_eq!(*op, BinOp::Le);
            } else {
                panic!("expected binary expr");
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn parse_export_const() {
    let m = parse("export const VERSION: u32 = 1").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => {
            assert!(c.exported);
        }
        _ => panic!("expected const"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Type definitions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_type_alias() {
    let m = parse("type AttHandle = u16le").unwrap();
    match &m.items[0] {
        AstTopItem::Type(t) => {
            assert_eq!(t.name, "AttHandle");
            match &t.body {
                AstTypeDeclBody::Alias { target } => {
                    assert!(matches!(target, AstTypeExpr::Named { name, .. } if name == "u16le"));
                }
                _ => panic!("expected alias"),
            }
        }
        _ => panic!("expected type"),
    }
}

#[test]
fn parse_computed_type() {
    let m = parse(
        "type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6],
                0b01 => bits[14],
                0b10 => bits[30],
                0b11 => bits[62],
            },
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Type(t) => {
            assert_eq!(t.name, "VarInt");
            match &t.body {
                AstTypeDeclBody::Fields { fields } => {
                    assert_eq!(fields.len(), 2);
                    assert_eq!(fields[0].name, "prefix");
                    assert!(matches!(
                        &fields[0].type_expr,
                        AstTypeExpr::Bits { width: 2, .. }
                    ));
                    assert!(matches!(&fields[1].type_expr, AstTypeExpr::Match { .. }));
                }
                _ => panic!("expected fields"),
            }
        }
        _ => panic!("expected type"),
    }
}

#[test]
fn parse_continuation_varint() {
    let m = parse(
        "type MqttLength = varint {
            continuation_bit: msb,
            value_bits: 7,
            max_bytes: 4,
            byte_order: little,
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::ContinuationVarInt(v) => {
            assert_eq!(v.name, "MqttLength");
            assert_eq!(v.continuation_bit, "msb");
            assert_eq!(v.value_bits, 7);
            assert_eq!(v.max_bytes, 4);
            assert_eq!(v.byte_order, "little");
        }
        _ => panic!("expected continuation varint"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Packet
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_simple_packet() {
    let m = parse(
        "packet UdpDatagram {
            src_port: u16,
            dst_port: u16,
            length: u16,
            checksum: u16,
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.name, "UdpDatagram");
            assert_eq!(p.fields.len(), 4);
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert_eq!(f.name, "src_port");
                assert!(matches!(&f.type_expr, AstTypeExpr::Named { name, .. } if name == "u16"));
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_packet_with_require() {
    let m = parse(
        "packet UdpDatagram {
            src_port: u16,
            length: u16,
            require length >= 8,
            data: bytes[length: length - 8],
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.fields.len(), 4);
            assert!(matches!(&p.fields[2], AstFieldItem::Require(_)));
            if let AstFieldItem::Field(f) = &p.fields[3] {
                assert_eq!(f.name, "data");
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::Length,
                        ..
                    }
                ));
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_packet_with_bits() {
    let m = parse(
        "packet BitTest {
            a4: bits[4],
            b4: bits[4],
            middle: u16,
            flag: bit,
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.fields.len(), 4);
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert!(matches!(&f.type_expr, AstTypeExpr::Bits { width: 4, .. }));
            }
            if let AstFieldItem::Field(f) = &p.fields[3] {
                assert!(matches!(&f.type_expr, AstTypeExpr::Bits { width: 1, .. }));
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_packet_with_derived_field() {
    let m = parse(
        "packet Foo {
            flags: u8,
            let is_set: bool = (flags & 0x01) != 0,
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Derived(d) = &p.fields[1] {
                assert_eq!(d.name, "is_set");
                assert_eq!(d.type_name, "bool");
            } else {
                panic!("expected derived");
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_packet_with_optional_field() {
    let m = parse(
        "packet Foo {
            flags: u8,
            extra: if flags & 0x01 { u16 },
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[1] {
                assert!(matches!(&f.type_expr, AstTypeExpr::Optional { .. }));
            } else {
                panic!("expected field");
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_packet_with_array() {
    let m = parse(
        "packet Foo {
            count: u16,
            items: [u8; count],
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[1] {
                if let AstTypeExpr::Array { count, .. } = &f.type_expr {
                    assert!(
                        matches!(count, AstArrayCount::Expr(AstExpr::NameRef { name, .. }) if name == "count")
                    );
                } else {
                    panic!("expected array type");
                }
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_packet_with_fill_array() {
    let m = parse(
        "packet Foo {
            items: [u8; fill],
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                if let AstTypeExpr::Array { count, .. } = &f.type_expr {
                    assert!(matches!(count, AstArrayCount::Fill));
                }
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_packet_bytes_remaining() {
    let m = parse(
        "packet Foo {
            data: bytes[remaining],
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::Remaining,
                        ..
                    }
                ));
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_packet_bytes_fixed() {
    let m = parse(
        "packet Foo {
            mac: bytes[6],
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::Fixed,
                        fixed_size: Some(6),
                        ..
                    }
                ));
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_packet_bytes_length_or_remaining() {
    let m = parse(
        "packet Foo {
            len: if flags & 0x02 { u16 },
            data: bytes[length_or_remaining: len],
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[1] {
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::LengthOrRemaining,
                        ..
                    }
                ));
            }
        }
        _ => panic!("expected packet"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Frame
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_frame() {
    let m = parse(
        "frame AttPdu = match opcode: u8 {
            0x01 => ErrorRsp { request_opcode: u8 },
            0x02 => MtuReq { client_mtu: u16 },
            0x0b => ReadRsp { value: bytes[remaining] },
            _ => Unknown { data: bytes[remaining] },
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => {
            assert_eq!(f.name, "AttPdu");
            assert_eq!(f.tag_field, "opcode");
            assert_eq!(f.tag_type, "u8");
            assert_eq!(f.branches.len(), 4);
            assert_eq!(f.branches[0].variant_name, "ErrorRsp");
            assert!(matches!(
                &f.branches[0].pattern,
                AstPattern::Value { value: 1, .. }
            ));
            assert!(matches!(
                &f.branches[3].pattern,
                AstPattern::Wildcard { .. }
            ));
        }
        _ => panic!("expected frame"),
    }
}

#[test]
fn parse_frame_with_range_pattern() {
    let m = parse(
        "frame F = match tag: u8 {
            0x02..=0x03 => Ack { x: u8 },
            0x08..=0x0f => Stream { y: u16 },
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => {
            assert!(matches!(
                &f.branches[0].pattern,
                AstPattern::RangeInclusive {
                    start: 2,
                    end: 3,
                    ..
                }
            ));
            assert!(matches!(
                &f.branches[1].pattern,
                AstPattern::RangeInclusive {
                    start: 8,
                    end: 15,
                    ..
                }
            ));
        }
        _ => panic!("expected frame"),
    }
}

#[test]
fn parse_frame_empty_variant() {
    let m = parse(
        "frame F = match tag: u8 {
            0x00 => Padding {},
            0x01 => Ping {},
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => {
            assert!(f.branches[0].fields.is_empty());
            assert!(f.branches[1].fields.is_empty());
        }
        _ => panic!("expected frame"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Capsule
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_capsule_simple() {
    let m = parse(
        "capsule TlvPacket {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0x01 => Data { data: bytes[remaining] },
                _ => Unknown { data: bytes[remaining] },
            },
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Capsule(c) => {
            assert_eq!(c.name, "TlvPacket");
            assert_eq!(c.fields.len(), 2); // header fields
            assert!(matches!(
                &c.payload_tag,
                AstPayloadTagSelector::Field { field_name } if field_name == "type_field"
            ));
            assert_eq!(c.payload_within, "length");
            assert_eq!(c.branches.len(), 2);
        }
        _ => panic!("expected capsule"),
    }
}

#[test]
fn parse_capsule_expr_tag() {
    let m = parse(
        "capsule MqttPacket {
            type_and_flags: u8,
            remaining_length: u16,
            payload: match (type_and_flags >> 4) within remaining_length {
                1 => Connect { data: bytes[remaining] },
                _ => Unknown { data: bytes[remaining] },
            },
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Capsule(c) => {
            assert!(matches!(&c.payload_tag, AstPayloadTagSelector::Expr { .. }));
        }
        _ => panic!("expected capsule"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Expressions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_expr_arithmetic() {
    let m = parse("static_assert 1 + 2 * 3").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            // Should be 1 + (2 * 3), i.e. Add(1, Mul(2, 3))
            if let AstExpr::Binary {
                op: BinOp::Add,
                left,
                right,
                ..
            } = &sa.expr
            {
                assert!(matches!(**left, AstExpr::Int { value: 1, .. }));
                assert!(matches!(**right, AstExpr::Binary { op: BinOp::Mul, .. }));
            } else {
                panic!("expected binary add: {:?}", sa.expr);
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn parse_expr_bitwise_binds_tighter_than_comparison() {
    // wirespec: a & mask == 0 means (a & mask) == 0
    let m = parse("static_assert a & mask == 0").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary {
                op: BinOp::Eq,
                left,
                right,
                ..
            } = &sa.expr
            {
                assert!(matches!(
                    **left,
                    AstExpr::Binary {
                        op: BinOp::BitAnd,
                        ..
                    }
                ));
                assert!(matches!(**right, AstExpr::Int { value: 0, .. }));
            } else {
                panic!("expected eq with bitand lhs: {:?}", sa.expr);
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn parse_expr_coalesce() {
    let m = parse(
        "packet Foo {
            x: if true { u16 },
            let y: u64 = x ?? 0,
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Derived(d) = &p.fields[1] {
                assert!(matches!(&d.expr, AstExpr::Coalesce { .. }));
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_expr_member_access() {
    let m = parse("static_assert src.path_id == 0").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary { left, .. } = &sa.expr {
                assert!(matches!(**left, AstExpr::MemberAccess { .. }));
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn parse_expr_subscript() {
    let m = parse("static_assert paths[0] == 0").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary { left, .. } = &sa.expr {
                assert!(matches!(**left, AstExpr::Subscript { .. }));
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn parse_expr_state_constructor() {
    let m = parse("static_assert PathState::Active(0, 1, 2) != null").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary { left, .. } = &sa.expr {
                if let AstExpr::StateConstructor {
                    sm_name,
                    state_name,
                    args,
                    ..
                } = &**left
                {
                    assert_eq!(sm_name, "PathState");
                    assert_eq!(state_name, "Active");
                    assert_eq!(args.len(), 3);
                } else {
                    panic!("expected state constructor");
                }
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn parse_expr_fill() {
    let m = parse("static_assert fill(0, 4) != null").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary { left, .. } = &sa.expr {
                assert!(matches!(**left, AstExpr::Fill { .. }));
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn parse_expr_unary_not() {
    let m = parse("static_assert !(x == 0)").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            assert!(matches!(
                &sa.expr,
                AstExpr::Unary {
                    op: UnaryOp::Not,
                    ..
                }
            ));
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn parse_expr_unary_neg() {
    let m = parse("static_assert -1 < 0").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary { left, .. } = &sa.expr {
                assert!(matches!(
                    **left,
                    AstExpr::Unary {
                        op: UnaryOp::Neg,
                        ..
                    }
                ));
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn parse_expr_shift() {
    let m = parse("static_assert x >> 4 == 1").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            // Should be (x >> 4) == 1 because shift binds tighter than comparison
            if let AstExpr::Binary {
                op: BinOp::Eq,
                left,
                ..
            } = &sa.expr
            {
                assert!(matches!(**left, AstExpr::Binary { op: BinOp::Shr, .. }));
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn parse_expr_and_or() {
    let m = parse("static_assert a == 1 and b == 2 or c == 3").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            // or binds looser than and: (a==1 and b==2) or (c==3)
            assert!(matches!(&sa.expr, AstExpr::Binary { op: BinOp::Or, .. }));
        }
        _ => panic!("expected static_assert"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// State Machine
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_state_machine_basic() {
    let m = parse(
        "state machine PathState {
            state Init { path_id: u8 }
            state Active { path_id: u8, rtt: u64 = 0 }
            state Closed [terminal]

            initial Init

            transition Init -> Active {
                on activate(id: u8)
                action { dst.path_id = src.path_id; }
            }
            transition Active -> Closed { on close }
            transition * -> Closed { on error }
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            assert_eq!(sm.name, "PathState");
            assert_eq!(sm.states.len(), 3);
            assert_eq!(sm.initial_state, "Init");
            assert_eq!(sm.transitions.len(), 3);

            // Check states
            assert_eq!(sm.states[0].name, "Init");
            assert_eq!(sm.states[0].fields.len(), 1);
            assert!(!sm.states[0].is_terminal);

            assert_eq!(sm.states[1].name, "Active");
            assert_eq!(sm.states[1].fields.len(), 2);
            assert_eq!(
                sm.states[1].fields[1].default_value,
                Some(AstLiteralValue::Int(0))
            );

            assert_eq!(sm.states[2].name, "Closed");
            assert!(sm.states[2].is_terminal);
            assert!(sm.states[2].fields.is_empty());

            // Wildcard transition
            assert_eq!(sm.transitions[2].src_state, "*");
            assert_eq!(sm.transitions[2].dst_state, "Closed");
        }
        _ => panic!("expected state machine"),
    }
}

#[test]
fn parse_transition_with_guard() {
    let m = parse(
        "state machine S {
            state A { count: u8 = 0 }
            state B
            initial A
            transition A -> A {
                on tick
                guard src.count < 10
                action { dst.count = src.count + 1; }
            }
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            let t = &sm.transitions[0];
            assert!(t.guard.is_some());
            assert_eq!(t.actions.len(), 1);
            assert_eq!(t.actions[0].op, "=");
        }
        _ => panic!("expected state machine"),
    }
}

#[test]
fn parse_transition_with_delegate() {
    let m = parse(
        "state machine Parent {
            state Active { child: u8 }
            initial Active
            transition Active -> Active {
                on child_event(id: u8, event: u8)
                delegate src.child <- event
            }
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            let t = &sm.transitions[0];
            assert!(t.delegate.is_some());
            let d = t.delegate.as_ref().unwrap();
            assert_eq!(d.event_name, "event");
        }
        _ => panic!("expected state machine"),
    }
}

#[test]
fn parse_transition_plus_assign() {
    let m = parse(
        "state machine S {
            state A { count: u8 = 0 }
            initial A
            transition A -> A {
                on tick
                action { dst.count += 1; }
            }
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            assert_eq!(sm.transitions[0].actions[0].op, "+=");
        }
        _ => panic!("expected state machine"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Integration: real .wspec examples
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_udp_example() {
    let src = r#"
module net.udp
@endian big

packet UdpDatagram {
    src_port: u16,
    dst_port: u16,
    length: u16,
    checksum: u16,
    require length >= 8,
    data: bytes[length: length - 8],
}
"#;
    let m = parse(src).unwrap();
    assert_eq!(m.module_decl.as_ref().unwrap().name, "net.udp");
    assert_eq!(m.annotations.len(), 1); // @endian big
    assert_eq!(m.items.len(), 1);
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.name, "UdpDatagram");
            assert_eq!(p.fields.len(), 6);
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_bits_groups_example() {
    let src = r#"
module test.bits_groups
@endian big

packet BitTest {
    a4: bits[4],
    b4: bits[4],
    middle: u16,
    c6: bits[6],
    d2: bits[2],
    e8: bits[8],
    tail: u8,
}

packet BitTest32 {
    x4: bits[4],
    y12: bits[12],
    z16: bits[16],
}
"#;
    let m = parse(src).unwrap();
    assert_eq!(m.items.len(), 2);
}

#[test]
fn parse_quic_frames_subset() {
    let src = r#"
module quic.frames
@endian big

type VarInt = {
    prefix: bits[2],
    value: match prefix {
        0b00 => bits[6],
        0b01 => bits[14],
        0b10 => bits[30],
        0b11 => bits[62],
    },
}

const MAX_CID_LENGTH: u8 = 20

packet AckRange { gap: VarInt, ack_range: VarInt }

frame QuicFrame = match frame_type: VarInt {
    0x00 => Padding {},
    0x01 => Ping {},
    0x06 => Crypto {
        offset: VarInt,
        length: VarInt,
        data: bytes[length],
    },
    0x30..=0x31 => Datagram {
        length: if frame_type & 0x01 { VarInt },
        data: bytes[length_or_remaining: length],
    },
    _ => Unknown { data: bytes[remaining] },
}
"#;
    let m = parse(src).unwrap();
    assert_eq!(m.items.len(), 4); // VarInt, MAX_CID_LENGTH, AckRange, QuicFrame
}

// ═══════════════════════════════════════════════════════════════════════════
// Error Cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_unexpected_token() {
    let result = parse("packet { }");
    assert!(result.is_err());
}

#[test]
fn error_unterminated_string() {
    let result = parse(r#"const X: u8 = "hello"#);
    assert!(result.is_err());
}

#[test]
fn error_missing_brace() {
    let result = parse("packet Foo { x: u8");
    assert!(result.is_err());
}

#[test]
fn error_invalid_hex() {
    let result = parse("const X: u8 = 0xGG");
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// Comments
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_with_hash_comments() {
    let src = r#"
# This is a comment
packet Foo {
    # Field comment
    x: u8,
}
"#;
    let m = parse(src).unwrap();
    assert_eq!(m.items.len(), 1);
}

#[test]
fn parse_with_slash_comments() {
    let src = r#"
// This is a comment
packet Foo {
    // Field comment
    x: u8,
}
"#;
    let m = parse(src).unwrap();
    assert_eq!(m.items.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Span tracking
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn spans_are_populated() {
    let m = parse("packet Foo { x: u8 }").unwrap();
    assert!(m.span.is_some());
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert!(p.span.is_some());
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert!(f.span.is_some());
            }
        }
        _ => panic!("expected packet"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Wildcard pattern
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_wildcard_pattern() {
    let m = parse(
        "frame F = match tag: u8 {
            _ => Unknown { data: bytes[remaining] },
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => {
            assert!(matches!(
                &f.branches[0].pattern,
                AstPattern::Wildcard { .. }
            ));
        }
        _ => panic!("expected frame"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Multiple items
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_multiple_items() {
    let src = r#"
module test
const A: u8 = 1
const B: u16 = 0xFF
enum E: u8 { X = 0, Y = 1 }
packet P { x: u8 }
"#;
    let m = parse(src).unwrap();
    assert_eq!(m.items.len(), 4);
    assert!(matches!(&m.items[0], AstTopItem::Const(_)));
    assert!(matches!(&m.items[1], AstTopItem::Const(_)));
    assert!(matches!(&m.items[2], AstTopItem::Enum(_)));
    assert!(matches!(&m.items[3], AstTopItem::Packet(_)));
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Parser Error Cases (negative tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_missing_packet_name() {
    assert!(parse("packet { x: u8 }").is_err());
}

#[test]
fn error_missing_field_type() {
    assert!(parse("packet P { x }").is_err());
}

#[test]
fn error_missing_colon_in_field() {
    assert!(parse("packet P { x u8 }").is_err());
}

#[test]
fn error_unclosed_brace_packet() {
    assert!(parse("packet P { x: u8").is_err());
}

#[test]
fn error_unclosed_bracket_array() {
    assert!(parse("packet P { x: [u8; 4 }").is_err());
}

#[test]
fn error_double_comma() {
    // Double comma: after the first comma the parser tries to parse a field
    // item and sees another comma, which is not a valid field start.
    let result = parse("packet P { x: u8,, y: u16 }");
    assert!(result.is_err());
}

#[test]
fn error_empty_enum() {
    // Empty enum body — the parser allows it (0 members)
    assert!(parse("enum E: u8 {}").is_ok());
}

#[test]
fn error_missing_enum_value() {
    // Missing = value after enum member name
    assert!(parse("enum E: u8 { A }").is_err());
}

#[test]
fn error_frame_missing_match() {
    // Frame syntax requires "match" keyword after "="
    assert!(parse("frame F = tag: u8 { }").is_err());
}

#[test]
fn error_capsule_missing_within() {
    // Capsule payload match must have "within" keyword
    assert!(
        parse(
            "capsule C { t: u8, l: u16, payload: match t { 0 => D { data: bytes[remaining] } } }"
        )
        .is_err()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Expression Edge Cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn expr_deeply_nested_parens() {
    let m = parse("static_assert ((((1 + 2))))").unwrap();
    // Should parse without stack overflow
    assert!(matches!(&m.items[0], AstTopItem::StaticAssert(_)));
}

#[test]
fn expr_chained_member_access() {
    let m = parse("static_assert a.b.c == 0").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary { left, .. } = &sa.expr {
                assert!(matches!(**left, AstExpr::MemberAccess { .. }));
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn expr_subscript_with_expr_index() {
    let m = parse("static_assert arr[i + 1] == 0").unwrap();
    assert!(matches!(&m.items[0], AstTopItem::StaticAssert(_)));
}

#[test]
fn expr_complex_bitwise() {
    // (a & 0xFF) | (b << 8) — verifies precedence
    let m = parse("static_assert (a & 0xFF) | (b << 8) == 0").unwrap();
    assert!(matches!(&m.items[0], AstTopItem::StaticAssert(_)));
}

#[test]
fn expr_all_binary_ops() {
    // Test every binary operator parses
    for op in [
        "==", "!=", "<", "<=", ">", ">=", "+", "-", "*", "/", "%", "&", "|", "^", "<<", ">>",
    ] {
        let src = format!("static_assert a {op} b");
        parse(&src).unwrap_or_else(|e| panic!("failed to parse op {op}: {e}"));
    }
}

#[test]
fn expr_logical_and_or() {
    let m = parse("static_assert a == 1 and b == 2").unwrap();
    assert!(matches!(&m.items[0], AstTopItem::StaticAssert(_)));
    let m = parse("static_assert a == 1 or b == 2").unwrap();
    assert!(matches!(&m.items[0], AstTopItem::StaticAssert(_)));
}

#[test]
fn expr_state_constructor_no_args() {
    let m = parse("static_assert MyState::Closed != null").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary { left, .. } = &sa.expr {
                if let AstExpr::StateConstructor { args, .. } = &**left {
                    assert!(args.is_empty());
                } else {
                    panic!("expected state constructor, got {:?}", left);
                }
            } else {
                panic!("expected binary expr");
            }
        }
        _ => panic!("expected static_assert"),
    }
}

#[test]
fn expr_slice() {
    let m = parse("static_assert paths[0..count] != null").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary { left, .. } = &sa.expr {
                assert!(matches!(**left, AstExpr::Slice { .. }));
            } else {
                panic!("expected binary expr");
            }
        }
        _ => panic!("expected static_assert"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Integer Literal Edge Cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn literal_zero() {
    let m = parse("const X: u8 = 0").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(0)),
        _ => panic!("expected const"),
    }
}

#[test]
fn literal_large_hex() {
    let m = parse("const X: u32 = 0x15228c00").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(0x15228c00)),
        _ => panic!("expected const"),
    }
}

#[test]
fn literal_binary() {
    let m = parse("const X: u8 = 0b11001100").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(0b11001100)),
        _ => panic!("expected const"),
    }
}

#[test]
fn literal_underscore_separator() {
    let m = parse("const X: u32 = 1_000_000").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(1_000_000)),
        _ => panic!("expected const"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. State Machine Parsing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_sm_multiple_events() {
    let src = r#"
        state machine S {
            state A
            state B [terminal]
            initial A
            transition A -> B {
                on event1
                on event2(x: u8)
            }
        }
    "#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            assert_eq!(sm.transitions[0].events.len(), 2);
            assert_eq!(sm.transitions[0].events[1].params.len(), 1);
        }
        _ => panic!("expected state machine"),
    }
}

#[test]
fn parse_sm_delegate() {
    let src = r#"
        state machine S {
            state A { child: u8 }
            initial A
            transition A -> A {
                on event(id: u8, ev: u8)
                delegate src.child <- ev
            }
        }
    "#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            let d = sm.transitions[0].delegate.as_ref().unwrap();
            assert_eq!(d.event_name, "ev");
        }
        _ => panic!("expected state machine"),
    }
}

#[test]
fn parse_sm_state_with_defaults() {
    let src = r#"
        state machine S {
            state A { x: u8 = 0, y: u16 = 100 }
            state B [terminal]
            initial A
            transition A -> B { on done }
        }
    "#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            assert_eq!(sm.states[0].fields.len(), 2);
            assert_eq!(
                sm.states[0].fields[0].default_value,
                Some(AstLiteralValue::Int(0))
            );
            assert_eq!(
                sm.states[0].fields[1].default_value,
                Some(AstLiteralValue::Int(100))
            );
        }
        _ => panic!("expected state machine"),
    }
}

#[test]
fn parse_sm_wildcard_src() {
    let src = r#"
        state machine S {
            state A
            state B [terminal]
            initial A
            transition * -> B { on error }
        }
    "#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            assert_eq!(sm.transitions[0].src_state, "*");
        }
        _ => panic!("expected state machine"),
    }
}

#[test]
fn parse_sm_guard_expr() {
    let src = r#"
        state machine S {
            state A { count: u8 = 0 }
            state B [terminal]
            initial A
            transition A -> A {
                on tick
                guard src.count < 10
                action { dst.count = src.count + 1; }
            }
            transition A -> B { on done }
        }
    "#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            assert!(sm.transitions[0].guard.is_some());
            assert_eq!(sm.transitions[0].actions.len(), 1);
        }
        _ => panic!("expected state machine"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Complex Type Expression Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_nested_optional_in_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0x01 => A {
                flags: u8,
                extra: if flags & 0x01 { u32 },
                data: bytes[remaining],
            },
        }
    "#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => {
            assert_eq!(f.branches[0].fields.len(), 3);
        }
        _ => panic!("expected frame"),
    }
}

#[test]
fn parse_array_fill_within() {
    let src = "packet P { entries: [u8; fill] within length }";
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                if let AstTypeExpr::Array {
                    within_expr, count, ..
                } = &f.type_expr
                {
                    assert!(matches!(count, AstArrayCount::Fill));
                    assert!(within_expr.is_some());
                } else {
                    panic!("expected array type");
                }
            } else {
                panic!("expected field");
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_bytes_length_expr() {
    let src = "packet P { len: u16, data: bytes[length: len - 4] }";
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[1] {
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::Length,
                        ..
                    }
                ));
            } else {
                panic!("expected field");
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_match_type_in_struct() {
    let src = r#"
        type T = {
            tag: bits[2],
            value: match tag {
                0b00 => bits[6],
                0b01 => bits[14],
                0b10 => bits[30],
                0b11 => bits[62],
            },
        }
    "#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::Type(t) => {
            if let AstTypeDeclBody::Fields { fields } = &t.body {
                assert_eq!(fields.len(), 2);
                assert!(matches!(&fields[1].type_expr, AstTypeExpr::Match { .. }));
            } else {
                panic!("expected fields body");
            }
        }
        _ => panic!("expected type"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Multiple imports and complex module structures
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_multiple_imports() {
    let src = r#"
        module test
        import quic.varint.VarInt
        import net.tcp
        import ble.att.AttPdu
    "#;
    let m = parse(src).unwrap();
    assert_eq!(m.imports.len(), 3);
    assert_eq!(m.imports[0].name.as_deref(), Some("VarInt"));
    assert!(m.imports[1].name.is_none()); // whole module
    assert_eq!(m.imports[2].name.as_deref(), Some("AttPdu"));
}

#[test]
fn parse_export_packet() {
    let m = parse("export packet P { x: u8 }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => assert!(p.exported),
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_export_enum() {
    let m = parse("export enum E: u8 { A = 0 }").unwrap();
    match &m.items[0] {
        AstTopItem::Enum(e) => assert!(e.exported),
        _ => panic!("expected enum"),
    }
}

#[test]
fn parse_export_frame() {
    let src = "export frame F = match t: u8 { 0 => A {} }";
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => assert!(f.exported),
        _ => panic!("expected frame"),
    }
}

#[test]
fn parse_string_literal_const() {
    let m = parse(r#"const NAME: u8 = "hello""#).unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => {
            assert_eq!(c.value, AstLiteralValue::String("hello".to_string()));
        }
        _ => panic!("expected const"),
    }
}

#[test]
fn parse_named_annotation_arg() {
    let m = parse("@verify(bound = 4)\npacket P { x: u8 }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.annotations[0].name, "verify");
            match &p.annotations[0].args[0] {
                AstAnnotationArg::NamedValue { name, value } => {
                    assert_eq!(name, "bound");
                    assert_eq!(*value, AstLiteralValue::Int(4));
                }
                _ => panic!("expected named value"),
            }
        }
        _ => panic!("expected packet"),
    }
}

#[test]
fn parse_annotation_on_const() {
    let m = parse(r#"@doc("test") const X: u8 = 1"#).unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => {
            assert_eq!(c.annotations.len(), 1);
            assert_eq!(c.annotations[0].name, "doc");
        }
        _ => panic!("expected const"),
    }
}

#[test]
fn parse_annotation_on_enum() {
    let m = parse("@doc(\"test\")\nenum E: u8 { A = 0 }").unwrap();
    match &m.items[0] {
        AstTopItem::Enum(e) => {
            assert_eq!(e.annotations.len(), 1);
        }
        _ => panic!("expected enum"),
    }
}

#[test]
fn error_export_static_assert() {
    assert!(parse("export static_assert 1 == 1").is_err());
}
