//! Tests that parse the actual example .wspec files from the corpus.
//! These verify the parser handles real-world protocol definitions.

use wirespec_syntax::parse;

fn parse_file(path: &str) -> wirespec_syntax::ast::AstModule {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
    parse(&source).unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"))
}

#[test]
fn parse_quic_varint() {
    let m = parse_file("../../examples/quic/varint.wire");
    assert!(m.module_decl.is_some());
    assert!(!m.items.is_empty());
}

#[test]
fn parse_quic_frames() {
    let m = parse_file("../../examples/quic/frames.wire");
    assert!(m.module_decl.is_some());
    // Should have VarInt type, const, packets, and QuicFrame
    assert!(m.items.len() >= 4);
}

#[test]
fn parse_udp() {
    let m = parse_file("../../examples/net/udp.wire");
    assert!(m.module_decl.is_some());
}

#[test]
fn parse_tcp() {
    let m = parse_file("../../examples/net/tcp.wire");
    assert!(m.module_decl.is_some());
}

#[test]
fn parse_mqtt() {
    let m = parse_file("../../examples/mqtt/mqtt.wire");
    assert!(m.module_decl.is_some());
    // Should have MqttLength varint, MqttString, MqttBytes, MqttPacket capsule
    assert!(m.items.len() >= 4);
}

#[test]
fn parse_ble_att() {
    let m = parse_file("../../examples/ble/att.wire");
    assert!(m.module_decl.is_some());
}

#[test]
fn parse_ethernet() {
    let m = parse_file("../../examples/net/ethernet.wire");
    assert!(m.module_decl.is_some());
}

#[test]
fn parse_bits_groups() {
    let m = parse_file("../../examples/test/bits_groups.wire");
    assert!(m.module_decl.is_some());
    assert_eq!(m.items.len(), 2);
}

#[test]
fn parse_mpquic_path() {
    let m = parse_file("../../examples/mpquic/path.wire");
    assert!(m.module_decl.is_some());
    // Has PathState state machine
    assert_eq!(m.items.len(), 1);
}
