use std::sync::Arc;

use wirespec_backend_api::*;
use wirespec_backend_c::*;

fn generate_c(src: &str) -> (String, String) {
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem =
        wirespec_sema::analyze(&ast, wirespec_sema::ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions::default()),
        checksum_bindings: Arc::new(checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };

    let lowered = backend.lower(&codec, &ctx).unwrap();
    (lowered.header_content.clone(), lowered.source_content.clone())
}

#[test]
fn codegen_simple_packet_header() {
    let (header, _) =
        generate_c("packet UdpDatagram { src_port: u16, dst_port: u16, length: u16, checksum: u16 }");
    assert!(header.contains("typedef struct"), "missing typedef struct");
    assert!(
        header.contains("test_udp_datagram_t"),
        "missing type name"
    );
    assert!(
        header.contains("uint16_t src_port"),
        "missing src_port field"
    );
    assert!(
        header.contains("wirespec_result_t test_udp_datagram_parse"),
        "missing parse decl"
    );
    assert!(
        header.contains("wirespec_result_t test_udp_datagram_serialize"),
        "missing serialize decl"
    );
}

#[test]
fn codegen_simple_packet_source() {
    let (_, source) = generate_c("packet P { x: u8, y: u16 }");
    assert!(
        source.contains("wirespec_cursor_read_u8"),
        "missing u8 cursor read"
    );
    assert!(
        source.contains("wirespec_cursor_read_u16be"),
        "missing u16be cursor read"
    );
    assert!(
        source.contains("wirespec_write_u8"),
        "missing u8 write"
    );
    assert!(
        source.contains("wirespec_write_u16be"),
        "missing u16be write"
    );
}

#[test]
fn codegen_packet_with_require() {
    let (_, source) = generate_c("packet P { length: u16, require length >= 8 }");
    assert!(
        source.contains("WIRESPEC_ERR_CONSTRAINT"),
        "missing constraint error"
    );
}

#[test]
fn codegen_packet_with_optional() {
    let (header, source) =
        generate_c("packet P { flags: u8, extra: if flags & 0x01 { u16 } }");
    assert!(
        header.contains("bool has_extra"),
        "missing has_extra flag in header"
    );
    assert!(
        source.contains("has_extra"),
        "missing has_extra in source"
    );
}

#[test]
fn codegen_bytes_field() {
    let (header, _) = generate_c("packet P { data: bytes[remaining] }");
    assert!(
        header.contains("wirespec_bytes_t data"),
        "missing bytes field"
    );
}

#[test]
fn codegen_array_field() {
    let (header, source) = generate_c("packet P { count: u8, items: [u8; count] }");
    assert!(header.contains("uint8_t items["), "missing array field");
    assert!(
        header.contains("uint32_t items_count"),
        "missing array count field"
    );
    assert!(source.contains("for"), "missing for loop");
}

#[test]
fn codegen_bitgroup() {
    let (_, source) = generate_c("packet P { a: bits[4], b: bits[4], c: u16 }");
    assert!(source.contains(">>"), "missing shift for bit extract");
    assert!(source.contains("& 0xf"), "missing mask for 4-bit field");
}

#[test]
fn codegen_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x01 => B { x: u8 },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let (header, source) = generate_c(src);
    assert!(header.contains("tag_t"), "missing tag enum type in header");
    assert!(header.contains("union"), "missing union in header");
    assert!(source.contains("switch"), "missing switch in source");
}

#[test]
fn codegen_capsule() {
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
    let (_, source) = generate_c(src);
    assert!(
        source.contains("wirespec_cursor_sub"),
        "missing sub-cursor for within"
    );
}

#[test]
fn codegen_derived_field() {
    let (header, _) =
        generate_c("packet P { flags: u8, let is_set: bool = (flags & 1) != 0 }");
    assert!(
        header.contains("bool is_set"),
        "missing derived field in header"
    );
}

#[test]
fn codegen_enum() {
    let (header, _) = generate_c("enum E: u8 { A = 0, B = 1 }");
    // Now uses typedef + #define pattern instead of C enum
    assert!(
        header.contains("typedef uint8_t"),
        "missing enum underlying typedef in header"
    );
    assert!(
        header.contains("#define"),
        "missing enum member #define in header"
    );
}

