// crates/wirespec-backend-rust/tests/codegen_tests.rs
//
// Integration tests for the Rust backend.

use wirespec_backend_api::*;
use wirespec_backend_rust::RustBackend;
use wirespec_backend_rust::checksum_binding::RustChecksumBindings;

fn generate_rust(src: &str) -> String {
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem = wirespec_sema::analyze(
        &ast,
        wirespec_sema::ComplianceProfile::default(),
        &Default::default(),
    )
    .unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    let backend = RustBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(RustBackendOptions::default()),
        checksum_bindings: std::sync::Arc::new(RustChecksumBindings),
        is_entry_module: true,
    };
    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();
    lowered.source
}

#[test]
fn codegen_simple_packet() {
    let rs = generate_rust("packet P { x: u8, y: u16 }");
    assert!(rs.contains("pub struct"), "should contain pub struct");
    assert!(rs.contains("pub x: u8"), "should contain x: u8");
    assert!(rs.contains("pub y: u16"), "should contain y: u16");
    assert!(rs.contains("fn parse"), "should contain fn parse");
    assert!(rs.contains("fn serialize"), "should contain fn serialize");
    assert!(
        rs.contains("fn serialized_len"),
        "should contain fn serialized_len"
    );
}

#[test]
fn codegen_bytes_field() {
    let rs = generate_rust("packet P { data: bytes[remaining] }");
    assert!(rs.contains("&'a [u8]"), "should contain byte slice type");
    assert!(rs.contains("<'a>"), "should have lifetime annotation");
}

#[test]
fn codegen_optional_field() {
    let rs = generate_rust("packet P { flags: u8, extra: if flags & 1 { u16 } }");
    assert!(rs.contains("Option<u16>"), "should contain Option<u16>");
}

#[test]
fn codegen_frame() {
    let src = "frame F = match tag: u8 { 0 => A {}, 1 => B { x: u8 }, _ => C { data: bytes[remaining] } }";
    let rs = generate_rust(src);
    assert!(rs.contains("pub enum"), "should contain pub enum");
    assert!(rs.contains("match"), "should contain match dispatch");
}

#[test]
fn codegen_bitgroup() {
    let rs = generate_rust("packet P { a: bits[4], b: bits[4], c: u16 }");
    assert!(
        rs.contains(">>"),
        "should contain shift right for extraction"
    );
    assert!(rs.contains("& 0xf"), "should contain mask for 4-bit field");
}

#[test]
fn codegen_array() {
    let rs = generate_rust("packet P { count: u8, items: [u8; count] }");
    assert!(rs.contains("items_count"), "should contain items_count");
}

#[test]
fn codegen_enum_def() {
    let rs = generate_rust("enum E: u8 { A = 0, B = 1 }");
    assert!(
        rs.contains("pub const"),
        "should contain pub const for enum values"
    );
}

#[test]
fn codegen_derived_field() {
    let rs = generate_rust("packet P { flags: u8, let is_set: bool = (flags & 1) != 0 }");
    assert!(rs.contains("is_set"), "should contain derived field is_set");
}

#[test]
fn codegen_require() {
    let rs = generate_rust("packet P { length: u16, require length >= 8 }");
    assert!(
        rs.contains("Error::Constraint"),
        "should contain constraint check"
    );
}

#[test]
fn codegen_artifact_emission() {
    let ast = wirespec_syntax::parse("packet P { x: u8 }").unwrap();
    let sem = wirespec_sema::analyze(
        &ast,
        wirespec_sema::ComplianceProfile::default(),
        &Default::default(),
    )
    .unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    let backend = RustBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(RustBackendOptions::default()),
        checksum_bindings: std::sync::Arc::new(RustChecksumBindings),
        is_entry_module: true,
    };
    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();

    let mut sink = MemorySink::new();
    Backend::emit(&backend, &lowered, &mut sink).unwrap();
    assert_eq!(
        sink.artifacts.len(),
        1,
        "should produce a single .rs artifact"
    );
    assert!(
        sink.artifacts[0]
            .0
            .relative_path
            .to_str()
            .unwrap()
            .ends_with(".rs"),
        "artifact should be a .rs file"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// VarInt codegen
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_rust_varint_prefix_match() {
    let src = r#"type VarInt = {
        prefix: bits[2],
        value: match prefix {
            0b00 => bits[6],
            0b01 => bits[14],
            0b10 => bits[30],
            0b11 => bits[62],
        },
    }"#;
    let rs = generate_rust(src);
    assert!(
        rs.contains("fn var_int_parse"),
        "should contain parse function"
    );
    assert!(
        rs.contains("fn var_int_serialize"),
        "should contain serialize function"
    );
    assert!(
        rs.contains("fn var_int_wire_size"),
        "should contain wire_size function"
    );
    assert!(rs.contains("match prefix"), "should contain prefix match");
}

