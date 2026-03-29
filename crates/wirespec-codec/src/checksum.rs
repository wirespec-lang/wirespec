// crates/wirespec-codec/src/checksum.rs
//
// ChecksumPlan synthesis from a scope's fields.

use crate::ir::*;
use wirespec_sema::checksum_catalog;

/// Synthesize a ChecksumPlan from a scope's fields.
/// Returns None if no field has a checksum annotation.
pub fn synthesize_checksum_plan(
    fields: &[CodecField],
    scope_kind: ScopeKind,
    scope_name: &str,
) -> Option<ChecksumPlan> {
    for (i, field) in fields.iter().enumerate() {
        if let Some(ref algorithm) = field.checksum_algorithm {
            let (verify_mode, input_model) = checksum_modes(algorithm);
            let field_width = checksum_field_width(algorithm);

            return Some(ChecksumPlan {
                scope_kind,
                scope_name: scope_name.to_string(),
                field_name: field.name.clone(),
                field_index_in_scope: i as u32,
                algorithm_id: algorithm.clone(),
                verify_mode,
                input_model,
                field_width_bytes: field_width,
                field_endianness: field.endianness,
            });
        }
    }
    None
}

fn checksum_modes(algorithm: &str) -> (ChecksumVerifyMode, ChecksumInputModel) {
    if let Some(spec) = checksum_catalog::lookup(algorithm) {
        (
            match spec.verify_mode {
                checksum_catalog::ChecksumVerifyMode::ZeroSum => ChecksumVerifyMode::ZeroSum,
                checksum_catalog::ChecksumVerifyMode::RecomputeCompare => {
                    ChecksumVerifyMode::RecomputeCompare
                }
            },
            match spec.input_model {
                checksum_catalog::ChecksumInputModel::ZeroSumWholeScope => {
                    ChecksumInputModel::ZeroSumWholeScope
                }
                checksum_catalog::ChecksumInputModel::RecomputeWithSkippedField => {
                    ChecksumInputModel::RecomputeWithSkippedField
                }
            },
        )
    } else {
        (
            ChecksumVerifyMode::RecomputeCompare,
            ChecksumInputModel::RecomputeWithSkippedField,
        )
    }
}

fn checksum_field_width(algorithm: &str) -> u8 {
    checksum_catalog::lookup(algorithm)
        .map(|s| s.field_width_bytes)
        .unwrap_or(0)
}