#[test]
fn codegen_artifact_emission() {
    let ast = wirespec_syntax::parse("packet P { x: u8 }").unwrap();
    let sem =
        wirespec_sema::analyze(&ast, wirespec_sema::ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions::default()),
        checksum_bindings: Arc::new(checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };

    let mut sink = MemorySink::new();
    let output = backend.lower_and_emit(&codec, &ctx, &mut sink).unwrap();
    assert_eq!(output.artifacts.len(), 2, "should emit .h + .c");
    assert_eq!(sink.artifacts.len(), 2, "sink should have 2 artifacts");
    assert!(
        output.artifacts[0]
            .relative_path
            .to_string_lossy()
            .ends_with(".h"),
        "first artifact should be .h"
    );
    assert!(
        output.artifacts[1]
            .relative_path
            .to_string_lossy()
            .ends_with(".c"),
        "second artifact should be .c"
    );
}

#[test]
fn codegen_void_r_suppression() {
    let (_, source) = generate_c("packet P { x: u8 }");
    assert!(source.contains("(void)r;"), "missing (void)r suppression");
}

#[test]
fn codegen_signed_type_cast() {
    let (_, source) = generate_c("packet P { x: i8 }");
    // Parse should cast the pointer for signed type
    assert!(
        source.contains("(uint8_t *)"),
        "missing unsigned pointer cast for i8 parse"
    );
    // Serialize should cast the value for signed type
    assert!(
        source.contains("(uint8_t)"),
        "missing unsigned value cast for i8 serialize"
    );
}

#[test]
fn codegen_frame_raw_tag() {
    let src = "frame F = match tag: u8 { 0 => A {}, _ => B { data: bytes[remaining] } }";
    let (header, source) = generate_c(src);
    // Header should have raw tag field in the frame struct
    assert!(
        header.contains("uint8_t frame_type;"),
        "missing raw tag field in header"
    );
    // Source should store the raw tag value
    assert!(
        source.contains("frame_type = _tag_val"),
        "missing raw tag store in parse"
    );
    // Serialize should write frame_type
    assert!(
        source.contains("val->frame_type"),
        "missing frame_type in serialize"
    );
}

#[test]
fn codegen_frame_default_case() {
    // Frames require a wildcard (_) branch per spec §4.2.
    // Verify that the wildcard variant generates a default: case.
    let src = "frame F = match tag: u8 { 0 => A {}, 1 => B { x: u8 }, _ => Unknown { data: bytes[remaining] } }";
    let (_, source) = generate_c(src);
    assert!(
        source.contains("default:"),
        "missing default case for wildcard variant"
    );
}

// ── VarInt codegen tests ──

#[test]
fn codegen_varint_prefix_match() {
    let src = r#"type VarInt = { prefix: bits[2], value: match prefix { 0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62] } }"#;
    let (header, source) = generate_c(src);
    assert!(header.contains("typedef uint64_t"), "missing VarInt typedef");
    assert!(header.contains("_parse"), "missing parse declaration");
    assert!(header.contains("_serialize"), "missing serialize declaration");
    assert!(header.contains("_wire_size"), "missing wire_size declaration");
    assert!(source.contains("prefix"), "missing prefix in parse");
}

#[test]
fn codegen_cont_varint() {
    let src = r#"type MqttLen = varint { continuation_bit: msb, value_bits: 7, max_bytes: 4, byte_order: little }"#;
    let (header, source) = generate_c(src);
    assert!(header.contains("typedef uint64_t"), "missing VarInt typedef");
    assert!(
        source.contains("0x80") || source.contains("0x7F") || source.contains("0x7f"),
        "missing continuation bit mask"
    );
}

// ── Checksum codegen tests ──

#[test]
fn codegen_checksum_internet() {
    let src = "packet P { data: u32, @checksum(internet) checksum: u16 }";
    let (_, source) = generate_c(src);
    assert!(
        source.contains("wirespec_internet_checksum"),
        "missing internet checksum verify"
    );
    assert!(
        source.contains("WIRESPEC_ERR_CHECKSUM"),
        "missing checksum error"
    );
    assert!(
        source.contains("wirespec_internet_checksum_compute"),
        "missing internet checksum compute"
    );
}

// ── Const/Enum codegen tests ──

#[test]
fn codegen_const_define() {
    let src = "const MAX_LEN: u8 = 20\npacket P { x: u8 }";
    let (header, _) = generate_c(src);
    assert!(header.contains("#define"), "missing #define for const");
    assert!(header.contains("MAX_LEN"), "missing const name in #define");
}

#[test]
fn codegen_enum_typedef_define() {
    let src = "enum E: u8 { A = 0, B = 1 }\npacket P { x: u8 }";
    let (header, _) = generate_c(src);
    assert!(header.contains("typedef"), "missing typedef for enum");
    assert!(header.contains("#define"), "missing #define for enum members");
}

// ── State machine codegen tests ──

