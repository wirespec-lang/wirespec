// crates/wirespec-layout/tests/corpus_layout_tests.rs
//
// Corpus integration tests: real .wspec files through the full pipeline
// (parse -> sema -> layout).

use wirespec_layout::lower;
use wirespec_sema::{ComplianceProfile, analyze};
use wirespec_syntax::parse;

fn layout_file(path: &str) -> wirespec_layout::ir::LayoutModule {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
    let ast = parse(&source).unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"));
    let sem = analyze(
        &ast,
        ComplianceProfile::Phase2ExtendedCurrent,
        &Default::default(),
    )
    .unwrap_or_else(|e| panic!("Failed to analyze {path}: {e}"));
    lower(&sem).unwrap_or_else(|e| panic!("Failed to lower {path}: {e}"))
}

#[test]
fn corpus_quic_varint() {
    let m = layout_file("../../examples/quic/varint.wspec");
    assert!(!m.varints.is_empty());
}

#[test]
fn corpus_udp() {
    let m = layout_file("../../examples/net/udp.wspec");
    assert_eq!(m.packets.len(), 1);
}

#[test]
fn corpus_tcp() {
    let m = layout_file("../../examples/net/tcp.wspec");
    assert_eq!(m.packets.len(), 1);
    // TCP has bit fields -> should have bitgroups
    assert!(!m.packets[0].bitgroups.is_empty());
}

#[test]
fn corpus_ethernet() {
    let m = layout_file("../../examples/net/ethernet.wspec");
    assert_eq!(m.packets.len(), 1);
}

#[test]
fn corpus_bits_groups() {
    let m = layout_file("../../examples/test/bits_groups.wspec");
    assert_eq!(m.packets.len(), 2);
    // BitTest: bits[4]+bits[4] | u16 | bits[6]+bits[2]+bits[8] | u8 -> 2 groups
    assert_eq!(m.packets[0].bitgroups.len(), 2);
    // BitTest32: bits[4]+bits[12]+bits[16] = 32 bits -> 1 group
    assert_eq!(m.packets[1].bitgroups.len(), 1);
    assert_eq!(m.packets[1].bitgroups[0].total_bits, 32);
}

#[test]
fn corpus_ble_att() {
    let m = layout_file("../../examples/ble/att.wspec");
    assert!(!m.frames.is_empty());
}

#[test]
fn corpus_mqtt() {
    let m = layout_file("../../examples/mqtt/mqtt.wspec");
    assert!(!m.capsules.is_empty());
}
