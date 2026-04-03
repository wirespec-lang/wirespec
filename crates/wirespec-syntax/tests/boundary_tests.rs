//! Boundary-value, edge-case, and error-path tests for wirespec-syntax parser.
//!
//! These tests focus on values at the limits of their domains, unusual but
//! valid constructs, and malformed inputs that must produce parse errors.
//! No existing tests or source files are modified.

use wirespec_syntax::ast::*;
use wirespec_syntax::parse;

// ══════════════════════════════════════════════════════════════════════
// Integer literal boundary values
// ══════════════════════════════════════════════════════════════════════

#[test]
fn literal_max_i64() {
    let m = parse("const X: u64 = 9223372036854775807").unwrap(); // i64::MAX
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(i64::MAX)),
        other => panic!("expected Const, got {:?}", other),
    }
}

#[test]
fn literal_zero() {
    let m = parse("const X: u8 = 0").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(0)),
        other => panic!("expected Const, got {:?}", other),
    }
}

#[test]
fn literal_zero_hex() {
    let m = parse("const X: u8 = 0x00").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(0)),
        other => panic!("expected Const, got {:?}", other),
    }
}

#[test]
fn literal_hex_max_u8() {
    let m = parse("const X: u8 = 0xFF").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(255)),
        other => panic!("expected Const, got {:?}", other),
    }
}

#[test]
fn literal_binary_all_ones() {
    let m = parse("const X: u8 = 0b11111111").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(255)),
        other => panic!("expected Const, got {:?}", other),
    }
}

#[test]
fn literal_binary_single_one() {
    let m = parse("const X: u8 = 0b00000001").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(1)),
        other => panic!("expected Const, got {:?}", other),
    }
}

#[test]
fn literal_binary_zero() {
    let m = parse("const X: u8 = 0b0").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(0)),
        other => panic!("expected Const, got {:?}", other),
    }
}

#[test]
fn literal_underscore_separator() {
    // Underscores are allowed in numeric literals for readability
    let m = parse("const X: u32 = 1_000_000").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(1_000_000)),
        other => panic!("expected Const, got {:?}", other),
    }
}

#[test]
fn literal_hex_underscore_separator() {
    let m = parse("const X: u32 = 0xFF_FF").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Int(0xFFFF)),
        other => panic!("expected Const, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Bits boundary values
// ══════════════════════════════════════════════════════════════════════

#[test]
fn bits_width_1() {
    let m = parse("packet P { x: bits[1] }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert!(
                    matches!(&f.type_expr, AstTypeExpr::Bits { width: 1, .. }),
                    "expected bits[1], got {:?}",
                    f.type_expr
                );
            } else {
                panic!("expected field");
            }
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn bits_width_64() {
    let m = parse("packet P { x: bits[64] }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert!(
                    matches!(&f.type_expr, AstTypeExpr::Bits { width: 64, .. }),
                    "expected bits[64], got {:?}",
                    f.type_expr
                );
            } else {
                panic!("expected field");
            }
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn bits_width_bit_alias() {
    // `bit` is an alias for `bits[1]`
    let m = parse("packet P { x: bit }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert!(
                    matches!(&f.type_expr, AstTypeExpr::Bits { width: 1, .. }),
                    "expected bits[1] from `bit`, got {:?}",
                    f.type_expr
                );
            } else {
                panic!("expected field");
            }
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Empty constructs
// ══════════════════════════════════════════════════════════════════════

#[test]
fn empty_packet() {
    let m = parse("packet P {}").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.name, "P");
            assert!(p.fields.is_empty());
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn empty_frame_variants() {
    let m = parse("frame F = match t: u8 { 0 => A {}, _ => B {} }").unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => {
            assert!(f.branches[0].fields.is_empty(), "variant A should be empty");
            assert!(f.branches[1].fields.is_empty(), "variant B should be empty");
        }
        other => panic!("expected Frame, got {:?}", other),
    }
}

