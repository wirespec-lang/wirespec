# wirespec-backend-api Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Define the backend API contracts (traits, types, error taxonomy) that concrete backends (C, Rust) implement and the driver consumes — enabling new backends without changing shared IR stages.

**Architecture:** Pure trait/type definitions. `Backend` trait for typed backend-local implementation, `BackendDyn` trait for object-safe registry use, `BackendContext` for driver-to-backend context, `ArtifactSink` for testable output, `BackendRegistry` for target lookup, `ChecksumBindingProvider` for checksum runtime integration. No implementation logic — just contracts.

**Tech Stack:** Rust (edition 2024), `wirespec-codec` crate

**Normative spec:** `docs/ref/BACKEND_API_SPEC.md`

---

## File Structure

```
crates/wirespec-backend-api/
├── Cargo.toml
├── src/
│   └── lib.rs                      # All API types in one file (pure contracts, ~250 lines)
└── tests/
    └── api_tests.rs                # Registry, context, error type tests
```

Single file because this is pure API surface — no implementation logic to split.

---

## Chunk 1: API Definitions and Tests

### Task 1: All backend API types

**Files:**
- Modify: `crates/wirespec-backend-api/src/lib.rs`
- Test: `crates/wirespec-backend-api/tests/api_tests.rs`

- [ ] **Step 1: Write the API types**

All types from BACKEND_API_SPEC §6–§16:

```rust
// crates/wirespec-backend-api/src/lib.rs
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

pub use wirespec_codec::CodecModule;

// ── Target IDs (spec §7) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct TargetId(pub &'static str);

pub const TARGET_C: TargetId = TargetId("c");
pub const TARGET_RUST: TargetId = TargetId("rust");

impl std::fmt::Display for TargetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Artifact (spec §7-§8) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    CHeader,
    CSource,
    CFuzzSource,
    RustSource,
    Other,
}

pub struct Artifact {
    pub target: TargetId,
    pub kind: ArtifactKind,
    pub module_name: String,
    pub module_prefix: String,
    pub relative_path: PathBuf,
    pub contents: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ArtifactMeta {
    pub kind: ArtifactKind,
    pub relative_path: PathBuf,
    pub byte_len: usize,
}

#[derive(Debug)]
pub struct BackendOutput {
    pub target: TargetId,
    pub artifacts: Vec<ArtifactMeta>,
}

// ── Artifact Sink (spec §8) ──

pub trait ArtifactSink {
    fn write(&mut self, artifact: Artifact) -> Result<(), BackendError>;
}

/// In-memory artifact sink for testing.
pub struct MemorySink {
    pub artifacts: Vec<(ArtifactMeta, Vec<u8>)>,
}

impl MemorySink {
    pub fn new() -> Self {
        Self {
            artifacts: Vec::new(),
        }
    }
}

impl Default for MemorySink {
    fn default() -> Self {
        Self::new()
    }
}

impl ArtifactSink for MemorySink {
    fn write(&mut self, artifact: Artifact) -> Result<(), BackendError> {
        let meta = ArtifactMeta {
            kind: artifact.kind,
            relative_path: artifact.relative_path,
            byte_len: artifact.contents.len(),
        };
        self.artifacts.push((meta, artifact.contents));
        Ok(())
    }
}

// ── Backend Context (spec §10) ──

pub struct BackendContext {
    pub module_name: String,
    pub module_prefix: String,
    pub source_prefixes: BTreeMap<String, String>,
    pub compliance_profile: String,
    pub common_options: CommonOptions,
    pub target_options: TargetOptions,
    pub checksum_bindings: Arc<dyn ChecksumBindingProvider>,
    pub is_entry_module: bool,
}

// ── Options (spec §11) ──

#[derive(Debug, Clone)]
pub struct CommonOptions {
    pub emit_comments: bool,
}

impl Default for CommonOptions {
    fn default() -> Self {
        Self {
            emit_comments: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TargetOptions {
    C(CBackendOptions),
    Rust(RustBackendOptions),
}

#[derive(Debug, Clone)]
pub struct CBackendOptions {
    pub emit_fuzz_harness: bool,
}

impl Default for CBackendOptions {
    fn default() -> Self {
        Self {
            emit_fuzz_harness: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RustBackendOptions {}

// ── Checksum Binding (spec §16) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeStyle {
    PatchInPlace,
    ReturnValue,
}

#[derive(Debug, Clone)]
pub struct ChecksumBackendBinding {
    pub verify_symbol: Option<String>,
    pub compute_symbol: String,
    pub compute_style: ComputeStyle,
}

pub trait ChecksumBindingProvider: Send + Sync {
    fn binding_for(&self, algorithm: &str) -> Result<ChecksumBackendBinding, BackendError>;
}

/// No-op checksum binding provider for targets that don't support checksums.
pub struct NoChecksumBindings;

impl ChecksumBindingProvider for NoChecksumBindings {
    fn binding_for(&self, algorithm: &str) -> Result<ChecksumBackendBinding, BackendError> {
        Err(BackendError::MissingChecksumBinding {
            target: TargetId("unknown"),
            algorithm: algorithm.to_string(),
        })
    }
}

// ── Backend Error (spec §12) ──

#[derive(Debug)]
pub enum BackendError {
    UnsupportedTarget(TargetId),
    UnsupportedProfile {
        target: TargetId,
        profile: String,
        reason: String,
    },
    UnsupportedOption {
        target: TargetId,
        option: String,
        reason: String,
    },
    UnsupportedFeature {
        target: TargetId,
        feature: String,
        reason: String,
    },
    MissingChecksumBinding {
        target: TargetId,
        algorithm: String,
    },
    InvalidCodecInput {
        target: TargetId,
        reason: String,
    },
    EmitFailure {
        target: TargetId,
        reason: String,
    },
    Io {
        target: TargetId,
        path: PathBuf,
        reason: String,
    },
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedTarget(t) => write!(f, "unsupported target: {t}"),
            Self::UnsupportedProfile { target, profile, reason } => {
                write!(f, "target {target} does not support profile {profile}: {reason}")
            }
            Self::UnsupportedOption { target, option, reason } => {
                write!(f, "target {target}: unsupported option '{option}': {reason}")
            }
            Self::UnsupportedFeature { target, feature, reason } => {
                write!(f, "target {target}: unsupported feature '{feature}': {reason}")
            }
            Self::MissingChecksumBinding { target, algorithm } => {
                write!(f, "target {target}: no checksum binding for '{algorithm}'")
            }
            Self::InvalidCodecInput { target, reason } => {
                write!(f, "target {target}: invalid codec input: {reason}")
            }
            Self::EmitFailure { target, reason } => {
                write!(f, "target {target}: emit failure: {reason}")
            }
            Self::Io { target, path, reason } => {
                write!(f, "target {target}: I/O error at {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for BackendError {}

// ── Backend Traits (spec §6) ──

/// Typed backend trait for backend-local testing.
pub trait Backend {
    type LoweredModule;

    fn id(&self) -> TargetId;

    fn lower(
        &self,
        module: &CodecModule,
        ctx: &BackendContext,
    ) -> Result<Self::LoweredModule, BackendError>;

    fn emit(
        &self,
        lowered: &Self::LoweredModule,
        sink: &mut dyn ArtifactSink,
    ) -> Result<BackendOutput, BackendError>;
}

/// Object-safe backend trait for the registry/driver.
pub trait BackendDyn: Send + Sync {
    fn id(&self) -> TargetId;

    fn lower_and_emit(
        &self,
        module: &CodecModule,
        ctx: &BackendContext,
        sink: &mut dyn ArtifactSink,
    ) -> Result<BackendOutput, BackendError>;
}

// ── Backend Registry (spec §13) ──

pub trait BackendFactory: Send + Sync {
    fn id(&self) -> TargetId;
    fn create(&self) -> Box<dyn BackendDyn>;
}

pub struct BackendRegistry {
    backends: BTreeMap<&'static str, Box<dyn BackendFactory>>,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self {
            backends: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, factory: Box<dyn BackendFactory>) {
        self.backends.insert(factory.id().0, factory);
    }

    pub fn get(&self, target: TargetId) -> Result<Box<dyn BackendDyn>, BackendError> {
        self.backends
            .get(target.0)
            .map(|f| f.create())
            .ok_or(BackendError::UnsupportedTarget(target))
    }

    pub fn available_targets(&self) -> Vec<TargetId> {
        self.backends.keys().map(|k| TargetId(k)).collect()
    }
}

impl Default for BackendRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 2: Write tests**

```rust
// crates/wirespec-backend-api/tests/api_tests.rs
use wirespec_backend_api::*;