#[test]
fn codegen_state_machine_header() {
    let src = r#"
        state machine PathState {
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
        }
    "#;
    let (header, _source) = generate_c(src);

    // State tag enum
    assert!(
        header.contains("_state_tag_t"),
        "missing state tag enum type"
    );
    assert!(
        header.contains("TEST_PATH_STATE_INIT"),
        "missing INIT state tag member"
    );
    assert!(
        header.contains("TEST_PATH_STATE_ACTIVE"),
        "missing ACTIVE state tag member"
    );
    assert!(
        header.contains("TEST_PATH_STATE_CLOSED"),
        "missing CLOSED state tag member"
    );

    // State data struct (tagged union)
    assert!(
        header.contains("test_path_state_t"),
        "missing state machine type name"
    );
    assert!(
        header.contains("uint8_t path_id"),
        "missing path_id field in state struct"
    );
    assert!(
        header.contains("uint64_t rtt"),
        "missing rtt field in state struct"
    );
    assert!(
        header.contains("uint8_t _unused"),
        "missing _unused placeholder for terminal state"
    );

    // Event tag enum
    assert!(
        header.contains("_event_tag_t"),
        "missing event tag enum type"
    );
    assert!(
        header.contains("EVENT_ACTIVATE"),
        "missing ACTIVATE event tag member"
    );
    assert!(
        header.contains("EVENT_CLOSE"),
        "missing CLOSE event tag member"
    );
    assert!(
        header.contains("EVENT_ERROR"),
        "missing ERROR event tag member"
    );

    // Event data struct
    assert!(
        header.contains("test_path_state_event_t"),
        "missing event struct type name"
    );

    // Dispatch declaration
    assert!(
        header.contains("_dispatch"),
        "missing dispatch function declaration"
    );

    // Init helper
    assert!(
        header.contains("_init"),
        "missing init helper function"
    );
    assert!(
        header.contains("static inline void"),
        "init helper should be static inline"
    );
}

#[test]
fn codegen_state_machine_source() {
    let src = r#"
        state machine PathState {
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
        }
    "#;
    let (_header, source) = generate_c(src);

    // Dispatch function
    assert!(
        source.contains("src_tag"),
        "dispatch should use src_tag variable"
    );
    assert!(
        source.contains("WIRESPEC_OK"),
        "dispatch should return WIRESPEC_OK on successful transition"
    );
    assert!(
        source.contains("WIRESPEC_ERR_INVALID_STATE"),
        "dispatch should return WIRESPEC_ERR_INVALID_STATE as fallback"
    );
    assert!(
        source.contains("*sm = dst"),
        "dispatch should apply transition via *sm = dst"
    );
}

#[test]
fn codegen_state_machine_with_guard() {
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
            transition A -> B { on stop }
        }
    "#;
    let (header, source) = generate_c(src);

    // Header checks
    assert!(header.contains("_state_tag_t"), "missing state tag enum");
    assert!(header.contains("_event_tag_t"), "missing event tag enum");
    assert!(header.contains("_dispatch"), "missing dispatch decl");
    assert!(header.contains("uint8_t count"), "missing count field");

    // Source checks - guard
    assert!(
        source.contains("sm->a.count"),
        "guard should reference sm->a.count"
    );
    // Source checks - action
    assert!(
        source.contains("dst.a.count"),
        "action should set dst.a.count"
    );
    assert!(
        source.contains("src_tag"),
        "dispatch should use src_tag"
    );
    assert!(
        source.contains("WIRESPEC_OK"),
        "dispatch should return WIRESPEC_OK"
    );
    assert!(
        source.contains("WIRESPEC_ERR_INVALID_STATE"),
        "dispatch should return WIRESPEC_ERR_INVALID_STATE"
    );
}

// ── Enum endianness propagation tests ──

#[test]
fn codegen_enum_little_endian() {
    let src = "@endian little\nmodule test\nenum E: u16 { A = 1 }\npacket P { code: E }";
    let (_, source) = generate_c(src);
    assert!(source.contains("read_u16le"), "enum field should use read_u16le, not read_u16be");
}

// ── Range pattern safeguard tests ──

#[test]
fn codegen_frame_range_pattern() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x08..=0x0f => B { x: u8 },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let (_, source) = generate_c(src);
    // All values in the range should appear as case labels
    assert!(source.contains("case 8:"), "missing case 8 for range start");
    assert!(source.contains("case 15:"), "missing case 15 for range end");
    assert!(source.contains("switch"), "missing switch statement");
}

// ── Fuzz harness codegen tests ──

