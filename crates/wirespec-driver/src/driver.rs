// crates/wirespec-driver/src/driver.rs
//!
//! Multi-module compilation driver.
//!
//! Resolves all transitive dependencies from an entry file, then compiles
//! each module in topological order, registering exported types for
//! downstream modules.

use std::path::PathBuf;

use crate::pipeline::{self, ExternalTypes, PipelineError};
use crate::resolve;
use wirespec_codec::CodecModule;
use wirespec_sema::ComplianceProfile;

/// Request to compile an entry module and all its transitive dependencies.
pub struct CompileRequest {
    pub entry: PathBuf,
    pub include_paths: Vec<PathBuf>,
    pub profile: ComplianceProfile,
    pub asn1_modules: crate::pipeline::Asn1ModuleMap,
}

/// A single compiled module with its codec IR.
pub struct CompiledModule {
    pub module_name: String,
    pub source_prefix: String,
    pub codec: CodecModule,
}

/// Result of a multi-module compilation.
pub struct CompileResult {
    pub modules: Vec<CompiledModule>,
}

/// Error during driver operation.
#[derive(Debug)]
pub struct DriverError {
    pub msg: String,
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "driver error: {}", self.msg)
    }
}

impl std::error::Error for DriverError {}

impl From<resolve::ResolveError> for DriverError {
    fn from(e: resolve::ResolveError) -> Self {
        Self { msg: e.msg }
    }
}

impl From<PipelineError> for DriverError {
    fn from(e: PipelineError) -> Self {
        Self { msg: e.msg }
    }
}

/// Compile an entry module and all its transitive dependencies.
///
/// Modules are resolved in topological order (dependencies first), then
/// each module is compiled through the full pipeline. Types exported by
/// each module are registered in an `ExternalTypes` registry so downstream
/// modules can reference them during semantic analysis.
pub fn compile(request: &CompileRequest) -> Result<CompileResult, DriverError> {
    // 1. Resolve all transitive dependencies in topological order
    let resolved = resolve::resolve(&request.entry, &request.include_paths)?;

    // 2. Compile each module in topological order
    let mut external_types = ExternalTypes::default();
    let mut compiled = Vec::new();

    for module in &resolved {
        let codec = pipeline::compile_module(
            &module.source,
            request.profile,
            &external_types,
            &request.asn1_modules,
        )?;

        // Register exported types for downstream modules
        pipeline::collect_external_types(
            &mut external_types,
            &codec,
            &module.module_name,
            &module.source_prefix,
        );

        compiled.push(CompiledModule {
            module_name: module.module_name.clone(),
            source_prefix: module.source_prefix.clone(),
            codec,
        });
    }

    Ok(CompileResult { modules: compiled })
}
