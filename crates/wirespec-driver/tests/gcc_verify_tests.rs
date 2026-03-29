// crates/wirespec-driver/tests/gcc_verify_tests.rs
//
// Integration tests that generate C code from .wspec sources via the Rust
// compiler pipeline, write the output to /tmp/wirespec-verify/, and then
// run `gcc -fsyntax-only` on each to verify that the generated C compiles
// without errors or warnings.

use std::sync::Arc;
use wirespec_backend_api::*;
use wirespec_sema::ComplianceProfile;
use wirespec_syntax::parse;

fn generate_c(src: &str, prefix: &str) -> (String, String) {
    let ast = parse(src).unwrap();
    let sem =
        wirespec_sema::analyze(&ast, ComplianceProfile::default(), &Default::default()).unwrap();
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

fn write_and_gcc(header: &str, source: &str, prefix: &str) -> (bool, String) {
    let dir = std::path::PathBuf::from("/tmp/wirespec-verify");
    std::fs::create_dir_all(&dir).unwrap();

    let runtime_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../runtime");

    let h_path = dir.join(format!("{prefix}.h"));
    let c_path = dir.join(format!("{prefix}.c"));
    std::fs::write(&h_path, header).unwrap();
    std::fs::write(&c_path, source).unwrap();

    let output = match std::process::Command::new("gcc")
        .args([
            "-Wall",
            "-Wextra",
            "-Werror",
            "-std=c11",
            "-fsyntax-only",
            "-I",
            &dir.to_string_lossy(),
            "-I",
            &runtime_dir.to_string_lossy(),
        ])
        .arg(&c_path)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            let msg = format!("gcc not found or failed to execute: {e}");
            eprintln!("{msg}");
            return (false, msg);
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output.status.success(), stderr)
}

struct TestCase {
    name: &'static str,
    prefix: &'static str,
    source: &'static str,
}

fn test_cases() -> Vec<TestCase> {
    vec![
        TestCase {
            name: "simple_packet",
            prefix: "test1",
            source: "packet UdpHeader { src_port: u16, dst_port: u16, length: u16, checksum: u16 }",
        },
        TestCase {
            name: "packet_with_require",
            prefix: "test2",
            source: "packet P { length: u16, require length >= 8, data: bytes[length: length - 8] }",
        },
        TestCase {
            name: "packet_with_optional",
            prefix: "test3",
            source: "packet P { flags: u8, extra: if flags & 0x01 { u16 } }",
        },
        TestCase {
            name: "packet_with_bits",
            prefix: "test4",
            source: "packet P { a: bits[4], b: bits[4], c: u16 }",
        },
        TestCase {
            name: "packet_with_array",
            prefix: "test5",
            source: "packet P { count: u8, items: [u8; count] }",
        },
        TestCase {
            name: "frame",
            prefix: "test6",
            source: "frame F = match tag: u8 { 0 => A {}, 1 => B { x: u8 }, _ => C { data: bytes[remaining] } }",
        },
        TestCase {
            name: "varint",
            prefix: "test7",
            source: "type VarInt = { prefix: bits[2], value: match prefix { 0b00 => bits[6], 0b01 => bits[14], 0b10 => bits[30], 0b11 => bits[62] } }\npacket P { x: VarInt }",
        },
        TestCase {
            name: "enum_packet",
            prefix: "test8",
            source: "enum E: u8 { A = 0, B = 1 }\npacket P { code: E }",
        },
        TestCase {
            name: "derived_field",
            prefix: "test9",
            source: "packet P { flags: u8, let is_set: bool = (flags & 1) != 0 }",
        },
        TestCase {
            name: "capsule",
            prefix: "test10",
            source: r#"
                capsule C {
                    type_field: u8,
                    length: u16,
                    payload: match type_field within length {
                        0 => D { data: bytes[remaining] },
                        _ => U { data: bytes[remaining] },
                    },
                }
            "#,
        },
        TestCase {
            name: "state_machine",
            prefix: "test11",
            source: r#"
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
            "#,
        },
        TestCase {
            name: "cont_varint",
            prefix: "test12",
            source: r#"type MqttLen = varint { continuation_bit: msb, value_bits: 7, max_bytes: 4, byte_order: little }"#,
        },
        TestCase {
            name: "checksum",
            prefix: "test13",
            source: "packet P { data: u32, @checksum(internet) checksum: u16 }",
        },
        TestCase {
            name: "signed_types",
            prefix: "test14",
            source: "packet P { x: i8, y: i16, z: i32 }",
        },
        TestCase {
            name: "bytes_remaining",
            prefix: "test15",
            source: "packet P { data: bytes[remaining] }",
        },
        TestCase {
            name: "complex_frame",
            prefix: "test16",
            source: r#"
                frame AttPdu = match opcode: u8 {
                    0x01 => ErrorRsp { code: u8 },
                    0x0b => ReadRsp { value: bytes[remaining] },
                    _ => Unknown { data: bytes[remaining] },
                }
            "#,
        },
        TestCase {
            name: "const_and_packet",
            prefix: "test17",
            source: "const MAX_LEN: u8 = 20\npacket P { x: u8 }",
        },
        TestCase {
            name: "bytes_fixed",
            prefix: "test18",
            source: "packet P { mac: bytes[6], data: bytes[remaining] }",
        },
        TestCase {
            name: "pattern_range_frame",
            prefix: "test19",
            source: r#"
                frame F = match tag: u8 {
                    0x02..=0x03 => Ranged { x: u8 },
                    _ => Other { data: bytes[remaining] },
                }
            "#,
        },
        TestCase {
            name: "state_machine_with_guard",
            prefix: "test20",
            source: r#"
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
            "#,
        },
    ]
}

#[test]
fn gcc_verify_all() {
    let cases = test_cases();
    let mut pass_count = 0;
    let mut fail_count = 0;
    let mut failures: Vec<(String, String)> = Vec::new();

    for tc in &cases {
        eprintln!(
            "=== Generating C for: {} (prefix={}) ===",
            tc.name, tc.prefix
        );
        let (header, source) = generate_c(tc.source, tc.prefix);

        eprintln!("--- Header ({}.h) ---", tc.prefix);
        eprintln!("{}", header);
        eprintln!("--- Source ({}.c) ---", tc.prefix);
        eprintln!("{}", source);

        let (ok, stderr) = write_and_gcc(&header, &source, tc.prefix);
        if ok {
            eprintln!(">>> PASS: {} compiled successfully\n", tc.name);
            pass_count += 1;
        } else {
            eprintln!(">>> FAIL: {} - gcc errors:\n{}\n", tc.name, stderr);
            fail_count += 1;
            failures.push((tc.name.to_string(), stderr));
        }
    }

    eprintln!("\n========================================");
    eprintln!(
        "SUMMARY: {} passed, {} failed out of {} total",
        pass_count,
        fail_count,
        cases.len()
    );
    eprintln!("========================================\n");

    if !failures.is_empty() {
        eprintln!("FAILURES:");
        for (name, err) in &failures {
            eprintln!("\n--- {} ---", name);
            eprintln!("{}", err);
        }
    }

    // Don't assert — we want to see all results even if some fail
    // But do print a clear message
    if fail_count > 0 {
        panic!(
            "{} out of {} test cases failed gcc compilation. See above for details.",
            fail_count,
            cases.len()
        );
    }
}
