// crates/wirespec-sema/src/checksum_catalog.rs

/// All metadata for a checksum algorithm — single source of truth.
#[derive(Debug, Clone)]
pub struct ChecksumAlgorithmSpec {
    pub id: &'static str,
    pub required_field_type: &'static str, // "u16" or "u32"
    pub field_width_bytes: u8,             // 2 or 4
    pub verify_mode: ChecksumVerifyMode,
    pub input_model: ChecksumInputModel,
    pub min_profile: &'static str, // "phase2_strict_v1_0" or "phase2_extended_current"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumVerifyMode {
    ZeroSum,
    RecomputeCompare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumInputModel {
    ZeroSumWholeScope,
    RecomputeWithSkippedField,
}

/// Static catalog of all known checksum algorithms.
static CATALOG: &[ChecksumAlgorithmSpec] = &[
    ChecksumAlgorithmSpec {
        id: "internet",
        required_field_type: "u16",
        field_width_bytes: 2,
        verify_mode: ChecksumVerifyMode::ZeroSum,
        input_model: ChecksumInputModel::ZeroSumWholeScope,
        min_profile: "phase2_strict_v1_0",
    },
    ChecksumAlgorithmSpec {
        id: "crc32",
        required_field_type: "u32",
        field_width_bytes: 4,
        verify_mode: ChecksumVerifyMode::RecomputeCompare,
        input_model: ChecksumInputModel::RecomputeWithSkippedField,
        min_profile: "phase2_strict_v1_0",
    },
    ChecksumAlgorithmSpec {
        id: "crc32c",
        required_field_type: "u32",
        field_width_bytes: 4,
        verify_mode: ChecksumVerifyMode::RecomputeCompare,
        input_model: ChecksumInputModel::RecomputeWithSkippedField,
        min_profile: "phase2_strict_v1_0",
    },
    ChecksumAlgorithmSpec {
        id: "fletcher16",
        required_field_type: "u16",
        field_width_bytes: 2,
        verify_mode: ChecksumVerifyMode::RecomputeCompare,
        input_model: ChecksumInputModel::RecomputeWithSkippedField,
        min_profile: "phase2_extended_current",
    },
];

/// Look up an algorithm by name.
pub fn lookup(algorithm: &str) -> Option<&'static ChecksumAlgorithmSpec> {
    CATALOG.iter().find(|s| s.id == algorithm)
}

/// Get all algorithm IDs allowed under a given profile.
pub fn algorithms_for_profile(profile: &str) -> Vec<&'static str> {
    CATALOG
        .iter()
        .filter(|s| profile_includes(profile, s.min_profile))
        .map(|s| s.id)
        .collect()
}

fn profile_includes(active: &str, required: &str) -> bool {
    match (active, required) {
        (_, "phase2_strict_v1_0") => true, // strict is included in all profiles
        ("phase2_extended_current", "phase2_extended_current") => true,
        _ => false,
    }
}
