// crates/wirespec-driver/tests/roundtrip_tests.rs
//
// End-to-end round-trip tests: generate C code from .wspec source via the Rust
// compiler pipeline, write a C test program that parses + serializes + verifies,
// compile with gcc, run the binary, and assert exit code 0.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use wirespec_backend_api::*;
use wirespec_sema::ComplianceProfile;
use wirespec_syntax::parse;

fn runtime_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protospec/runtime")
}

fn generate_c_files(wspec_src: &str, prefix: &str, outdir: &Path) {
    let ast = parse(wspec_src).unwrap();
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
    fs::write(outdir.join(format!("{prefix}.h")), &lowered.header_content).unwrap();
    fs::write(outdir.join(format!("{prefix}.c")), &lowered.source_content).unwrap();

    // Also dump for debugging on failure
    eprintln!("--- {prefix}.h ---\n{}", lowered.header_content);
    eprintln!("--- {prefix}.c ---\n{}", lowered.source_content);
}

fn compile_and_run(outdir: &Path, prefix: &str, test_c_code: &str) -> Result<(), String> {
    let test_file = outdir.join("test_main.c");
    fs::write(&test_file, test_c_code).unwrap();

    let runtime = runtime_dir();
    let binary = outdir.join("test_binary");

    // Compile
    let compile_output = Command::new("gcc")
        .args([
            "-Wall",
            "-Wextra",
            "-Werror",
            "-std=c11",
            "-O0",
            "-g",
            "-I",
            &outdir.to_string_lossy(),
            "-I",
            &runtime.to_string_lossy(),
            "-o",
            &binary.to_string_lossy(),
        ])
        .arg(outdir.join(format!("{prefix}.c")).to_string_lossy().as_ref())
        .arg(test_file.to_string_lossy().as_ref())
        .output()
        .map_err(|e| format!("gcc not found: {e}"))?;

    if !compile_output.status.success() {
        return Err(format!(
            "gcc compile failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&compile_output.stdout),
            String::from_utf8_lossy(&compile_output.stderr)
        ));
    }

    // Run
    let run_output = Command::new(&binary)
        .output()
        .map_err(|e| format!("run failed: {e}"))?;

    let stdout = String::from_utf8_lossy(&run_output.stdout);
    let stderr = String::from_utf8_lossy(&run_output.stderr);
    eprintln!("test stdout: {stdout}");
    if !stderr.is_empty() {
        eprintln!("test stderr: {stderr}");
    }

    if !run_output.status.success() {
        return Err(format!(
            "test failed (exit {}):\nstdout: {stdout}\nstderr: {stderr}",
            run_output.status.code().unwrap_or(-1)
        ));
    }

    Ok(())
}

