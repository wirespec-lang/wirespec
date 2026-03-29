// crates/wirespec-codec/tests/corpus_codec_tests.rs
//
// Corpus integration tests: real .wire files through the full pipeline
// (parse -> sema -> layout -> codec).

use wirespec_codec::ir;
use wirespec_codec::lower;
use wirespec_sema::{ComplianceProfile, analyze};
use wirespec_syntax::parse;

fn codec_file(path: &str) -> ir::CodecModule {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
    let ast = parse(&source).unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"));
    let sem = analyze(
        &ast,
        ComplianceProfile::Phase2ExtendedCurrent,
        &Default::default(),
    )
    .unwrap_or_else(|e| panic!("Failed to analyze {path}: {e}"));
    let layout =
        wirespec_layout::lower(&sem).unwrap_or_else(|e| panic!("Failed to layout {path}: {e}"));
    lower(&layout).unwrap_or_else(|e| panic!("Failed to codec {path}: {e}"))
}

#[test]
fn corpus_quic_varint() {
    let c = codec_file("../../protospec/examples/quic/varint.wire");
    assert!(!c.varints.is_empty());
}

#[test]
fn corpus_udp() {
    let c = codec_file("../../protospec/examples/net/udp.wire");
    assert_eq!(c.packets.len(), 1);
    // UDP has require -> items should include it
    assert!(c.packets[0].items.len() >= 5);
}

#[test]
fn corpus_tcp() {
    let c = codec_file("../../protospec/examples/net/tcp.wire");
    assert_eq!(c.packets.len(), 1);
    // TCP has bitgroup fields
    assert!(
        c.packets[0]
            .fields
            .iter()
            .any(|f| f.strategy == ir::FieldStrategy::BitGroup)
    );
}

#[test]
fn corpus_ethernet() {
    let c = codec_file("../../protospec/examples/net/ethernet.wire");
    assert_eq!(c.packets.len(), 1);
}

#[test]
fn corpus_bits_groups() {
    let c = codec_file("../../protospec/examples/test/bits_groups.wire");
    assert_eq!(c.packets.len(), 2);
}

#[test]
fn corpus_ble_att() {
    let c = codec_file("../../protospec/examples/ble/att.wire");
    assert!(!c.frames.is_empty());
}

#[test]
fn corpus_mqtt() {
    let c = codec_file("../../protospec/examples/mqtt/mqtt.wire");
    assert!(!c.capsules.is_empty());
}