#[test]
fn codegen_rust_varint_continuation_bit() {
    let src = r#"type MqttLen = varint {
        continuation_bit: msb,
        value_bits: 7,
        max_bytes: 4,
        byte_order: little,
    }"#;
    let rs = generate_rust(src);
    assert!(
        rs.contains("fn mqtt_len_parse"),
        "should contain parse function"
    );
    assert!(
        rs.contains("fn mqtt_len_serialize"),
        "should contain serialize function"
    );
    assert!(
        rs.contains("fn mqtt_len_wire_size"),
        "should contain wire_size function"
    );
    assert!(rs.contains("0x7f"), "should contain value mask");
    assert!(rs.contains("0x80"), "should contain continuation mask");
}

// ═══════════════════════════════════════════════════════════════════════════
// Const codegen
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_rust_const() {
    let rs = generate_rust("const MAX: u8 = 20\npacket P { x: u8 }");
    assert!(rs.contains("pub const"), "should contain pub const");
    assert!(rs.contains("MAX"), "should contain constant name");
}

// ═══════════════════════════════════════════════════════════════════════════
// Enum codegen (type alias + const pattern)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_rust_enum_type_alias() {
    let rs = generate_rust("enum E: u8 { A = 0, B = 1 }");
    assert!(rs.contains("pub type"), "should contain type alias");
    assert!(rs.contains("pub const"), "should contain pub const");
}

// ═══════════════════════════════════════════════════════════════════════════
// State Machine codegen
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_rust_state_machine() {
    let src = r#"
        state machine S {
            state Init { count: u8 = 0 }
            state Done [terminal]
            initial Init
            transition Init -> Done { on finish }
        }
    "#;
    let rs = generate_rust(src);
    assert!(rs.contains("pub enum"), "should contain state enum");
    assert!(rs.contains("dispatch"), "should contain dispatch method");
    assert!(rs.contains("fn new"), "should contain new constructor");
}

#[test]
fn codegen_rust_state_machine_with_guard_and_action() {
    let src = r#"
        state machine Counter {
            state Counting { count: u8 = 0 }
            state Done [terminal]
            initial Counting
            transition Counting -> Counting {
                on tick
                guard src.count < 10
                action { dst.count = src.count + 1; }
            }
            transition Counting -> Done { on finish }
        }
    "#;
    let rs = generate_rust(src);
    assert!(rs.contains("pub enum Counter"), "should contain state enum");
    assert!(
        rs.contains("pub enum CounterEvent"),
        "should contain event enum"
    );
    assert!(rs.contains("dispatch"), "should contain dispatch method");
    assert!(rs.contains("InvalidState"), "should contain guard error");
}

#[test]
fn codegen_rust_state_machine_event_params() {
    let src = r#"
        state machine S {
            state Init { val: u8 = 0 }
            state Active { val: u8 }
            state Done [terminal]
            initial Init
            transition Init -> Active {
                on start(x: u8)
                action { dst.val = x; }
            }
            transition Active -> Done { on finish }
        }
    "#;
    let rs = generate_rust(src);
    assert!(rs.contains("pub enum SEvent"), "should contain event enum");
    assert!(rs.contains("x: u8"), "should contain event parameter");
}

// ═══════════════════════════════════════════════════════════════════════════
// Checksum codegen
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_rust_checksum_internet() {
    let src = "packet P { data: u32, @checksum(internet) checksum: u16 }";
    let rs = generate_rust(src);
    assert!(
        rs.contains("internet_checksum"),
        "should contain internet_checksum call for verify"
    );
    assert!(
        rs.contains("internet_checksum_compute"),
        "should contain internet_checksum_compute call for serialize"
    );
    assert!(
        rs.contains("Error::Checksum"),
        "should contain Checksum error"
    );
    assert!(rs.contains("_start"), "should track start position");
    assert!(
        rs.contains("_cksum_offset"),
        "should track checksum field offset for serialize"
    );
}

