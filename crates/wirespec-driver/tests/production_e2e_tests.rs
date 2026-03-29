use std::process::Command;
use std::sync::Arc;
use wirespec_backend_api::*;
use wirespec_sema::{ComplianceProfile, analyze};
use wirespec_syntax::parse;

fn full_pipeline_c(src: &str, prefix: &str) -> (String, String) {
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    let backend = wirespec_backend_c::CBackend;
    let ctx = BackendContext {
        module_name: prefix.into(),
        module_prefix: prefix.into(),
        source_prefixes: Default::default(),
        compliance_profile: "phase2_extended_current".into(),
        common_options: CommonOptions::default(),
        target_options: Box::new(CBackendOptions::default()),
        checksum_bindings: Arc::new(wirespec_backend_c::checksum_binding::CChecksumBindings),
        is_entry_module: true,
    };
    let lowered = Backend::lower(&backend, &codec, &ctx).unwrap();
    (
        lowered.header_content.clone(),
        lowered.source_content.clone(),
    )
}

fn generate_and_gcc(src: &str, prefix: &str) {
    let (header, source) = full_pipeline_c(src, prefix);
    let dir = std::path::PathBuf::from("/tmp/wirespec-prod-tests");
    std::fs::create_dir_all(&dir).unwrap();

    let runtime = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../runtime");

    std::fs::write(dir.join(format!("{prefix}.h")), &header).unwrap();
    std::fs::write(dir.join(format!("{prefix}.c")), &source).unwrap();

    let output = match Command::new("gcc")
        .args([
            "-Wall",
            "-Wextra",
            "-Werror",
            "-std=c11",
            "-fsyntax-only",
            "-I",
            &dir.to_string_lossy(),
            "-I",
            &runtime.to_string_lossy(),
        ])
        .arg(dir.join(format!("{prefix}.c")))
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("gcc not found or failed to execute: {e}");
            return; // skip if gcc unavailable
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("GCC failed for {prefix}:\n{stderr}");
    }
}

// ── Complex real-world patterns ──

#[test]
fn e2e_gcc_packet_with_bits_optional_require() {
    let src = r#"
        @endian big
        packet Complex {
            version: bits[4],
            header_len: bits[4],
            flags: u8,
            total_length: u16,
            optional_id: if flags & 0x01 { u32 },
            data_length: u8,
            require data_length <= 20,
            data: bytes[length: data_length],
        }
    "#;
    generate_and_gcc(src, "prod_complex");
}

#[test]
fn e2e_gcc_frame_with_ranges() {
    let src = r#"
        frame Protocol = match msg_type: u8 {
            0x00 => Heartbeat {},
            0x01..=0x0F => Data {
                sequence: u16,
                payload: bytes[remaining],
            },
            0x10..=0x1F => Control {
                command: u8,
                param: u32,
            },
            _ => Unknown { raw: bytes[remaining] },
        }
    "#;
    generate_and_gcc(src, "prod_frame_ranges");
}

#[test]
fn e2e_gcc_capsule_with_bitfields() {
    let src = r#"
        capsule TlvMessage {
            type_field: bits[4],
            priority: bits[4],
            length: u16,
            payload: match type_field within length {
                0 => Ping {},
                1 => Data { content: bytes[remaining] },
                _ => Unknown { raw: bytes[remaining] },
            },
        }
    "#;
    generate_and_gcc(src, "prod_capsule_bits");
}

#[test]
fn e2e_gcc_varint_packet() {
    let src = r#"
        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6], 0b01 => bits[14],
                0b10 => bits[30], 0b11 => bits[62],
            },
        }
        packet Message {
            msg_type: VarInt,
            payload: bytes[remaining],
        }
    "#;
    generate_and_gcc(src, "prod_varint_packet");
}

#[test]
fn e2e_gcc_checksum_internet() {
    let src = r#"
        @endian big
        packet IpHeader {
            version_ihl: u8,
            tos: u8,
            total_length: u16,
            identification: u16,
            flags_offset: u16,
            ttl: u8,
            protocol: u8,
            @checksum(internet)
            header_checksum: u16,
            src_addr: u32,
            dst_addr: u32,
        }
    "#;
    generate_and_gcc(src, "prod_checksum_ip");
}

#[test]
fn e2e_gcc_enum_and_array() {
    let src = r#"
        enum Color: u8 { Red = 0, Green = 1, Blue = 2 }
        packet Palette {
            count: u8,
            @max_len(256)
            colors: [u8; count],
        }
    "#;
    generate_and_gcc(src, "prod_enum_array");
}

#[test]
fn e2e_gcc_state_machine() {
    let src = r#"
        state machine Connection {
            state Idle
            state Active { retries: u8 = 0 }
            state Closed [terminal]
            initial Idle
            transition Idle -> Active {
                on connect
                action { dst.retries = 0; }
            }
            transition Active -> Active {
                on retry
                guard src.retries < 3
                action { dst.retries = src.retries + 1; }
            }
            transition Active -> Closed { on disconnect }
            transition * -> Closed { on timeout }
        }
    "#;
    generate_and_gcc(src, "prod_sm_connection");
}

#[test]
fn e2e_gcc_continuation_varint() {
    let src = r#"
        type Length = varint {
            continuation_bit: msb,
            value_bits: 7,
            max_bytes: 4,
            byte_order: little,
        }
        packet Message {
            length: Length,
            data: bytes[length: length],
        }
    "#;
    generate_and_gcc(src, "prod_cont_varint");
}

