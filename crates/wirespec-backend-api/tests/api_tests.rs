// crates/wirespec-backend-api/tests/api_tests.rs
use wirespec_backend_api::*;

#[test]
fn target_id_display() {
    assert_eq!(TargetId("c").to_string(), "c");
    assert_eq!(TargetId("rust").to_string(), "rust");
}

#[test]
fn target_id_equality() {
    assert_eq!(TargetId("c"), TargetId("c"));
    assert_ne!(TargetId("c"), TargetId("rust"));
}

#[test]
fn registry_unknown_target() {
    let reg = BackendRegistry::new();
    let result = reg.get(TargetId("zig"));
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(matches!(err, BackendError::UnsupportedTarget(_)));
}

#[test]
fn registry_empty_targets() {
    let reg = BackendRegistry::new();
    assert!(reg.available_targets().is_empty());
}

#[test]
fn memory_sink_collects_artifacts() {
    let mut sink = MemorySink::new();
    let artifact = Artifact {
        target: TargetId("c"),
        kind: ArtifactKind::C_HEADER,
        module_name: "test".to_string(),
        module_prefix: "test".to_string(),
        relative_path: "test.h".into(),
        contents: b"// header\n".to_vec(),
    };
    sink.write(artifact).unwrap();
    assert_eq!(sink.artifacts.len(), 1);
    assert_eq!(sink.artifacts[0].0.kind, ArtifactKind::C_HEADER);
    assert_eq!(sink.artifacts[0].0.byte_len, 10);
}

#[test]
fn no_checksum_bindings_returns_error() {
    let provider = NoChecksumBindings;
    let result = provider.binding_for("internet");
    assert!(result.is_err());
}

#[test]
fn backend_error_display() {
    let err = BackendError::UnsupportedTarget(TargetId("zig"));
    assert_eq!(err.to_string(), "unsupported target: zig");

    let err = BackendError::MissingChecksumBinding {
        target: TargetId("c"),
        algorithm: "sha256".to_string(),
    };
    assert!(err.to_string().contains("sha256"));
}

#[test]
fn c_backend_options_default() {
    let opts = CBackendOptions::default();
    assert!(!opts.emit_fuzz_harness);
}

#[test]
fn common_options_default() {
    let opts = CommonOptions::default();
    assert!(opts.emit_comments);
}

#[test]
fn artifact_kind_variants() {
    // Verify all artifact kinds exist
    let kinds = [
        ArtifactKind::C_HEADER,
        ArtifactKind::C_SOURCE,
        ArtifactKind::C_FUZZ_SOURCE,
        ArtifactKind::RUST_SOURCE,
        ArtifactKind::OTHER,
    ];
    assert_eq!(kinds.len(), 5);
}
