// wirespec-driver: Compilation driver for wirespec
//!
//! This crate provides the multi-module compilation driver that:
//! 1. Resolves module dependencies (resolve)
//! 2. Compiles individual modules through the pipeline (pipeline)
//! 3. Orchestrates multi-module compilation (driver)

pub mod driver;
pub mod pipeline;
pub mod resolve;

#[cfg(feature = "asn1")]
pub mod asn1_compile;

pub use driver::{CompileRequest, CompileResult, CompiledModule, compile};
pub use pipeline::{
    Asn1ModuleInfo, Asn1ModuleMap, ExternalType, ExternalTypeKind, ExternalTypes, compile_module,
};
