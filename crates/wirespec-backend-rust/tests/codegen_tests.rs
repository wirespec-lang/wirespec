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

// ═══════════════════════════════════════════════════════════════════════════
// Bug fixes: SM child type mapping, delegate dispatch, in_state matching
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_sm_child_sm_field_type() {
    let src = r#"
        state machine ChildSm {
            state Active {}
            state Done [terminal]
            initial Active
            transition Active -> Done { on finish }
        }
        state machine ParentSm {
            state Running { child: ChildSm }
            state Stopped [terminal]
            initial Running
            transition Running -> Stopped { on stop }
        }
    "#;
    let rs = generate_rust(src);
    // child field should be ChildSm type, NOT u64
    assert!(
        rs.contains("child: ChildSm"),
        "SM field should use child SM type, not u64. Got:\n{}",
        rs
    );
    assert!(
        !rs.contains("child: u64"),
        "SM field should NOT be u64. Got:\n{}",
        rs
    );
}

#[test]
fn codegen_sm_array_of_child_sm_field_type() {
    let src = r#"
        state machine PathState {
            state Active { path_id: u8 }
            state Closed [terminal]
            initial Active
            transition Active -> Closed { on close_path }
        }
        state machine Conn {
            state Connected { paths: [PathState; 4], count: u8 = 1 }
            state Done [terminal]
            initial Connected
            transition Connected -> Done { on stop }
        }
    "#;
    let rs = generate_rust(src);
    // paths field should be Vec<PathState>, not Vec<u64> or u64
    assert!(
        rs.contains("Vec<PathState>"),
        "Array-of-SM field should use Vec<ChildSm>. Got:\n{}",
        rs
    );
    assert!(
        !rs.contains("paths: u64"),
        "Array-of-SM field should NOT be u64. Got:\n{}",
        rs
    );
}

#[test]
fn codegen_sm_delegate_generates_dispatch_call() {
    let src = r#"
        state machine Child {
            state A
            state B [terminal]
            initial A
            transition A -> B { on finish }
        }
        state machine Parent {
            state Active { child: Child }
            state Done [terminal]
            initial Active
            transition Active -> Active {
                on child_ev(ev: u8)
                delegate src.child <- ev
            }
            transition Active -> Done { on finish }
        }
    "#;
    let rs = generate_rust(src);
    // Should contain actual dispatch call, not TODO comments
    assert!(
        !rs.contains("// TODO: map"),
        "Delegate should not contain TODO map comment. Got:\n{}",
        rs
    );
    assert!(
        !rs.contains("// TODO: dispatch to"),
        "Delegate should not contain TODO dispatch comment. Got:\n{}",
        rs
    );
    assert!(
        rs.contains(".dispatch("),
        "Delegate should generate dispatch call. Got:\n{}",
        rs
    );
}

#[test]
fn codegen_sm_delegate_redispatches_child_state_changed_from_new_state() {
    let src = r#"
        state machine ChildSm {
            state Ready { value: u8 }
            state Done [terminal]
            initial Ready
            transition Ready -> Done { on finish }
            transition * -> Done { on force_close }
        }
        state machine ParentSm {
            state Running { child: ChildSm }
            state Complete [terminal]
            initial Running
            transition Running -> Running {
                on forward_to_child(child_ev_tag: u8)
                delegate src.child <- child_ev_tag
            }
            transition Running -> Complete {
                on child_state_changed
                guard src.child in_state(Done)
            }
            transition * -> Complete { on shutdown }
        }
    "#;
    let rs = generate_rust(src);
    assert!(
        rs.contains("let _ = new_state.dispatch(&ParentSmEvent::ChildStateChanged);"),
        "delegate should redispatch child_state_changed from the updated parent state. Got:\n{}",
        rs
    );
    assert!(
        !rs.contains("let _ = self.dispatch(&ParentSmEvent::ChildStateChanged);"),
        "delegate should not redispatch child_state_changed from the stale parent state. Got:\n{}",
        rs
    );
}