#[test]
fn target_id_display() {
    assert_eq!(TARGET_C.to_string(), "c");
    assert_eq!(TARGET_RUST.to_string(), "rust");
}

#[test]
fn target_id_equality() {
    assert_eq!(TARGET_C, TargetId("c"));
    assert_ne!(TARGET_C, TARGET_RUST);
}

#[test]
fn registry_unknown_target() {
    let reg = BackendRegistry::new();
    let result = reg.get(TargetId("zig"));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BackendError::UnsupportedTarget(_)));
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
        target: TARGET_C,
        kind: ArtifactKind::CHeader,
        module_name: "test".to_string(),
        module_prefix: "test".to_string(),
        relative_path: "test.h".into(),
        contents: b"// header\n".to_vec(),
    };
    sink.write(artifact).unwrap();
    assert_eq!(sink.artifacts.len(), 1);
    assert_eq!(sink.artifacts[0].0.kind, ArtifactKind::CHeader);
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
        target: TARGET_C,
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
        ArtifactKind::CHeader,
        ArtifactKind::CSource,
        ArtifactKind::CFuzzSource,
        ArtifactKind::RustSource,
        ArtifactKind::Other,
    ];
    assert_eq!(kinds.len(), 5);
}
```

- [ ] **Step 3: Verify build and tests**

Run: `cargo test -p wirespec-backend-api`
Expected: PASS

- [ ] **Step 4: Verify workspace**

Run: `cargo test --workspace`
Expected: All pass

- [ ] **Step 5: Commit**

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1 | Task 1 | Complete backend API: traits, types, registry, error taxonomy, artifact sink |

**Total: ~10 tests** covering target IDs, registry lookup, memory sink, checksum bindings, error display, option defaults.

This is a small, focused crate — pure API surface with no implementation logic. The actual backend implementations (`wirespec-backend-c`, `wirespec-backend-rust`) will implement these traits.
