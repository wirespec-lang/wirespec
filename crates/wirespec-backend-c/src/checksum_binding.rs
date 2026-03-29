// crates/wirespec-backend-c/src/checksum_binding.rs
//
// C checksum runtime bindings.

use crate::TARGET_C;
use wirespec_backend_api::*;

pub struct CChecksumBindings;

impl ChecksumBindingProvider for CChecksumBindings {
    fn binding_for(&self, algorithm: &str) -> Result<ChecksumBackendBinding, BackendError> {
        match algorithm {
            "internet" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_internet_checksum".into()),
                compute_symbol: "wirespec_internet_checksum_compute".into(),
                compute_style: ComputeStyle::PatchInPlace,
            }),
            "crc32" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_crc32_verify".into()),
                compute_symbol: "wirespec_crc32_compute".into(),
                compute_style: ComputeStyle::ReturnValue,
            }),
            "crc32c" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_crc32c_verify".into()),
                compute_symbol: "wirespec_crc32c_compute".into(),
                compute_style: ComputeStyle::ReturnValue,
            }),
            "fletcher16" => Ok(ChecksumBackendBinding {
                verify_symbol: Some("wirespec_fletcher16_verify".into()),
                compute_symbol: "wirespec_fletcher16_compute".into(),
                compute_style: ComputeStyle::ReturnValue,
            }),
            _ => Err(BackendError::MissingChecksumBinding {
                target: TARGET_C,
                algorithm: algorithm.to_string(),
            }),
        }
    }
}