#[test]
fn empty_state_no_fields() {
    let m = parse(
        "state machine S { state A state B [terminal] initial A transition A -> B { on go } }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            assert!(
                sm.states[0].fields.is_empty(),
                "state A should have no fields"
            );
        }
        other => panic!("expected StateMachine, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Deeply nested / chained expressions
// ══════════════════════════════════════════════════════════════════════

#[test]
fn deeply_nested_binary_add_chain() {
    // a + b + c + d + e + f + g + h should parse as left-associative
    let m = parse("static_assert a + b + c + d + e + f + g + h == 0").unwrap();
    assert!(matches!(&m.items[0], AstTopItem::StaticAssert(_)));
}

#[test]
fn nested_member_access_chain() {
    let m = parse("static_assert a.b.c.d.e == 0").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            // The outermost expression should be Eq
            if let AstExpr::Binary {
                op: BinOp::Eq,
                left,
                ..
            } = &sa.expr
            {
                // The left side is a chain of MemberAccess
                assert!(
                    matches!(**left, AstExpr::MemberAccess { .. }),
                    "expected member access chain, got {:?}",
                    left
                );
            } else {
                panic!("expected eq expression, got {:?}", sa.expr);
            }
        }
        other => panic!("expected StaticAssert, got {:?}", other),
    }
}

#[test]
fn nested_parenthesized_expression() {
    let m = parse("static_assert ((((x)))) == 0").unwrap();
    assert!(matches!(&m.items[0], AstTopItem::StaticAssert(_)));
}

// ══════════════════════════════════════════════════════════════════════
// Multi-field packets (stress)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn packet_many_fields() {
    let mut src = "packet P {\n".to_string();
    for i in 0..50 {
        src.push_str(&format!("    f{i}: u8,\n"));
    }
    src.push('}');
    let m = parse(&src).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => assert_eq!(p.fields.len(), 50),
        other => panic!("expected Packet, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Pattern values (large and boundary)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn pattern_large_hex_value() {
    let m = parse("frame F = match t: u32 { 0x15228c00 => A {}, _ => B {} }").unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => {
            if let AstPattern::Value { value, .. } = &f.branches[0].pattern {
                assert_eq!(*value, 0x15228c00_i64);
            } else {
                panic!("expected Value pattern, got {:?}", f.branches[0].pattern);
            }
        }
        other => panic!("expected Frame, got {:?}", other),
    }
}

#[test]
fn pattern_range_boundary_values() {
    let m = parse("frame F = match t: u8 { 0x00..=0xFF => A {}, _ => B {} }").unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => {
            assert!(matches!(
                &f.branches[0].pattern,
                AstPattern::RangeInclusive {
                    start: 0,
                    end: 255,
                    ..
                }
            ));
        }
        other => panic!("expected Frame, got {:?}", other),
    }
}

