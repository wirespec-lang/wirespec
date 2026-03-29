// crates/wirespec-sema/src/validate.rs
use crate::error::*;
use wirespec_syntax::span::Span;

/// Simplified field descriptor for scope-level validation.
pub struct FieldDescriptor {
    pub name: String,
    pub is_remaining: bool,
    pub is_fill: bool,
    pub is_wire: bool,
}

/// Spec §3.15: bytes[remaining] and [T; fill] must be the last wire field in scope.
pub fn validate_remaining_is_last(fields: &[FieldDescriptor]) -> SemaResult<()> {
    let wire_fields: Vec<_> = fields.iter().filter(|f| f.is_wire).collect();
    for (i, field) in wire_fields.iter().enumerate() {
        if (field.is_remaining || field.is_fill) && i < wire_fields.len() - 1 {
            return Err(SemaError::new(
                ErrorKind::RemainingNotLast,
                format!(
                    "field '{}' consumes remaining scope but is not the last wire field",
                    field.name
                ),
            ));
        }
    }
    Ok(())
}

/// Spec §3.11: at most one @checksum per scope.
pub fn validate_single_checksum(
    checksum_fields: &[&str],
    scope_desc: &str,
) -> SemaResult<()> {
    if checksum_fields.len() > 1 {
        return Err(SemaError::new(
            ErrorKind::DuplicateChecksum,
            format!(
                "multiple @checksum fields in {scope_desc}: {}",
                checksum_fields.join(", ")
            ),
        ));
    }
    Ok(())
}

/// Spec §3.14: fields may only reference previously declared fields.
pub fn validate_no_forward_refs(
    referenced: &[String],
    declared: &[String],
    field_name: &str,
    span: Option<Span>,
) -> SemaResult<()> {
    for name in referenced {
        if !declared.contains(name) {
            return Err(
                SemaError::new(
                    ErrorKind::ForwardReference,
                    format!("field '{field_name}' references undeclared '{name}'"),
                )
                .with_span(span)
                .with_hint(format!("'{name}' must be declared before '{field_name}'")),
            );
        }
    }
    Ok(())
}

/// Validate checksum field type matches algorithm requirement.
pub fn validate_checksum_field_type(
    algorithm: &str,
    field_type_name: &str,
    field_name: &str,
) -> SemaResult<()> {
    let required = crate::profile::checksum_required_type(algorithm);
    if let Some(req) = required {
        if field_type_name != req {
            return Err(SemaError::new(
                ErrorKind::InvalidChecksumType,
                format!(
                    "@checksum({algorithm}) requires field type '{req}', but '{field_name}' has type '{field_type_name}'"
                ),
            ));
        }
    }
    Ok(())
}

/// Validate checksum algorithm is allowed under profile.
pub fn validate_checksum_profile(
    algorithm: &str,
    profile: crate::profile::ComplianceProfile,
) -> SemaResult<()> {
    if !profile
        .allowed_checksum_algorithms()
        .contains(&algorithm)
    {
        return Err(SemaError::new(
            ErrorKind::ChecksumProfileViolation,
            format!(
                "@checksum({algorithm}) is not available in profile {}",
                profile.as_str()
            ),
        )
        .with_hint(format!(
            "use --profile phase2_extended_current to enable extension algorithms"
        )));
    }
    Ok(())
}
