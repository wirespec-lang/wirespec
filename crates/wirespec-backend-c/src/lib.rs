// crates/wirespec-backend-c/src/lib.rs
//
// C backend for wirespec: lowers Codec IR into .h header and .c source files.

pub mod checksum_binding;
pub mod expr;
pub mod header;
pub mod names;
pub mod parse_emit;
pub mod serialize_emit;
pub mod source;
pub mod type_map;

use wirespec_backend_api::*;
use wirespec_codec::CodecModule;

pub const TARGET_C: wirespec_backend_api::TargetId = wirespec_backend_api::TargetId("c");

pub struct CBackend;

impl Backend for CBackend {
    type LoweredModule = CLoweredModule;

    fn id(&self) -> TargetId {
        TARGET_C
    }

    fn lower(
        &self,
        module: &CodecModule,
        ctx: &BackendContext,
    ) -> Result<Self::LoweredModule, BackendError> {
        let opts = ctx
            .target_options
            .downcast_ref::<CBackendOptions>()
            .ok_or_else(|| BackendError::UnsupportedOption {
                target: TARGET_C,
                option: "target_options".into(),
                reason: "expected CBackendOptions".into(),
            })?;
        let emit_fuzz = opts.emit_fuzz_harness;

        let header_content = header::emit_header(module, &ctx.module_prefix);
        let source_content = source::emit_source(module, &ctx.module_prefix);
        let fuzz_content = if emit_fuzz {
            source::emit_fuzz_source(module, &ctx.module_prefix)
        } else {
            None
        };

        Ok(CLoweredModule {
            header_content,
            source_content,
            fuzz_content,
            prefix: ctx.module_prefix.clone(),
            emit_fuzz,
        })
    }

    fn emit(
        &self,
        lowered: &Self::LoweredModule,
        sink: &mut dyn ArtifactSink,
    ) -> Result<BackendOutput, BackendError> {
        let mut artifacts = Vec::new();

        // Header
        sink.write(Artifact {
            target: TARGET_C,
            kind: ArtifactKind::C_HEADER,
            module_name: lowered.prefix.clone(),
            module_prefix: lowered.prefix.clone(),
            relative_path: format!("{}.h", lowered.prefix).into(),
            contents: lowered.header_content.as_bytes().to_vec(),
        })?;
        artifacts.push(ArtifactMeta {
            kind: ArtifactKind::C_HEADER,
            relative_path: format!("{}.h", lowered.prefix).into(),
            byte_len: lowered.header_content.len(),
        });

        // Source
        sink.write(Artifact {
            target: TARGET_C,
            kind: ArtifactKind::C_SOURCE,
            module_name: lowered.prefix.clone(),
            module_prefix: lowered.prefix.clone(),
            relative_path: format!("{}.c", lowered.prefix).into(),
            contents: lowered.source_content.as_bytes().to_vec(),
        })?;
        artifacts.push(ArtifactMeta {
            kind: ArtifactKind::C_SOURCE,
            relative_path: format!("{}.c", lowered.prefix).into(),
            byte_len: lowered.source_content.len(),
        });

        // Fuzz harness
        if let Some(ref fuzz) = lowered.fuzz_content {
            sink.write(Artifact {
                target: TARGET_C,
                kind: ArtifactKind::C_FUZZ_SOURCE,
                module_name: lowered.prefix.clone(),
                module_prefix: lowered.prefix.clone(),
                relative_path: format!("{}_fuzz.c", lowered.prefix).into(),
                contents: fuzz.as_bytes().to_vec(),
            })?;
            artifacts.push(ArtifactMeta {
                kind: ArtifactKind::C_FUZZ_SOURCE,
                relative_path: format!("{}_fuzz.c", lowered.prefix).into(),
                byte_len: fuzz.len(),
            });
        }

        Ok(BackendOutput {
            target: TARGET_C,
            artifacts,
        })
    }
}

pub struct CLoweredModule {
    pub header_content: String,
    pub source_content: String,
    pub fuzz_content: Option<String>,
    pub prefix: String,
    pub emit_fuzz: bool,
}

impl BackendDyn for CBackend {
    fn id(&self) -> TargetId {
        TARGET_C
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