#[test]
fn codegen_fuzz_harness() {
    let ast = wirespec_syntax::parse("packet P { x: u8, y: u16 }").unwrap();
    let sem =
        wirespec_sema::analyze(&ast, wirespec_sema::ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions { emit_fuzz_harness: true }),
        checksum_bindings: Arc::new(checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };

    let mut sink = MemorySink::new();
    let output = backend.lower_and_emit(&codec, &ctx, &mut sink).unwrap();
    assert_eq!(output.artifacts.len(), 3, "should emit .h + .c + _fuzz.c");
    assert_eq!(sink.artifacts.len(), 3, "sink should have 3 artifacts");

    let fuzz_content = String::from_utf8_lossy(&sink.artifacts[2].1);
    assert!(fuzz_content.contains("LLVMFuzzerTestOneInput"), "missing LLVMFuzzerTestOneInput");
    assert!(fuzz_content.contains("__builtin_trap"), "missing __builtin_trap");
    assert!(fuzz_content.contains("memcmp"), "missing memcmp");
    assert!(fuzz_content.contains("test_p_parse"), "missing parse function name");
    assert!(fuzz_content.contains("test_p_serialize"), "missing serialize function name");
    assert!(fuzz_content.contains("test_p_t"), "missing type name");
}

#[test]
fn codegen_no_fuzz_by_default() {
    let ast = wirespec_syntax::parse("packet P { x: u8 }").unwrap();
    let sem =
        wirespec_sema::analyze(&ast, wirespec_sema::ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions::default()),
        checksum_bindings: Arc::new(checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };

    let mut sink = MemorySink::new();
    let output = backend.lower_and_emit(&codec, &ctx, &mut sink).unwrap();
    assert_eq!(output.artifacts.len(), 2, "should emit only .h + .c (no fuzz)");
    assert_eq!(sink.artifacts.len(), 2, "sink should have 2 artifacts (no fuzz)");
}

#[test]
fn codegen_fuzz_targets_frame_first() {
    let src = r#"frame F = match tag: u8 { 0 => A {}, _ => B { data: bytes[remaining] } }
    packet P { x: u8 }"#;
    let ast = wirespec_syntax::parse(src).unwrap();
    let sem =
        wirespec_sema::analyze(&ast, wirespec_sema::ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();

    let backend = CBackend;
    let ctx = BackendContext {
        module_name: "test".into(),
        module_prefix: "test".into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions { emit_fuzz_harness: true }),
        checksum_bindings: Arc::new(checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };

    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();
    let fuzz = lowered.fuzz_content.as_ref().expect("fuzz content should exist");
    assert!(fuzz.contains("test_f_parse"), "should target frame F, not packet P");
    assert!(fuzz.contains("test_f_serialize"), "should target frame F for serialize");
    assert!(fuzz.contains("test_f_t"), "should use frame F type name");
}

// ── Advanced SM codegen tests ──

#[test]
fn codegen_sm_in_state_guard() {
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
    let (header, source) = generate_c(src);
    assert!(source.contains("_dispatch"), "missing dispatch function");
    assert!(header.contains("TEST_S_ACTIVE"), "missing ACTIVE tag");
    assert!(header.contains("TEST_S_DONE"), "missing DONE tag");
    assert!(
        source.contains("sm->active.status"),
        "guard should reference sm->active.status"
    );
}

#[test]
fn codegen_sm_plus_assign() {
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
    let (_, source) = generate_c(src);
    assert!(source.contains("count"), "missing count field reference");
    assert!(source.contains("WIRESPEC_OK"), "missing WIRESPEC_OK return");
}

#[test]
fn codegen_frame_varint_tag_parse() {
    let src = r#"
        type VarInt = { prefix: bits[2], value: match prefix {
            0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62],
        } }
        frame F = match tag: VarInt {
            0x06 => Crypto { offset: u32 },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    let (_, source) = generate_c(src);
    // Should use VarInt parse, not read_u8
    assert!(source.contains("var_int_parse_cursor"));
    assert!(!source.contains("wirespec_cursor_read_u8(cur, &_tag_val)")
            || source.contains("var_int_parse_cursor"));
}

#[test]
fn codegen_fill_array() {
    let src = "packet P { items: [u8; fill] }";
    let (header, source) = generate_c(src);
    assert!(header.contains("items_count"));
    // Should have a loop parsing until cursor exhausted
    assert!(source.contains("wirespec_cursor_remaining"));
}

#[test]
fn codegen_fill_array_within() {
    let src = r#"
        packet P {
            length: u16,
            items: [u8; fill] within length,
        }
    "#;
    let (_, source) = generate_c(src);
    // Should create a sub-cursor bounded by length
    assert!(source.contains("wirespec_cursor_sub") || source.contains("_arr_sub"));
}
