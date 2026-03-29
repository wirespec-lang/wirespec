// crates/wirespec-backend-rust/src/checksum_binding.rs
//
// Rust checksum runtime bindings using wirespec_rt.

use wirespec_backend_api::*;
use crate::TARGET_RUST;

pub struct RustChecksumBindings;

impl ChecksumBindingProvider for RustChecksumBindings {
    fn binding_for(&self, algorithm: &str) -> Result<ChecksumBackendBinding, BackendError> {
        match algorithm {
            "internet" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_rt::internet_checksum".into()),
                compute_symbol: "wirespec_rt::internet_checksum_compute".into(),
                compute_style: ComputeStyle::PatchInPlace,
            }),
            "crc32" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_rt::crc32_verify".into()),
                compute_symbol: "wirespec_rt::crc32_compute".into(),
                compute_style: ComputeStyle::ReturnValue,
            }),
            "crc32c" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_rt::crc32c_verify".into()),
                compute_symbol: "wirespec_rt::crc32c_compute".into(),
                compute_style: ComputeStyle::ReturnValue,
            }),
            "fletcher16" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_rt::fletcher16_verify".into()),
                compute_symbol: "wirespec_rt::fletcher16_compute".into(),
                compute_style: ComputeStyle::ReturnValue,
            }),
            _ => Err(BackendError::MissingChecksumBinding {
                target: TARGET_RUST,
                algorithm: algorithm.to_string(),
            }),
        }
    }
}
