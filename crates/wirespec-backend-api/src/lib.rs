// crates/wirespec-backend-api/src/lib.rs
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

pub use wirespec_codec::CodecModule;

// ── Target IDs (spec §7) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct TargetId(pub &'static str);

impl std::fmt::Display for TargetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Artifact (spec §7-§8) ──

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactKind(pub &'static str);

// Common constants (backends can define their own too)
impl ArtifactKind {
    pub const C_HEADER: ArtifactKind = ArtifactKind("c-header");
    pub const C_SOURCE: ArtifactKind = ArtifactKind("c-source");
    pub const C_FUZZ_SOURCE: ArtifactKind = ArtifactKind("c-fuzz-source");
    pub const RUST_SOURCE: ArtifactKind = ArtifactKind("rust-source");
    pub const OTHER: ArtifactKind = ArtifactKind("other");
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
    pub target_options: Box<dyn std::any::Any + Send + Sync>,
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
            Self::UnsupportedProfile {
                target,
                profile,
                reason,
            } => {
                write!(
                    f,
                    "target {target} does not support profile {profile}: {reason}"
                )
            }
            Self::UnsupportedOption {
                target,
                option,
                reason,
            } => {
                write!(
                    f,
                    "target {target}: unsupported option '{option}': {reason}"
                )
            }
            Self::UnsupportedFeature {
                target,
                feature,
                reason,
            } => {
                write!(
                    f,
                    "target {target}: unsupported feature '{feature}': {reason}"
                )
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
            Self::Io {
                target,
                path,
                reason,
            } => {
                write!(
                    f,
                    "target {target}: I/O error at {}: {reason}",
                    path.display()
                )
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
    fn default_options(&self) -> Box<dyn std::any::Any + Send + Sync>;
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

    pub fn get_factory(&self, target: TargetId) -> Result<&dyn BackendFactory, BackendError> {
        self.backends
            .get(target.0)
            .map(|f| f.as_ref())
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
