use wirespec_sema::ComplianceProfile;
use wirespec_sema::analyze;
use wirespec_sema::types::Endianness;
use wirespec_syntax::parse;

#[test]
fn analyze_empty_module() {
    let ast = parse("module test").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.module_name, "test");
    assert_eq!(sem.module_endianness, Endianness::Big);
}

#[test]
fn analyze_const() {
    let ast = parse("const MAX: u8 = 20").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.consts.len(), 1);
    assert_eq!(sem.consts[0].name, "MAX");
    assert_eq!(sem.consts[0].const_id, "const:MAX");
}

#[test]
fn analyze_enum() {
    let ast = parse("enum E: u8 { A = 0, B = 1 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.enums.len(), 1);
    assert_eq!(sem.enums[0].name, "E");
    assert_eq!(sem.enums[0].members.len(), 2);
    assert!(!sem.enums[0].is_flags);
}

#[test]
fn analyze_flags() {
    let ast = parse("flags F: u8 { A = 0x01, B = 0x02 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.enums.len(), 1);
    assert!(sem.enums[0].is_flags);
}

#[test]
fn analyze_endianness_from_annotation() {
    let ast = parse("@endian little\nmodule test\npacket P { x: u16 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.module_endianness, Endianness::Little);
}

#[test]
fn analyze_simple_packet() {
    let ast = parse("packet Foo { x: u8, y: u16 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.packets.len(), 1);
    assert_eq!(sem.packets[0].name, "Foo");
    assert_eq!(sem.packets[0].fields.len(), 2);
    assert_eq!(sem.packets[0].fields[0].name, "x");
}

#[test]
fn analyze_packet_with_require() {
    let ast = parse("packet P { length: u16, require length >= 8 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.packets[0].requires.len(), 1);
    assert_eq!(sem.packets[0].items.len(), 2);
}

#[test]
fn analyze_packet_with_derived() {
    let ast = parse("packet P { flags: u8, let is_set: bool = (flags & 0x01) != 0 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.packets[0].derived.len(), 1);
    assert_eq!(sem.packets[0].derived[0].name, "is_set");
}

#[test]
fn analyze_type_alias() {
    let ast = parse("type Handle = u16le\npacket P { h: Handle }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.packets[0].fields[0].name, "h");
}

#[test]
fn analyze_undefined_type_error() {
    let ast = parse("packet P { x: Unknown }").unwrap();
    let result = analyze(&ast, ComplianceProfile::default(), &Default::default());
    assert!(result.is_err());
}

#[test]
fn analyze_forward_reference_error() {
    let ast = parse("packet P { data: bytes[length: length], length: u16 }").unwrap();
    let result = analyze(&ast, ComplianceProfile::default(), &Default::default());
    assert!(result.is_err());
}

#[test]
fn analyze_static_assert() {
    let ast = parse("const MAX: u8 = 20\nstatic_assert MAX <= 255").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.static_asserts.len(), 1);
}

#[test]
fn analyze_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x01 => B { x: u8 },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.frames.len(), 1);
    assert_eq!(sem.frames[0].variants.len(), 3);
    assert_eq!(sem.frames[0].tag_name, "tag");
}

#[test]
fn analyze_capsule() {
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
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.capsules.len(), 1);
    assert_eq!(sem.capsules[0].header_fields.len(), 2);
    assert_eq!(sem.capsules[0].variants.len(), 2);
}

#[test]
fn analyze_varint_prefix_match() {
    let src = r#"
        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6],
                0b01 => bits[14],
                0b10 => bits[30],
                0b11 => bits[62],
            },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.varints.len(), 1);
    assert_eq!(sem.varints[0].name, "VarInt");
}

#[test]
fn analyze_continuation_varint() {
    let src = r#"
        type MqttLen = varint {
            continuation_bit: msb,
            value_bits: 7,
            max_bytes: 4,
            byte_order: little,
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.varints.len(), 1);
    assert_eq!(sem.varints[0].name, "MqttLen");
}

#[test]
fn analyze_state_machine_basic() {
    let src = r#"
        state machine S {
            state Init { count: u8 = 0 }
            state Done [terminal]
            initial Init
            transition Init -> Done { on finish }
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.state_machines.len(), 1);
    let sm = &sem.state_machines[0];
    assert_eq!(sm.name, "S");
    assert_eq!(sm.states.len(), 2);
    assert_eq!(sm.transitions.len(), 1);
    assert!(sm.states[1].is_terminal);
}

#[test]
fn analyze_sm_wildcard_transition() {
    let src = r#"
        state machine S {
            state A
            state B [terminal]
            initial A
            transition A -> B { on close }
            transition * -> B { on error }
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let sm = &sem.state_machines[0];
    assert!(sm.transitions.len() >= 2);
}

#[test]
fn analyze_sm_with_guard_and_action() {
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
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let sm = &sem.state_machines[0];
    let t = &sm.transitions[0];
    assert!(t.guard.is_some());
    assert_eq!(t.actions.len(), 1);
}

#[test]
fn analyze_sm_undefined_state_error() {
    let src = r#"
        state machine S {
            state A
            initial A
            transition A -> Nonexistent { on go }
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(&ast, ComplianceProfile::default(), &Default::default());
    assert!(result.is_err());
}

#[test]
fn analyze_packet_bytes_remaining() {
    let ast = parse("packet P { data: bytes[remaining] }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.packets[0].fields.len(), 1);
}

#[test]
fn analyze_packet_bytes_fixed() {
    let ast = parse("packet P { mac: bytes[6] }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.packets[0].fields.len(), 1);
}

#[test]
fn analyze_packet_with_bits() {
    let ast = parse("packet P { a: bits[4], b: bits[4], c: u16 }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.packets[0].fields.len(), 3);
}

#[test]
fn analyze_packet_with_optional() {
    let ast = parse("packet P { flags: u8, extra: if flags & 0x01 { u16 } }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.packets[0].fields.len(), 2);
    // The second field should have Conditional presence
    assert!(matches!(
        sem.packets[0].fields[1].presence,
        wirespec_sema::types::FieldPresence::Conditional { .. }
    ));
}

#[test]
fn analyze_packet_with_array() {
    let ast = parse("packet P { count: u16, items: [u8; count] }").unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(sem.packets[0].fields.len(), 2);
}
