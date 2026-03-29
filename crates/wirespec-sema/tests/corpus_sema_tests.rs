//! Integration tests: parse + analyze real .wire example files.
//!
//! Files that use `import` are marked `#[ignore]` because the sema crate
//! does not yet include a module resolver.

use wirespec_sema::ComplianceProfile;
use wirespec_sema::analyze;
use wirespec_syntax::parse;

fn analyze_file(path: &str) -> Result<wirespec_sema::SemanticModule, String> {
    let source =
        std::fs::read_to_string(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let ast = parse(&source).map_err(|e| format!("parse error in {path}: {e}"))?;
    analyze(
        &ast,
        ComplianceProfile::Phase2ExtendedCurrent,
        &Default::default(),
    )
    .map_err(|e| format!("sema error in {path}: {e}"))
}

// ── Files without imports (should work) ──

#[test]
fn corpus_quic_varint() {
    let sem = analyze_file("../../examples/quic/varint.wire")
        .expect("quic/varint.wire should analyze successfully");
    assert_eq!(sem.module_name, "quic.varint");
    assert!(
        !sem.varints.is_empty(),
        "should contain at least one varint"
    );
}

#[test]
fn corpus_net_udp() {
    let sem = analyze_file("../../examples/net/udp.wire")
        .expect("net/udp.wire should analyze successfully");
    assert_eq!(sem.module_name, "net.udp");
    assert_eq!(sem.packets.len(), 1);
    assert_eq!(sem.packets[0].name, "UdpDatagram");
}

#[test]
fn corpus_net_tcp() {
    let sem = analyze_file("../../examples/net/tcp.wire")
        .expect("net/tcp.wire should analyze successfully");
    assert_eq!(sem.module_name, "net.tcp");
    assert_eq!(sem.packets.len(), 1);
    assert_eq!(sem.packets[0].name, "TcpSegment");
}

#[test]
fn corpus_net_ethernet() {
    let sem = analyze_file("../../examples/net/ethernet.wire")
        .expect("net/ethernet.wire should analyze successfully");
    assert_eq!(sem.module_name, "net.ethernet");
    assert_eq!(sem.packets.len(), 1);
    assert_eq!(sem.packets[0].name, "EthernetFrame");
}

#[test]
fn corpus_test_bits_groups() {
    let sem = analyze_file("../../examples/test/bits_groups.wire")
        .expect("test/bits_groups.wire should analyze successfully");
    assert_eq!(sem.module_name, "test.bits_groups");
    assert_eq!(sem.packets.len(), 2, "should have BitTest and BitTest32");
}

#[test]
fn corpus_quic_frames() {
    let sem = analyze_file("../../examples/quic/frames.wire")
        .expect("quic/frames.wire should analyze successfully");
    assert_eq!(sem.module_name, "quic.frames");
    assert!(!sem.frames.is_empty(), "should contain at least one frame");
}

#[test]
fn corpus_ble_att() {
    let sem = analyze_file("../../examples/ble/att.wire")
        .expect("ble/att.wire should analyze successfully");
    assert_eq!(sem.module_name, "ble.att");
    assert!(!sem.frames.is_empty(), "should contain AttPdu frame");
}

#[test]
fn corpus_mqtt() {
    let sem = analyze_file("../../examples/mqtt/mqtt.wire")
        .expect("mqtt/mqtt.wire should analyze successfully");
    assert_eq!(sem.module_name, "mqtt");
    assert!(
        !sem.capsules.is_empty(),
        "should contain MqttPacket capsule"
    );
}

// ── Files that use `import` (no resolver yet) ──

#[test]
fn corpus_mpquic_path() {
    let sem = analyze_file("../../examples/mpquic/path.wire")
        .expect("mpquic/path.wire should analyze successfully");
    assert!(!sem.state_machines.is_empty());
}
