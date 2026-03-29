// crates/wirespec-sema/src/profile.rs

/// Compliance profile per COMPLIANCE_PROFILE_SPEC.md
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplianceProfile {
    Phase2StrictV1_0,
    Phase2ExtendedCurrent,
}

impl ComplianceProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phase2StrictV1_0 => "phase2_strict_v1_0",
            Self::Phase2ExtendedCurrent => "phase2_extended_current",
        }
    }

    /// Checksum algorithms allowed under this profile.
    pub fn allowed_checksum_algorithms(self) -> Vec<&'static str> {
        crate::checksum_catalog::algorithms_for_profile(self.as_str())
    }

    /// Whether capsule-scope checksums are allowed.
    pub fn allows_capsule_checksum(self) -> bool {
        match self {
            Self::Phase2StrictV1_0 => false,
            Self::Phase2ExtendedCurrent => true,
        }
    }
}

impl Default for ComplianceProfile {
    fn default() -> Self {
        // Migration default: extended current
        Self::Phase2ExtendedCurrent
    }
}

/// Expected field type for a checksum algorithm.
pub fn checksum_required_type(algorithm: &str) -> Option<&'static str> {
    crate::checksum_catalog::lookup(algorithm).map(|s| s.required_field_type)
}

/// Field width in bytes for a checksum algorithm.
pub fn checksum_field_width(algorithm: &str) -> Option<u8> {
    crate::checksum_catalog::lookup(algorithm).map(|s| s.field_width_bytes)
}