// ============================================================
// Test 1: UDP round-trip
// ============================================================
#[test]
fn roundtrip_udp() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/udp");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = r#"
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
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    /* UDP datagram: src=1234, dst=80, length=11 (8 hdr + 3 data), checksum=0, data="abc" */
    uint8_t buf[] = {
        0x04, 0xD2,        /* src_port = 1234 */
        0x00, 0x50,        /* dst_port = 80   */
        0x00, 0x0B,        /* length   = 11   */
        0x00, 0x00,        /* checksum = 0    */
        0x61, 0x62, 0x63   /* data = "abc"    */
    };

    /* Parse */
    test_udp_datagram_t pkt;
    size_t consumed;
    wirespec_result_t r = test_udp_datagram_parse(buf, sizeof(buf), &pkt, &consumed);
    assert(r == WIRESPEC_OK);
    assert(consumed == 11);
    assert(pkt.src_port == 1234);
    assert(pkt.dst_port == 80);
    assert(pkt.length == 11);
    assert(pkt.checksum == 0);
    assert(pkt.data.len == 3);
    assert(memcmp(pkt.data.ptr, "abc", 3) == 0);

    /* Serialize */
    uint8_t out[64];
    size_t written;
    r = test_udp_datagram_serialize(&pkt, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 11);
    assert(memcmp(buf, out, 11) == 0);

    /* Serialized length */
    size_t slen = test_udp_datagram_serialized_len(&pkt);
    assert(slen == 11);

    printf("UDP round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 2: Fixed-width fields round-trip
// ============================================================
#[test]
fn roundtrip_fixed_fields() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/fixed");
    fs::create_dir_all(&outdir).unwrap();

    // Default endianness is big-endian
    let wspec = "packet P { a: u8, b: u16, c: u32 }";
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    uint8_t buf[] = {
        0x42,                       /* a = 0x42       */
        0x12, 0x34,                 /* b = 0x1234 BE  */
        0xDE, 0xAD, 0xBE, 0xEF     /* c = 0xDEADBEEF */
    };

    test_p_t p;
    size_t consumed;
    wirespec_result_t r = test_p_parse(buf, sizeof(buf), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(consumed == 7);
    assert(p.a == 0x42);
    assert(p.b == 0x1234);
    assert(p.c == 0xDEADBEEF);

    uint8_t out[64];
    size_t written;
    r = test_p_serialize(&p, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 7);
    assert(memcmp(buf, out, 7) == 0);

    printf("Fixed fields round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 3: Bitgroup round-trip
// ============================================================
#[test]
fn roundtrip_bitgroup() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/bits");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = "@endian big\npacket P { version: bits[4], ihl: bits[4], tos: u8 }";
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    /* version=4, ihl=5 -> byte = (4 << 4) | 5 = 0x45, tos=0x00 */
    uint8_t buf[] = { 0x45, 0x00 };

    test_p_t p;
    size_t consumed;
    wirespec_result_t r = test_p_parse(buf, sizeof(buf), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(consumed == 2);
    assert(p.version == 4);
    assert(p.ihl == 5);
    assert(p.tos == 0);

    uint8_t out[64];
    size_t written;
    r = test_p_serialize(&p, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 2);
    assert(memcmp(buf, out, 2) == 0);

    printf("Bitgroup round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 4: Optional field round-trip
// ============================================================
#[test]
fn roundtrip_optional() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/optional");
    fs::create_dir_all(&outdir).unwrap();

    let wspec =
        "packet P { flags: u8, extra: if flags & 0x01 { u16 }, data: bytes[remaining] }";
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    /* Case 1: flag set, extra present */
    uint8_t buf1[] = { 0x01, 0x00, 0xFF, 0xAA, 0xBB };
    test_p_t p1;
    size_t consumed;
    wirespec_result_t r = test_p_parse(buf1, sizeof(buf1), &p1, &consumed);
    assert(r == WIRESPEC_OK);
    assert(p1.flags == 0x01);
    assert(p1.has_extra == true);
    assert(p1.extra == 0x00FF);   /* big-endian */
    assert(p1.data.len == 2);     /* 0xAA, 0xBB */

    uint8_t out1[64];
    size_t written;
    r = test_p_serialize(&p1, out1, sizeof(out1), &written);
    assert(r == WIRESPEC_OK);
    assert(written == sizeof(buf1));
    assert(memcmp(buf1, out1, sizeof(buf1)) == 0);

    /* Case 2: flag not set, extra absent */
    uint8_t buf2[] = { 0x00, 0xCC, 0xDD };
    test_p_t p2;
    r = test_p_parse(buf2, sizeof(buf2), &p2, &consumed);
    assert(r == WIRESPEC_OK);
    assert(p2.flags == 0x00);
    assert(p2.has_extra == false);
    assert(p2.data.len == 2);     /* 0xCC, 0xDD */

    uint8_t out2[64];
    r = test_p_serialize(&p2, out2, sizeof(out2), &written);
    assert(r == WIRESPEC_OK);
    assert(written == sizeof(buf2));
    assert(memcmp(buf2, out2, sizeof(buf2)) == 0);

    printf("Optional field round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 5: Array field round-trip
// ============================================================
#[test]
fn roundtrip_array() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/array");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = "packet P { count: u8, items: [u8; count] }";
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    uint8_t buf[] = { 0x03, 0x0A, 0x0B, 0x0C };

    test_p_t p;
    size_t consumed;
    wirespec_result_t r = test_p_parse(buf, sizeof(buf), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(consumed == 4);
    assert(p.count == 3);
    assert(p.items_count == 3);
    assert(p.items[0] == 0x0A);
    assert(p.items[1] == 0x0B);
    assert(p.items[2] == 0x0C);

    uint8_t out[64];
    size_t written;
    r = test_p_serialize(&p, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 4);
    assert(memcmp(buf, out, 4) == 0);

    printf("Array round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 6: Frame with tag dispatch round-trip
// ============================================================
#[test]
fn roundtrip_frame() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/frame");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = r#"
        frame F = match tag: u8 {
            0x01 => Ping {},
            0x02 => Data { length: u8, payload: bytes[length: length] },
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    /* Test Ping (tag=0x01, no body) */
    uint8_t buf_ping[] = { 0x01 };
    test_f_t f;
    size_t consumed;
    wirespec_result_t r = test_f_parse(buf_ping, sizeof(buf_ping), &f, &consumed);
    assert(r == WIRESPEC_OK);
    assert(f.tag == TEST_F_PING);
    assert(consumed == 1);

    uint8_t out[64];
    size_t written;
    r = test_f_serialize(&f, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 1);
    assert(out[0] == 0x01);

    /* Test Data (tag=0x02, length=3, payload="xyz") */
    uint8_t buf_data[] = { 0x02, 0x03, 0x78, 0x79, 0x7A };
    r = test_f_parse(buf_data, sizeof(buf_data), &f, &consumed);
    assert(r == WIRESPEC_OK);
    assert(f.tag == TEST_F_DATA);
    assert(f.value.data.length == 3);
    assert(f.value.data.payload.len == 3);
    assert(memcmp(f.value.data.payload.ptr, "xyz", 3) == 0);

    r = test_f_serialize(&f, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 5);
    assert(memcmp(buf_data, out, 5) == 0);

    /* Test Unknown (tag=0xFF, catch-all) */
    uint8_t buf_unk[] = { 0xFF, 0x11, 0x22 };
    r = test_f_parse(buf_unk, sizeof(buf_unk), &f, &consumed);
    assert(r == WIRESPEC_OK);
    assert(f.tag == TEST_F_UNKNOWN);
    assert(f.value.unknown.data.len == 2);

    r = test_f_serialize(&f, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 3);
    assert(memcmp(buf_unk, out, 3) == 0);

    printf("Frame round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 7: Require constraint (valid, invalid, short buffer)
// ============================================================
#[test]
fn roundtrip_require_violation() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/require");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = "packet P { length: u16, require length >= 8 }";
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <assert.h>

int main(void) {
    /* Valid: length = 8 */
    uint8_t buf_ok[] = { 0x00, 0x08 };
    test_p_t p;
    size_t consumed;
    wirespec_result_t r = test_p_parse(buf_ok, sizeof(buf_ok), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(p.length == 8);

    /* Invalid: length = 7 -> CONSTRAINT violation */
    uint8_t buf_bad[] = { 0x00, 0x07 };
    r = test_p_parse(buf_bad, sizeof(buf_bad), &p, &consumed);
    assert(r == WIRESPEC_ERR_CONSTRAINT);

    /* Short buffer: only 1 byte for u16 */
    uint8_t buf_short[] = { 0x00 };
    r = test_p_parse(buf_short, sizeof(buf_short), &p, &consumed);
    assert(r == WIRESPEC_ERR_SHORT_BUFFER);

    printf("Require + error handling: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 8: Little-endian round-trip
// ============================================================
#[test]
fn roundtrip_little_endian() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/le");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = "@endian little\npacket P { x: u16, y: u32 }";
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    /* Little-endian: x=0x1234 stored as 34 12, y=0xDEADBEEF stored as EF BE AD DE */
    uint8_t buf[] = { 0x34, 0x12, 0xEF, 0xBE, 0xAD, 0xDE };

    test_p_t p;
    size_t consumed;
    wirespec_result_t r = test_p_parse(buf, sizeof(buf), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(consumed == 6);
    assert(p.x == 0x1234);
    assert(p.y == 0xDEADBEEF);

    uint8_t out[64];
    size_t written;
    r = test_p_serialize(&p, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 6);
    assert(memcmp(buf, out, 6) == 0);

    printf("Little-endian round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 9: VarInt prefix-match round-trip (QUIC-style)
// ============================================================
#[test]
fn roundtrip_varint() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/varint");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = r#"
        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6],
                0b01 => bits[14],
                0b10 => bits[30],
                0b11 => bits[62],
            },
        }
        packet P { x: VarInt }
    "#;
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    /* 1-byte VarInt: value=37 -> byte = 0x25 (00_100101) */
    {
        uint8_t buf[] = { 0x25 };
        test_p_t p;
        size_t consumed;
        wirespec_result_t r = test_p_parse(buf, sizeof(buf), &p, &consumed);
        assert(r == WIRESPEC_OK);
        assert(p.x == 37);
        assert(consumed == 1);

        uint8_t out[16];
        size_t written;
        r = test_p_serialize(&p, out, sizeof(out), &written);
        assert(r == WIRESPEC_OK);
        assert(written == 1);
        assert(out[0] == 0x25);
    }

    /* 2-byte VarInt: value=500 -> 0x41, 0xF4 (01_000001 11110100) */
    {
        uint8_t buf[] = { 0x41, 0xF4 };
        test_p_t p;
        size_t consumed;
        wirespec_result_t r = test_p_parse(buf, sizeof(buf), &p, &consumed);
        assert(r == WIRESPEC_OK);
        assert(p.x == 500);
        assert(consumed == 2);

        uint8_t out[16];
        size_t written;
        r = test_p_serialize(&p, out, sizeof(out), &written);
        assert(r == WIRESPEC_OK);
        assert(written == 2);
        assert(memcmp(buf, out, 2) == 0);
    }

    /* 1-byte boundary: value=63 (max for 1-byte) */
    {
        uint8_t buf[] = { 0x3F };
        test_p_t p;
        size_t consumed;
        wirespec_result_t r = test_p_parse(buf, sizeof(buf), &p, &consumed);
        assert(r == WIRESPEC_OK);
        assert(p.x == 63);
    }

    /* 1-byte: value=0 */
    {
        uint8_t buf[] = { 0x00 };
        test_p_t p;
        size_t consumed;
        wirespec_result_t r = test_p_parse(buf, sizeof(buf), &p, &consumed);
        assert(r == WIRESPEC_OK);
        assert(p.x == 0);
    }

    printf("VarInt round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 10: Capsule with within (sub-cursor)
// ============================================================
#[test]
fn roundtrip_capsule() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/capsule");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = r#"
        capsule TlvPacket {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                1 => Data { content: bytes[remaining] },
                _ => Unknown { raw: bytes[remaining] },
            },
        }
    "#;
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    /* Valid TLV: type=1, length=3, payload="abc" */
    uint8_t buf[] = { 0x01, 0x00, 0x03, 0x61, 0x62, 0x63 };
    test_tlv_packet_t pkt;
    size_t consumed;
    wirespec_result_t r = test_tlv_packet_parse(buf, sizeof(buf), &pkt, &consumed);
    assert(r == WIRESPEC_OK);
    assert(pkt.type_field == 1);
    assert(pkt.length == 3);
    assert(pkt.tag == TEST_TLV_PACKET_DATA);
    assert(pkt.value.data.content.len == 3);
    assert(memcmp(pkt.value.data.content.ptr, "abc", 3) == 0);

    uint8_t out[64];
    size_t written;
    r = test_tlv_packet_serialize(&pkt, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 6);
    assert(memcmp(buf, out, 6) == 0);

    /* Unknown type: type=99, length=2, payload=0xFF 0xFE */
    uint8_t buf2[] = { 0x63, 0x00, 0x02, 0xFF, 0xFE };
    r = test_tlv_packet_parse(buf2, sizeof(buf2), &pkt, &consumed);
    assert(r == WIRESPEC_OK);
    assert(pkt.tag == TEST_TLV_PACKET_UNKNOWN);
    assert(pkt.value.unknown.raw.len == 2);

    printf("Capsule round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 11: Nested struct (packet referencing packet)
// ============================================================
#[test]
fn roundtrip_nested_struct() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/nested");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = r#"
        packet Inner { a: u8, b: u8 }
        packet Outer { header: u16, inner: Inner, trailer: u8 }
    "#;
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    uint8_t buf[] = { 0x12, 0x34, 0xAA, 0xBB, 0xFF };
    test_outer_t p;
    size_t consumed;
    wirespec_result_t r = test_outer_parse(buf, sizeof(buf), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(p.header == 0x1234);
    assert(p.inner.a == 0xAA);
    assert(p.inner.b == 0xBB);
    assert(p.trailer == 0xFF);

    uint8_t out[64];
    size_t written;
    r = test_outer_serialize(&p, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 5);
    assert(memcmp(buf, out, 5) == 0);

    printf("Nested struct round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 12: Continuation-bit VarInt (MQTT style)
// ============================================================
#[test]
fn roundtrip_cont_varint() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/cont_varint");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = r#"
        type MqttLen = varint {
            continuation_bit: msb,
            value_bits: 7,
            max_bytes: 4,
            byte_order: little,
        }
        packet P { length: MqttLen }
    "#;
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <assert.h>

int main(void) {
    /* value=0 */
    {
        uint8_t buf[] = { 0x00 };
        test_p_t p;
        size_t c;
        assert(test_p_parse(buf, 1, &p, &c) == WIRESPEC_OK);
        assert(p.length == 0);
        uint8_t out[8];
        size_t w;
        assert(test_p_serialize(&p, out, 8, &w) == WIRESPEC_OK);
        assert(w == 1 && out[0] == 0x00);
    }

    /* value=127 */
    {
        uint8_t buf[] = { 0x7F };
        test_p_t p;
        size_t c;
        assert(test_p_parse(buf, 1, &p, &c) == WIRESPEC_OK);
        assert(p.length == 127);
    }

    /* value=128 -> 0x80, 0x01 */
    {
        uint8_t buf[] = { 0x80, 0x01 };
        test_p_t p;
        size_t c;
        assert(test_p_parse(buf, 2, &p, &c) == WIRESPEC_OK);
        assert(p.length == 128);
        uint8_t out[8];
        size_t w;
        assert(test_p_serialize(&p, out, 8, &w) == WIRESPEC_OK);
        assert(w == 2);
        assert(out[0] == 0x80 && out[1] == 0x01);
    }

    /* value=16383 -> 0xFF, 0x7F */
    {
        uint8_t buf[] = { 0xFF, 0x7F };
        test_p_t p;
        size_t c;
        assert(test_p_parse(buf, 2, &p, &c) == WIRESPEC_OK);
        assert(p.length == 16383);
    }

    printf("Continuation VarInt round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 13: Error cases (invalid tag, capacity, short buffer)
// ============================================================
#[test]
fn roundtrip_error_cases() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/errors");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = r#"
        frame F = match tag: u8 {
            0x01 => Ping {},
            _ => Unknown { data: bytes[remaining] },
        }
    "#;
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <assert.h>

int main(void) {
    /* Empty buffer -> SHORT_BUFFER */
    test_f_t f;
    size_t consumed;
    wirespec_result_t r = test_f_parse(NULL, 0, &f, &consumed);
    assert(r == WIRESPEC_ERR_SHORT_BUFFER);

    printf("Error cases: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 14: Zero-length edge cases
// ============================================================
#[test]
fn roundtrip_zero_length() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/zerolen");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = "packet P { count: u8, items: [u8; count], data: bytes[remaining] }";
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    /* count=0, no items, no remaining data */
    uint8_t buf[] = { 0x00 };
    test_p_t p;
    size_t consumed;
    wirespec_result_t r = test_p_parse(buf, sizeof(buf), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(p.count == 0);
    assert(p.items_count == 0);
    assert(p.data.len == 0);

    uint8_t out[64];
    size_t written;
    r = test_p_serialize(&p, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(written == 1);
    assert(out[0] == 0x00);

    printf("Zero-length: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 15: Derived field value verification
// ============================================================
#[test]
fn roundtrip_derived_field() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/derived");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = r#"
        packet P {
            flags: u8,
            let is_urgent: bool = (flags & 0x80) != 0,
            let masked: u8 = flags & 0x0F,
        }
    "#;
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <assert.h>

int main(void) {
    uint8_t buf[] = { 0x8A };  /* flags = 0x8A -> is_urgent=true, masked=0x0A */
    test_p_t p;
    size_t consumed;
    wirespec_result_t r = test_p_parse(buf, sizeof(buf), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(p.flags == 0x8A);
    assert(p.is_urgent == true);
    assert(p.masked == 0x0A);

    /* Non-urgent: flags = 0x03 */
    uint8_t buf2[] = { 0x03 };
    r = test_p_parse(buf2, sizeof(buf2), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(p.is_urgent == false);
    assert(p.masked == 0x03);

    printf("Derived field: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}

// ============================================================
// Test 16: bytes[length_or_remaining:]
// ============================================================
#[test]
fn roundtrip_bytes_lor() {
    let outdir = PathBuf::from("/tmp/wirespec-roundtrip/bytes_lor");
    fs::create_dir_all(&outdir).unwrap();

    let wspec = r#"
        packet P {
            flags: u8,
            length: if flags & 0x01 { u16 },
            data: bytes[length_or_remaining: length],
        }
    "#;
    generate_c_files(wspec, "test", &outdir);

    let test_c = r#"
#include "test.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

int main(void) {
    /* Case 1: length present (flags & 1), length=3, data="abc" */
    uint8_t buf1[] = { 0x01, 0x00, 0x03, 0x61, 0x62, 0x63 };
    test_p_t p;
    size_t consumed;
    wirespec_result_t r = test_p_parse(buf1, sizeof(buf1), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(p.has_length == true);
    assert(p.length == 3);
    assert(p.data.len == 3);
    assert(memcmp(p.data.ptr, "abc", 3) == 0);

    uint8_t out[64];
    size_t written;
    r = test_p_serialize(&p, out, sizeof(out), &written);
    assert(r == WIRESPEC_OK);
    assert(memcmp(buf1, out, sizeof(buf1)) == 0);

    /* Case 2: length absent (flags & 1 == 0), remaining="xyz" */
    uint8_t buf2[] = { 0x00, 0x78, 0x79, 0x7A };
    r = test_p_parse(buf2, sizeof(buf2), &p, &consumed);
    assert(r == WIRESPEC_OK);
    assert(p.has_length == false);
    assert(p.data.len == 3);
    assert(memcmp(p.data.ptr, "xyz", 3) == 0);

    printf("bytes[length_or_remaining] round-trip: OK\n");
    return 0;
}
"#;
    compile_and_run(&outdir, "test", test_c).unwrap();
}