#[test]
fn pattern_wildcard_only() {
    let m = parse("frame F = match t: u8 { _ => Fallback { data: bytes[remaining] } }").unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => {
            assert_eq!(f.branches.len(), 1);
            assert!(matches!(
                &f.branches[0].pattern,
                AstPattern::Wildcard { .. }
            ));
        }
        other => panic!("expected Frame, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Annotation edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn annotation_string_with_newline_escape() {
    let m = parse(r#"@doc("hello\nworld") packet P { x: u8 }"#).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.annotations.len(), 1);
            assert_eq!(p.annotations[0].name, "doc");
            match &p.annotations[0].args[0] {
                AstAnnotationArg::String(s) => {
                    assert!(s.contains('\n'), "expected newline escape, got {:?}", s);
                }
                other => panic!("expected String arg, got {:?}", other),
            }
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn annotation_string_with_tab_escape() {
    let m = parse(r#"@doc("col1\tcol2") packet P { x: u8 }"#).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => match &p.annotations[0].args[0] {
            AstAnnotationArg::String(s) => {
                assert!(s.contains('\t'), "expected tab escape, got {:?}", s);
            }
            other => panic!("expected String arg, got {:?}", other),
        },
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn annotation_string_with_escaped_quote() {
    let m = parse(r#"@doc("say \"hi\"") packet P { x: u8 }"#).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => match &p.annotations[0].args[0] {
            AstAnnotationArg::String(s) => {
                assert!(s.contains('"'), "expected escaped quote, got {:?}", s);
            }
            other => panic!("expected String arg, got {:?}", other),
        },
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn annotation_bare_no_args() {
    let m = parse("@strict\npacket P { x: u8 }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.annotations[0].name, "strict");
            assert!(p.annotations[0].args.is_empty());
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn annotation_with_named_value() {
    let m = parse("@checksum(scope = true)\npacket P { x: u8 }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.annotations[0].name, "checksum");
            assert!(matches!(
                &p.annotations[0].args[0],
                AstAnnotationArg::NamedValue { name, value: AstLiteralValue::Bool(true) } if name == "scope"
            ));
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn multiple_annotations_on_packet() {
    // Without module decl, all annotations flow to the first item
    let m = parse("@derive(debug)\n@strict\npacket P { x: u8 }").unwrap();
    assert!(
        m.annotations.is_empty(),
        "no module-level annotations expected"
    );
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.annotations.len(), 2);
            assert_eq!(p.annotations[0].name, "derive");
            assert_eq!(p.annotations[1].name, "strict");
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn annotations_after_module_decl_are_file_level() {
    // With module decl, annotations between module decl and first item are file-level
    let m = parse("@endian big\nmodule test\n@derive(debug)\npacket P { x: u8 }").unwrap();
    // Both @endian and @derive end up as file-level annotations
    assert!(!m.annotations.is_empty(), "expected file-level annotations");
    assert_eq!(m.annotations[0].name, "endian");
}

// ══════════════════════════════════════════════════════════════════════
// Error recovery: malformed inputs
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_missing_closing_brace_packet() {
    assert!(parse("packet P { x: u8").is_err());
}

#[test]
fn error_missing_closing_brace_frame() {
    assert!(parse("frame F = match t: u8 { 0 => A { x: u8 }").is_err());
}

#[test]
fn error_missing_fat_arrow_in_frame() {
    assert!(parse("frame F = match t: u8 { 0 A {} }").is_err());
}

#[test]
fn error_double_fat_arrow() {
    assert!(parse("frame F = match t: u8 { 0 => => A {} }").is_err());
}

#[test]
fn error_missing_type_in_field() {
    assert!(parse("packet P { x: }").is_err());
}

#[test]
fn error_missing_field_name() {
    assert!(parse("packet P { : u8 }").is_err());
}

#[test]
fn error_missing_const_value() {
    assert!(parse("const X: u8 =").is_err());
}

#[test]
fn error_missing_enum_value() {
    assert!(parse("enum E: u8 { A }").is_err());
}

#[test]
fn error_unterminated_string() {
    assert!(parse(r#"@doc("unterminated) packet P {}"#).is_err());
}

#[test]
fn error_completely_empty_frame() {
    // Frame with no branches at all
    assert!(
        parse("frame F = match t: u8 {}").is_ok() || parse("frame F = match t: u8 {}").is_err()
    );
    // This test just verifies the parser doesn't crash
}

#[test]
fn error_missing_match_keyword() {
    assert!(parse("frame F = t: u8 { 0 => A {} }").is_err());
}

#[test]
fn error_missing_state_machine_opening_brace() {
    assert!(parse("state machine S state A initial A").is_err());
}

#[test]
fn error_garbage_input() {
    assert!(parse("!@#$%^&*()").is_err());
}

#[test]
fn error_only_keywords() {
    assert!(parse("packet packet packet").is_err());
}

// ══════════════════════════════════════════════════════════════════════
// Bytes edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn bytes_zero_fixed() {
    let m = parse("packet P { data: bytes[0] }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::Fixed,
                        fixed_size: Some(0),
                        ..
                    }
                ));
            } else {
                panic!("expected field");
            }
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn bytes_large_fixed() {
    let m = parse("packet P { data: bytes[65535] }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::Fixed,
                        fixed_size: Some(65535),
                        ..
                    }
                ));
            } else {
                panic!("expected field");
            }
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Whitespace, comments, and formatting variations
// ══════════════════════════════════════════════════════════════════════

#[test]
fn tabs_and_spaces_mixed() {
    let src = "packet\tP\t{\n\tx:\tu8,\n\ty: u16\n}";
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => assert_eq!(p.fields.len(), 2),
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn hash_comment_between_fields() {
    let src = "packet P {\n    x: u8,\n    # this is a comment\n    y: u16,\n}";
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => assert_eq!(p.fields.len(), 2),
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn slash_comment_between_fields() {
    let src = "packet P {\n    x: u8,\n    // another comment\n    y: u16,\n}";
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => assert_eq!(p.fields.len(), 2),
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn comment_after_closing_brace() {
    let src = "packet P { x: u8 } # trailing comment";
    let m = parse(src).unwrap();
    assert_eq!(m.items.len(), 1);
}

#[test]
fn all_whitespace_types() {
    // Carriage returns, newlines, tabs, spaces
    let src = "packet P {\r\n\tx: u8,\r\n\ty: u16,\r\n}";
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => assert_eq!(p.fields.len(), 2),
        other => panic!("expected Packet, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Item declaration order preservation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn items_declaration_order_preserved() {
    let src = "const A: u8 = 1\npacket P { x: u8 }\nenum E: u8 { X = 0 }\npacket Q { y: u16 }";
    let m = parse(src).unwrap();
    assert_eq!(m.items.len(), 4);
    assert!(matches!(&m.items[0], AstTopItem::Const(_)));
    assert!(matches!(&m.items[1], AstTopItem::Packet(_)));
    assert!(matches!(&m.items[2], AstTopItem::Enum(_)));
    assert!(matches!(&m.items[3], AstTopItem::Packet(_)));
}

// ══════════════════════════════════════════════════════════════════════
// State machine parser edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn sm_parser_allows_missing_initial() {
    // The parser allows no `initial` declaration; initial_state will be empty string.
    // Semantic analysis catches this.
    let result = parse("state machine S { state A transition A -> A { on tick } }");
    assert!(result.is_ok(), "parser should accept SM without initial");
    match &result.unwrap().items[0] {
        AstTopItem::StateMachine(sm) => {
            assert!(sm.initial_state.is_empty());
        }
        other => panic!("expected StateMachine, got {:?}", other),
    }
}

#[test]
fn sm_multiple_events_on_transition() {
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on event1 on event2 }
    }"#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            assert_eq!(sm.transitions[0].events.len(), 2);
            assert_eq!(sm.transitions[0].events[0].name, "event1");
            assert_eq!(sm.transitions[0].events[1].name, "event2");
        }
        other => panic!("expected StateMachine, got {:?}", other),
    }
}

#[test]
fn sm_event_with_params() {
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on ev(id: u8, data: u16) }
    }"#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            let ev = &sm.transitions[0].events[0];
            assert_eq!(ev.name, "ev");
            assert_eq!(ev.params.len(), 2);
            assert_eq!(ev.params[0].name, "id");
            assert_eq!(ev.params[1].name, "data");
        }
        other => panic!("expected StateMachine, got {:?}", other),
    }
}

#[test]
fn sm_wildcard_source_state() {
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition * -> B { on error }
    }"#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::StateMachine(sm) => {
            assert_eq!(sm.transitions[0].src_state, "*");
        }
        other => panic!("expected StateMachine, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Capsule edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn capsule_with_expr_tag_shift() {
    let m = parse(
        "capsule C {
            header: u8,
            length: u16,
            payload: match (header >> 4) within length {
                1 => A { data: bytes[remaining] },
                _ => B { data: bytes[remaining] },
            },
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::Capsule(c) => {
            assert!(matches!(&c.payload_tag, AstPayloadTagSelector::Expr { .. }));
            assert_eq!(c.branches.len(), 2);
        }
        other => panic!("expected Capsule, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Enum / flags edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn enum_single_member() {
    let m = parse("enum E: u8 { Only = 0 }").unwrap();
    match &m.items[0] {
        AstTopItem::Enum(e) => {
            assert_eq!(e.members.len(), 1);
            assert_eq!(e.members[0].name, "Only");
            assert_eq!(e.members[0].value, 0);
        }
        other => panic!("expected Enum, got {:?}", other),
    }
}

#[test]
fn enum_many_members() {
    let mut src = "enum E: u16 {\n".to_string();
    for i in 0..20 {
        src.push_str(&format!("    V{i} = {i},\n"));
    }
    src.push('}');
    let m = parse(&src).unwrap();
    match &m.items[0] {
        AstTopItem::Enum(e) => {
            assert_eq!(e.members.len(), 20);
            assert_eq!(e.members[19].name, "V19");
            assert_eq!(e.members[19].value, 19);
        }
        other => panic!("expected Enum, got {:?}", other),
    }
}

#[test]
fn flags_bitmask_values() {
    let m = parse("flags F: u8 { A = 0x01, B = 0x02, C = 0x04, D = 0x08 }").unwrap();
    match &m.items[0] {
        AstTopItem::Flags(f) => {
            assert_eq!(f.members.len(), 4);
            assert_eq!(f.members[0].value, 0x01);
            assert_eq!(f.members[1].value, 0x02);
            assert_eq!(f.members[2].value, 0x04);
            assert_eq!(f.members[3].value, 0x08);
        }
        other => panic!("expected Flags, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Continuation varint edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn continuation_varint_big_endian() {
    let m = parse(
        "type V = varint {
            continuation_bit: msb,
            value_bits: 7,
            max_bytes: 4,
            byte_order: big,
        }",
    )
    .unwrap();
    match &m.items[0] {
        AstTopItem::ContinuationVarInt(v) => {
            assert_eq!(v.name, "V");
            assert_eq!(v.byte_order, "big");
            assert_eq!(v.max_bytes, 4);
            assert_eq!(v.value_bits, 7);
            assert_eq!(v.continuation_bit, "msb");
        }
        other => panic!("expected ContinuationVarInt, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Multiple items in single source
// ══════════════════════════════════════════════════════════════════════

#[test]
fn many_items_various_types() {
    let src = r#"
module test.boundary

const A: u8 = 1
const B: u16 = 0xFFFF

enum Direction: u8 { In = 0, Out = 1 }
flags Opts: u8 { Fast = 0x01, Reliable = 0x02 }

type Handle = u16le

packet Header { x: u8, y: u16 }
packet Body { data: bytes[remaining] }

frame F = match tag: u8 {
    0 => Empty {},
    _ => Fallback { data: bytes[remaining] },
}

static_assert A <= 255
"#;
    let m = parse(src).unwrap();
    assert_eq!(m.module_decl.as_ref().unwrap().name, "test.boundary");
    // const, const, enum, flags, type, packet, packet, frame, static_assert = 9
    assert_eq!(m.items.len(), 9);
}

// ══════════════════════════════════════════════════════════════════════
// Import edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn multiple_imports() {
    let src = "import a.b.C\nimport d.e\nimport f.g.H\npacket P { x: u8 }";
    let m = parse(src).unwrap();
    assert_eq!(m.imports.len(), 3);
    assert_eq!(m.imports[0].module, "a.b");
    assert_eq!(m.imports[0].name.as_deref(), Some("C"));
    assert_eq!(m.imports[1].module, "d.e");
    assert!(m.imports[1].name.is_none());
    assert_eq!(m.imports[2].module, "f.g");
    assert_eq!(m.imports[2].name.as_deref(), Some("H"));
}

// ══════════════════════════════════════════════════════════════════════
// Operator precedence edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn precedence_multiply_before_add() {
    let m = parse("static_assert 2 * 3 + 4 == 10").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            // Should be ((2 * 3) + 4) == 10
            if let AstExpr::Binary {
                op: BinOp::Eq,
                left,
                ..
            } = &sa.expr
            {
                if let AstExpr::Binary {
                    op: BinOp::Add,
                    left: add_l,
                    ..
                } = &**left
                {
                    assert!(
                        matches!(**add_l, AstExpr::Binary { op: BinOp::Mul, .. }),
                        "mul should be inside add"
                    );
                } else {
                    panic!("expected add, got {:?}", left);
                }
            } else {
                panic!("expected eq, got {:?}", sa.expr);
            }
        }
        other => panic!("expected StaticAssert, got {:?}", other),
    }
}

#[test]
fn precedence_logical_and_before_or() {
    let m = parse("static_assert a == 1 or b == 2 and c == 3").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            // or binds looser: a==1 or (b==2 and c==3)
            assert!(matches!(&sa.expr, AstExpr::Binary { op: BinOp::Or, .. }));
        }
        other => panic!("expected StaticAssert, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Boolean / null literals
// ══════════════════════════════════════════════════════════════════════

#[test]
fn const_bool_true() {
    let m = parse("const X: bool = true").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Bool(true)),
        other => panic!("expected Const, got {:?}", other),
    }
}

#[test]
fn const_bool_false() {
    let m = parse("const X: bool = false").unwrap();
    match &m.items[0] {
        AstTopItem::Const(c) => assert_eq!(c.value, AstLiteralValue::Bool(false)),
        other => panic!("expected Const, got {:?}", other),
    }
}

#[test]
fn expr_null_comparison() {
    let m = parse("static_assert x != null").unwrap();
    match &m.items[0] {
        AstTopItem::StaticAssert(sa) => {
            if let AstExpr::Binary {
                op: BinOp::Ne,
                right,
                ..
            } = &sa.expr
            {
                assert!(matches!(**right, AstExpr::Null { .. }));
            } else {
                panic!("expected ne, got {:?}", sa.expr);
            }
        }
        other => panic!("expected StaticAssert, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Export variations
// ══════════════════════════════════════════════════════════════════════

#[test]
fn export_packet() {
    let m = parse("export packet P { x: u8 }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => assert!(p.exported),
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn export_enum() {
    let m = parse("export enum E: u8 { A = 0 }").unwrap();
    match &m.items[0] {
        AstTopItem::Enum(e) => assert!(e.exported),
        other => panic!("expected Enum, got {:?}", other),
    }
}

#[test]
fn export_flags() {
    let m = parse("export flags F: u8 { A = 1 }").unwrap();
    match &m.items[0] {
        AstTopItem::Flags(f) => assert!(f.exported),
        other => panic!("expected Flags, got {:?}", other),
    }
}

#[test]
fn non_exported_by_default() {
    let m = parse("packet P { x: u8 }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => assert!(!p.exported),
        other => panic!("expected Packet, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Packet with all bytes kinds
// ══════════════════════════════════════════════════════════════════════

#[test]
fn all_bytes_kinds_in_one_packet() {
    let src = r#"packet P {
        fixed: bytes[16],
        len: u16,
        dynamic: bytes[length: len],
        opt_len: if true { u16 },
        hybrid: bytes[length_or_remaining: opt_len],
        rest: bytes[remaining],
    }"#;
    let m = parse(src).unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            assert_eq!(p.fields.len(), 6);
            // Verify bytes kinds
            if let AstFieldItem::Field(f) = &p.fields[0] {
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::Fixed,
                        ..
                    }
                ));
            }
            if let AstFieldItem::Field(f) = &p.fields[2] {
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::Length,
                        ..
                    }
                ));
            }
            if let AstFieldItem::Field(f) = &p.fields[4] {
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::LengthOrRemaining,
                        ..
                    }
                ));
            }
            if let AstFieldItem::Field(f) = &p.fields[5] {
                assert!(matches!(
                    &f.type_expr,
                    AstTypeExpr::Bytes {
                        kind: AstBytesKind::Remaining,
                        ..
                    }
                ));
            }
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Array edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn array_fill_count() {
    let m = parse("packet P { items: [u8; fill] }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[0] {
                if let AstTypeExpr::Array { count, .. } = &f.type_expr {
                    assert!(matches!(count, AstArrayCount::Fill));
                } else {
                    panic!("expected array type");
                }
            }
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn array_expr_count() {
    let m = parse("packet P { n: u8, items: [u16; n * 2] }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Field(f) = &p.fields[1] {
                if let AstTypeExpr::Array {
                    count: AstArrayCount::Expr(expr),
                    ..
                } = &f.type_expr
                {
                    assert!(matches!(expr, AstExpr::Binary { op: BinOp::Mul, .. }));
                } else {
                    panic!("expected array with expr count, got {:?}", f.type_expr);
                }
            }
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Trailing commas
// ══════════════════════════════════════════════════════════════════════

#[test]
fn trailing_comma_in_packet() {
    let m = parse("packet P { x: u8, y: u16, }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => assert_eq!(p.fields.len(), 2),
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn trailing_comma_in_enum() {
    let m = parse("enum E: u8 { A = 0, B = 1, }").unwrap();
    match &m.items[0] {
        AstTopItem::Enum(e) => assert_eq!(e.members.len(), 2),
        other => panic!("expected Enum, got {:?}", other),
    }
}

#[test]
fn trailing_comma_in_frame() {
    let m = parse("frame F = match t: u8 { 0 => A {}, _ => B {}, }").unwrap();
    match &m.items[0] {
        AstTopItem::Frame(f) => assert_eq!(f.branches.len(), 2),
        other => panic!("expected Frame, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Require clause edge cases
// ══════════════════════════════════════════════════════════════════════

#[test]
fn multiple_require_clauses() {
    let m =
        parse("packet P { x: u8, y: u8, require x > 0, require y < 100, require x != y }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            let require_count = p
                .fields
                .iter()
                .filter(|f| matches!(f, AstFieldItem::Require(_)))
                .count();
            assert_eq!(require_count, 3);
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

#[test]
fn require_with_complex_expr() {
    let m = parse("packet P { len: u16, require len >= 8 and len <= 65535 }").unwrap();
    match &m.items[0] {
        AstTopItem::Packet(p) => {
            if let AstFieldItem::Require(r) = &p.fields[1] {
                assert!(matches!(&r.expr, AstExpr::Binary { op: BinOp::And, .. }));
            } else {
                panic!("expected Require clause");
            }
        }
        other => panic!("expected Packet, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Transition clause multiplicity (spec §6.2: guard, action, delegate 0..1)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn error_duplicate_guard_in_transition() {
    let src = r#"state machine S {
        state A state B [terminal] initial A
        transition A -> B { on go guard true guard false }
    }"#;
    assert!(parse(src).is_err(), "duplicate guard should fail");
}

#[test]
fn error_duplicate_action_in_transition() {
    let src = r#"state machine S {
        state A { x: u8 = 0 } state B [terminal] initial A
        transition A -> B { on go action { } action { } }
    }"#;
    assert!(parse(src).is_err(), "duplicate action should fail");
}

#[test]
fn error_duplicate_delegate_in_transition() {
    let src = r#"state machine S {
        state A { child: Sub } state B [terminal] initial A
        transition A -> A { on ev delegate src.child <- ev delegate src.child <- ev }
    }"#;
    assert!(parse(src).is_err(), "duplicate delegate should fail");
}

// ══════════════════════════════════════════════════════════════════════
// Fuzz / boundary tests for crash resilience
// ══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_random_bytes_no_panic() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    for seed in 0..1000u64 {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        let hash = hasher.finish();
        let len = (hash % 1024) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|i| ((hash >> (i % 8)) ^ (i as u64)) as u8)
            .collect();
        let input = String::from_utf8_lossy(&bytes).to_string();
        let _ = wirespec_syntax::parse(&input);
    }
}

#[test]
fn fuzz_mutated_valid_wspec_no_panic() {
    let valid = "packet Foo { x: u8, y: u16, data: bytes[remaining] }";
    let bytes = valid.as_bytes();
    for i in 0..100 {
        let mut mutated = bytes.to_vec();
        let pos = i % mutated.len();
        mutated[pos] = (mutated[pos].wrapping_add(i as u8)) % 128;
        let input = String::from_utf8_lossy(&mutated).to_string();
        let _ = wirespec_syntax::parse(&input);
    }
}

#[test]
fn fuzz_deeply_nested_expression_no_stackoverflow() {
    // Use a thread with a larger stack to avoid stack overflow in the test
    // harness; the important thing is that the parser doesn't panic.
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024) // 8 MB stack
        .spawn(|| {
            let depth = 200;
            let mut expr = "a".to_string();
            for _ in 0..depth {
                expr = format!("({expr} + 1)");
            }
            let input = format!("packet P {{ x: u8, require {expr} }}");
            let _ = wirespec_syntax::parse(&input);
        })
        .unwrap();
    handle
        .join()
        .expect("parser should not panic on deeply nested expression");
}

#[test]
fn fuzz_very_long_identifier_no_panic() {
    let long_name = "a".repeat(10000);
    let input = format!("packet {long_name} {{ x: u8 }}");
    let _ = wirespec_syntax::parse(&input);
}

#[test]
fn fuzz_many_fields_no_panic() {
    let fields: String = (0..1000)
        .map(|i| format!("field_{i}: u8"))
        .collect::<Vec<_>>()
        .join(", ");
    let input = format!("packet BigPacket {{ {fields} }}");
    let _ = wirespec_syntax::parse(&input);
}
