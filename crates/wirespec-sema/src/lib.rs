// crates/wirespec-sema/src/lib.rs
pub mod analyzer;
pub mod checksum_catalog;
pub mod error;
pub mod expr;
pub mod ir;
pub mod profile;
pub mod resolve;
pub mod types;
pub mod validate;

pub use analyzer::analyze;
pub use ir::SemanticModule;
pub use profile::ComplianceProfile;
