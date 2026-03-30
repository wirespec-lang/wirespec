use std::path::PathBuf;
use wirespec_driver::{CompileRequest, compile};
use wirespec_sema::ComplianceProfile;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn compile_example(rel_path: &str) -> wirespec_driver::CompileResult {
    let entry = examples_dir().join(rel_path);
    compile(&CompileRequest {
        entry: entry.clone(),
        include_paths: vec![examples_dir()],
        profile: ComplianceProfile::Phase2ExtendedCurrent,
        asn1_modules: Default::default(),
    })
    .unwrap_or_else(|e| panic!("Failed to compile {rel_path}: {e}"))
}

#[test]
fn corpus_quic_varint() {
    let result = compile_example("quic/varint.wspec");
    assert_eq!(result.modules.len(), 1);
    assert!(!result.modules[0].codec.varints.is_empty());
}

#[test]
fn corpus_quic_frames() {
    // frames.wire defines VarInt locally (no import), so this is single-module
    let result = compile_example("quic/frames.wspec");
    assert_eq!(result.modules.len(), 1);
    assert!(!result.modules[0].codec.frames.is_empty());
}

#[test]
fn corpus_net_udp() {
    let result = compile_example("net/udp.wspec");
    assert_eq!(result.modules.len(), 1);
    assert!(!result.modules[0].codec.packets.is_empty());
}

#[test]
fn corpus_net_tcp() {
    let result = compile_example("net/tcp.wspec");
    assert_eq!(result.modules.len(), 1);
    assert!(!result.modules[0].codec.packets.is_empty());
}

#[test]
fn corpus_net_ethernet() {
    let result = compile_example("net/ethernet.wspec");
    assert_eq!(result.modules.len(), 1);
    assert!(!result.modules[0].codec.packets.is_empty());
}

#[test]
fn corpus_ble_att() {
    let result = compile_example("ble/att.wspec");
    assert_eq!(result.modules.len(), 1);
    assert!(!result.modules[0].codec.frames.is_empty());
}

#[test]
fn corpus_mqtt() {
    let result = compile_example("mqtt/mqtt.wspec");
    assert_eq!(result.modules.len(), 1);
    assert!(!result.modules[0].codec.capsules.is_empty());
}

#[test]
fn corpus_bits_groups() {
    let result = compile_example("test/bits_groups.wspec");
    assert_eq!(result.modules.len(), 1);
    assert!(!result.modules[0].codec.packets.is_empty());
}

#[test]
fn corpus_mpquic_path_with_imports() {
    // This file imports quic.varint.VarInt -- multi-module compilation
    // requires sema to accept external types, which is not yet implemented.
    let result = compile_example("mpquic/path.wspec");
    assert!(result.modules.len() >= 2); // varint + path
}