#[test]
fn codegen_sm_in_state_unit_variant_no_braces() {
    let src = r#"
        state machine ChildSm {
            state Active {}
            state Done [terminal]
            initial Active
            transition Active -> Done { on finish }
        }
        state machine ParentSm {
            state Running { child: ChildSm }
            state Complete [terminal]
            initial Running
            transition Running -> Complete {
                on child_done
                guard src.child in_state(Done)
            }
            transition * -> Complete { on shutdown }
        }
    "#;
    let rs = generate_rust(src);
    // Done is terminal (no fields), should NOT have { .. }
    assert!(
        !rs.contains("ChildSm::Done { .. }"),
        "Terminal state should not have {{ .. }}. Got:\n{}",
        rs
    );
    // Should have just ChildSm::Done without braces
    assert!(
        rs.contains("ChildSm::Done)"),
        "Terminal state should be ChildSm::Done without braces. Got:\n{}",
        rs
    );
}

#[test]
fn codegen_rust_escapes_keyword_identifiers() {
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
        capsule TlvContainer {
            type: VarInt,
            length: VarInt,
            payload: match type within length {
                1 => Data { value: u8 },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let rs = generate_rust(src);
    assert!(
        rs.contains("pub r#type: u64"),
        "field name should be escaped. Got:\n{rs}"
    );
    assert!(
        rs.contains("let r#type ="),
        "local binding should be escaped. Got:\n{rs}"
    );
    assert!(
        rs.contains("match r#type"),
        "match expression should use escaped identifier. Got:\n{rs}"
    );
    assert!(
        rs.contains("self.r#type"),
        "field access should use escaped identifier. Got:\n{rs}"
    );
}

#[test]
fn codegen_rust_threads_named_lifetimes_into_variant_fields() {
    let src = r#"
        packet MqttString {
            length: u16,
            data: bytes[length],
        }
        capsule MqttPacket {
            tag: u8,
            length: u16,
            payload: match tag within length {
                1 => Connect {
                    protocol_name: MqttString,
                    username: if tag == 1 { MqttString },
                },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let rs = generate_rust(src);
    assert!(
        rs.contains("protocol_name: MqttString<'a>"),
        "named packet fields inside lifetime-carrying variants should keep the lifetime. Got:\n{rs}"
    );
    assert!(
        rs.contains("username: Option<MqttString<'a>>"),
        "optional named packet fields inside lifetime-carrying variants should keep the lifetime. Got:\n{rs}"
    );
}

#[test]
fn codegen_rust_uses_from_fn_for_fixed_arrays() {
    let src = r#"
        packet Entry { key: u8, val: u16 }
        packet Container {
            count: u8,
            @max_len(128)
            entries: [Entry; count],
        }
    "#;
    let rs = generate_rust(src);
    assert!(
        rs.contains("std::array::from_fn(|_| Default::default())"),
        "fixed arrays should initialize through from_fn so composite elements do not require Copy. Got:\n{rs}"
    );
}

#[test]
fn codegen_rust_frame_varint_aliases_tag_refs_in_parse_and_len() {
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
        frame QuicFrame = match frame_type: VarInt {
            0x1c..=0x1d => ConnectionClose {
                error_code: VarInt,
                offending_frame_type: if frame_type == 0x1c { VarInt },
            },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let rs = generate_rust(src);
    assert!(
        rs.contains("if (_tag_val == 28)"),
        "frame parse should rewrite tag field references to _tag_val. Got:\n{rs}"
    );
    assert!(
        rs.contains("len += var_int_wire_size(28);"),
        "frame serialized_len should size the representative tag in the matching arm. Got:\n{rs}"
    );
}

#[test]
fn codegen_rust_capsule_expr_tag_keeps_within_and_match_separate() {
    let src = r#"
        type MqttLength = varint {
            continuation_bit: msb,
            value_bits: 7,
            max_bytes: 4,
            byte_order: little,
        }
        capsule MqttPacket {
            type_and_flags: u8,
            remaining_length: MqttLength,
            payload: match (type_and_flags >> 4) within remaining_length {
                1 => Connect { protocol_level: u8 },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let rs = generate_rust(src);
    assert!(
        rs.contains("let mut sub = cur.sub_cursor(remaining_length as usize)?;"),
        "capsule parse should size the sub-cursor from the within field. Got:\n{rs}"
    );
    assert!(
        rs.contains("let payload = match (type_and_flags >> 4) {"),
        "capsule parse should dispatch payload variants with the tag expression. Got:\n{rs}"
    );
}
