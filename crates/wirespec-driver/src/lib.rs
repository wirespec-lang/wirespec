// wirespec-driver: Compilation driver for wirespec
//!
//! This crate provides the multi-module compilation driver that:
//! 1. Resolves module dependencies (resolve)
//! 2. Compiles individual modules through the pipeline (pipeline)
//! 3. Orchestrates multi-module compilation (driver)

pub mod driver;
pub mod pipeline;
pub mod resolve;

pub use driver::{CompileRequest, CompileResult, CompiledModule, compile};
pub use pipeline::{ExternalType, ExternalTypeKind, ExternalTypes, compile_module};
