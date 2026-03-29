// crates/wirespec-backend-rust/src/lib.rs
//
// Rust backend for wirespec: lowers Codec IR into .rs source files.

pub mod checksum_binding;
pub mod emit;
pub mod expr;
pub mod names;
pub mod parse_emit;
pub mod serialize_emit;
pub mod type_map;

use wirespec_backend_api::*;
use wirespec_codec::CodecModule;

pub const TARGET_RUST: wirespec_backend_api::TargetId = wirespec_backend_api::TargetId("rust");

pub struct RustBackend;

impl Backend for RustBackend {
    type LoweredModule = RustLoweredModule;

    fn id(&self) -> TargetId {
        TARGET_RUST
    }

    fn lower(
        &self,
        module: &CodecModule,
        ctx: &BackendContext,
    ) -> Result<Self::LoweredModule, BackendError> {
        ctx.target_options
            .downcast_ref::<RustBackendOptions>()
            .ok_or_else(|| BackendError::UnsupportedOption {
                target: TARGET_RUST,
                option: "target_options".into(),
                reason: "expected RustBackendOptions".into(),
            })?;

        let source = emit::emit_source(module, &ctx.module_prefix);

        Ok(RustLoweredModule {
            source,
            prefix: ctx.module_prefix.clone(),
        })
    }

    fn emit(
        &self,
        lowered: &Self::LoweredModule,
        sink: &mut dyn ArtifactSink,
    ) -> Result<BackendOutput, BackendError> {
        let mut artifacts = Vec::new();

        sink.write(Artifact {
            target: TARGET_RUST,
            kind: ArtifactKind::RUST_SOURCE,
            module_name: lowered.prefix.clone(),
            module_prefix: lowered.prefix.clone(),
            relative_path: format!("{}.rs", lowered.prefix).into(),
            contents: lowered.source.as_bytes().to_vec(),
        })?;
        artifacts.push(ArtifactMeta {
            kind: ArtifactKind::RUST_SOURCE,
            relative_path: format!("{}.rs", lowered.prefix).into(),
            byte_len: lowered.source.len(),
        });

        Ok(BackendOutput {
            target: TARGET_RUST,
            artifacts,
        })
    }
}

pub struct RustLoweredModule {
    pub source: String,
    pub prefix: String,
}

impl BackendDyn for RustBackend {
    fn id(&self) -> TargetId {
        TARGET_RUST
    }

    fn lower_and_emit(
        &self,
        module: &CodecModule,
        ctx: &BackendContext,
        sink: &mut dyn ArtifactSink,
    ) -> Result<BackendOutput, BackendError> {
        let lowered = Backend::lower(self, module, ctx)?;
        Backend::emit(self, &lowered, sink)
    }
}
