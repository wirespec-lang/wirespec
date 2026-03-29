use wirespec_driver::pipeline::*;
use wirespec_sema::ComplianceProfile;

#[test]
fn pipeline_simple_packet() {
    let codec = compile_module(
        "packet P { x: u8, y: u16 }",
        ComplianceProfile::default(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(codec.packets.len(), 1);
    assert_eq!(codec.packets[0].fields.len(), 2);
}

#[test]
fn pipeline_with_varint_and_packet() {
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
    let codec = compile_module(src, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(codec.varints.len(), 1);
    assert_eq!(codec.packets.len(), 1);
}

#[test]
fn pipeline_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0 => A {},
            _ => B { data: bytes[remaining] },
        }
    "#;
    let codec = compile_module(src, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(codec.frames.len(), 1);
}

#[test]
fn pipeline_capsule() {
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
    let codec = compile_module(src, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(codec.capsules.len(), 1);
}

#[test]
fn pipeline_state_machine() {
    let src = r#"
        state machine S {
            state A
            state B [terminal]
            initial A
            transition A -> B { on done }
        }
    "#;
    let codec = compile_module(src, ComplianceProfile::default(), &Default::default()).unwrap();
    assert_eq!(codec.state_machines.len(), 1);
}

#[test]
fn pipeline_parse_error() {
    let result = compile_module(
        "packet { bad }",
        ComplianceProfile::default(),
        &Default::default(),
    );
    assert!(result.is_err());
}

#[test]
fn pipeline_sema_error() {
    let result = compile_module(
        "packet P { x: NonExistent }",
        ComplianceProfile::default(),
        &Default::default(),
    );
    assert!(result.is_err());
}

#[test]
fn pipeline_with_external_types() {
    // ExternalTypes are wired into sema — imported types resolve correctly.
    let mut ext = ExternalTypes::default();
    ext.register(
        "VarInt",
        ExternalType {
            module: "quic.varint".to_string(),
            name: "VarInt".to_string(),
            source_prefix: "quic_varint".to_string(),
            kind: ExternalTypeKind::VarInt,
        },
    );
    // VarInt is registered as an external type, so sema should resolve it.
    let codec = compile_module(
        "packet P { x: VarInt }",
        ComplianceProfile::default(),
        &ext,
    )
    .unwrap();
    assert_eq!(codec.packets.len(), 1);
}