#[test]
fn e2e_gcc_signed_types() {
    let src = "packet P { a: i8, b: i16, c: i32, d: i64 }";
    generate_and_gcc(src, "prod_signed");
}

#[test]
fn e2e_gcc_multiple_packets() {
    let src = r#"
        packet Header { magic: u32, version: u16, length: u16 }
        packet Payload { data: bytes[remaining] }
    "#;
    generate_and_gcc(src, "prod_multi_packet");
}

#[test]
fn e2e_gcc_bytes_fixed() {
    let src = "packet P { mac: bytes[6], data: bytes[remaining] }";
    generate_and_gcc(src, "prod_bytes_fixed");
}

#[test]
fn e2e_gcc_derived_field() {
    let src = "packet P { flags: u8, let is_set: bool = (flags & 1) != 0 }";
    generate_and_gcc(src, "prod_derived_field");
}

#[test]
fn e2e_gcc_const_and_require() {
    let src = r#"
        packet P {
            length: u8,
            require length <= 100,
            data: bytes[length: length],
        }
    "#;
    generate_and_gcc(src, "prod_const_require");
}

#[test]
fn e2e_gcc_packet_all_unsigned_widths() {
    let src = "packet P { a: u8, b: u16, c: u24, d: u32, e: u64 }";
    generate_and_gcc(src, "prod_all_unsigned");
}

#[test]
fn e2e_gcc_16bit_bitgroup() {
    let src = r#"
        @endian big
        packet P {
            a: bits[4],
            b: bits[12],
            c: u8,
        }
    "#;
    generate_and_gcc(src, "prod_bitgroup_16");
}

#[test]
fn e2e_gcc_32bit_bitgroup() {
    let src = r#"
        @endian big
        packet P {
            a: bits[4],
            b: bits[12],
            c: bits[16],
            d: u8,
        }
    "#;
    generate_and_gcc(src, "prod_bitgroup_32");
}

#[test]
fn e2e_gcc_bytes_length_or_remaining() {
    let src = r#"
        packet P {
            flags: u8,
            len: if flags & 0x01 { u16 },
            data: bytes[length_or_remaining: len],
        }
    "#;
    generate_and_gcc(src, "prod_bytes_lor");
}

#[test]
fn e2e_gcc_frame_single_variant_wildcard() {
    let src = r#"
        frame F = match t: u8 {
            _ => Catch { data: bytes[remaining] },
        }
    "#;
    generate_and_gcc(src, "prod_frame_wildcard");
}

#[test]
fn e2e_gcc_capsule_with_require() {
    let src = r#"
        capsule C {
            type_field: u8,
            length: u16,
            require length >= 3,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => U { data: bytes[remaining] },
            },
        }
    "#;
    generate_and_gcc(src, "prod_capsule_require");
}

// ── Pipeline correctness (non-GCC) ──

#[test]
fn e2e_pipeline_preserves_multiple_consts() {
    let src = r#"
        const A: u8 = 1
        const B: u16 = 2
        const C: u32 = 3
        packet P { x: u8 }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    assert_eq!(codec.consts.len(), 3);
}

#[test]
fn e2e_pipeline_preserves_multiple_enums() {
    let src = r#"
        enum A: u8 { X = 0 }
        enum B: u16 { Y = 100 }
        packet P { x: u8 }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    assert_eq!(codec.enums.len(), 2);
}

#[test]
fn e2e_pipeline_frame_variant_ordinals() {
    let src = r#"
        frame F = match t: u8 {
            0 => A {},
            1 => B { x: u8 },
            _ => C { data: bytes[remaining] },
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    assert_eq!(codec.frames[0].variants[0].ordinal, 0);
    assert_eq!(codec.frames[0].variants[1].ordinal, 1);
    assert_eq!(codec.frames[0].variants[2].ordinal, 2);
}

#[test]
fn e2e_pipeline_checksum_crc32() {
    let src = "packet P { data: u32, @checksum(crc32) c: u32 }";
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
    let layout = wirespec_layout::lower(&sem).unwrap();
    let codec = wirespec_codec::lower(&layout).unwrap();
    let plan = codec.packets[0].checksum_plan.as_ref().unwrap();
    assert_eq!(plan.algorithm_id, "crc32");
    assert_eq!(
        plan.verify_mode,
        wirespec_codec::ir::ChecksumVerifyMode::RecomputeCompare
    );
}

#[test]
fn e2e_c_output_contains_parse_and_serialize() {
    let (h, _) = full_pipeline_c("packet P { x: u8, y: u16 }", "test");
    assert!(h.contains("test_p_parse"));
    assert!(h.contains("test_p_serialize"));
}

#[test]
fn e2e_c_output_frame_has_switch() {
    let src = r#"frame F = match t: u8 {
        0 => A {},
        1 => B { x: u8 },
        _ => C { data: bytes[remaining] },
    }"#;
    let (_, c) = full_pipeline_c(src, "test");
    assert!(c.contains("switch"));
}

#[test]
fn e2e_c_output_capsule_has_sub_cursor() {
    let src = r#"capsule C {
        type_field: u8, length: u16,
        payload: match type_field within length {
            0 => D { data: bytes[remaining] },
            _ => U { data: bytes[remaining] },
        },
    }"#;
    let (_, c) = full_pipeline_c(src, "test");
    assert!(c.contains("wirespec_cursor_sub"));
}