#[test]
fn codegen_rust_checksum_crc32() {
    let src = "packet P { data: u32, @checksum(crc32) checksum: u32 }";
    let rs = generate_rust(src);
    assert!(
        rs.contains("crc32_verify"),
        "should contain crc32_verify call"
    );
    assert!(
        rs.contains("crc32_compute"),
        "should contain crc32_compute call"
    );
    assert!(
        rs.contains("Error::Checksum"),
        "should contain Checksum error"
    );
}

#[test]
fn codegen_rust_checksum_fletcher16() {
    let src = "packet P { data: u32, @checksum(fletcher16) checksum: u16 }";
    let rs = generate_rust(src);
    assert!(
        rs.contains("fletcher16_verify"),
        "should contain fletcher16_verify call"
    );
    assert!(
        rs.contains("fletcher16_compute"),
        "should contain fletcher16_compute call"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// @strict VarInt noncanonical checks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_rust_varint_strict() {
    let src = r#"@strict
    type VarInt = { prefix: bits[2], value: match prefix {
        0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62],
    } }"#;
    let rs = generate_rust(src);
    assert!(rs.contains("Noncanonical"));
}

#[test]
fn codegen_rust_varint_not_strict() {
    let src = r#"type VarInt = { prefix: bits[2], value: match prefix {
        0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62],
    } }"#;
    let rs = generate_rust(src);
    assert!(!rs.contains("Noncanonical"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Delegate transition
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_rust_delegate_transition() {
    let src = r#"
        state machine Child { state A state B [terminal] initial A
            transition A -> B { on finish } }
        state machine Parent {
            state Active { child: Child }
            state Done [terminal]
            initial Active
            transition Active -> Active {
                on child_ev(id: u8, ev: u8)
                delegate src.child <- ev
            }
            transition Active -> Done { on finish }
        }
    "#;
    let rs = generate_rust(src);
    // Delegate transition should auto-copy and reference child SM
    assert!(rs.contains("clone") || rs.contains("delegate"));
    assert!(
        rs.contains("child"),
        "should reference child field in delegate"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced SM expression codegen
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_rust_sm_in_state() {
    let src = r#"
        state machine S {
            state Active { status: u8 = 0 }
            state Done [terminal]
            initial Active
            transition Active -> Done {
                on finish
                guard src.status == 1
                action {}
            }
            transition * -> Done { on abort }
        }
    "#;
    let rs = generate_rust(src);
    assert!(rs.contains("dispatch"), "should contain dispatch method");
}

#[test]
fn codegen_rust_sm_action_field_copy() {
    let src = r#"
        state machine S {
            state A { count: u8 = 0, data: u16 = 0 }
            state B [terminal]
            initial A
            transition A -> A {
                on tick
                guard src.count < 10
                action { dst.count = src.count + 1; dst.data = src.data; }
            }
            transition A -> B { on done }
        }
    "#;
    let rs = generate_rust(src);
    assert!(rs.contains("count"), "should contain count field in action");
    assert!(rs.contains("data"), "should contain data field in action");
    // Should contain guard check
    assert!(
        rs.contains("< 10") || rs.contains("<10"),
        "should contain guard comparison"
    );
}

#[test]
fn codegen_rust_sm_plus_assign() {
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
    let rs = generate_rust(src);
    assert!(rs.contains("count"), "should contain count field");
    assert!(rs.contains("+ 1"), "should contain increment expression");
}

#[test]
fn codegen_capsule_serialize_includes_payload() {
    let src = r#"
        capsule C {
            tag: u8,
            length: u16,
            payload: match tag within length {
                1 => Data { x: u8, y: u16 },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let rs = generate_rust(src);

    // serialize must write payload variants, not just header
    let serialize_section = rs.split("fn serialize").nth(1).unwrap_or("");
    assert!(
        serialize_section.contains("match"),
        "serialize should match on payload variant, got:\n{}",
        rs
    );
    assert!(
        serialize_section.contains("Data"),
        "serialize should handle Data variant, got:\n{}",
        rs
    );
    assert!(
        serialize_section.contains("Unknown"),
        "serialize should handle Unknown variant, got:\n{}",
        rs
    );
}

#[test]
fn codegen_capsule_serialized_len_includes_payload() {
    let src = r#"
        capsule C {
            tag: u8,
            length: u16,
            payload: match tag within length {
                1 => Data { x: u8, y: u16 },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let rs = generate_rust(src);

    let len_section = rs.split("fn serialized_len").nth(1).unwrap_or("");
    assert!(
        len_section.contains("match"),
        "serialized_len should match on payload variant, got:\n{}",
        rs
    );
}
