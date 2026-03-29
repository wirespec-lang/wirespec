use wirespec_sema::validate::*;
use wirespec_sema::error::ErrorKind;

#[test]
fn remaining_not_last_ok() {
    // bytes[remaining] as last wire field — should pass
    let fields = vec![
        mock_wire_field("x", false, false),
        mock_wire_field("data", true, false), // remaining
    ];
    assert!(validate_remaining_is_last(&fields).is_ok());
}

#[test]
fn remaining_not_last_err() {
    // bytes[remaining] not last — should fail
    let fields = vec![
        mock_wire_field("data", true, false), // remaining
        mock_wire_field("x", false, false),
    ];
    let err = validate_remaining_is_last(&fields).unwrap_err();
    assert_eq!(err.kind, ErrorKind::RemainingNotLast);
}

#[test]
fn fill_not_last_ok() {
    let fields = vec![
        mock_wire_field("x", false, false),
        mock_wire_field("items", false, true), // fill
    ];
    assert!(validate_remaining_is_last(&fields).is_ok());
}

#[test]
fn single_checksum_ok() {
    let checksums = vec!["checksum"];
    assert!(validate_single_checksum(&checksums, "packet Foo").is_ok());
}

#[test]
fn duplicate_checksum_err() {
    let checksums = vec!["checksum1", "checksum2"];
    let err = validate_single_checksum(&checksums, "packet Foo").unwrap_err();
    assert_eq!(err.kind, ErrorKind::DuplicateChecksum);
}

#[test]
fn forward_ref_detected() {
    // Field "data" references "length" which hasn't been declared yet
    let declared = vec!["src_port".to_string()];
    let refs = vec!["length".to_string()];
    let err = validate_no_forward_refs(&refs, &declared, "data", None).unwrap_err();
    assert_eq!(err.kind, ErrorKind::ForwardReference);
}

#[test]
fn forward_ref_ok() {
    let declared = vec!["length".to_string()];
    let refs = vec!["length".to_string()];
    assert!(validate_no_forward_refs(&refs, &declared, "data", None).is_ok());
}

// Helper to create a simplified field descriptor for validation
fn mock_wire_field(name: &str, is_remaining: bool, is_fill: bool) -> FieldDescriptor {
    FieldDescriptor {
        name: name.to_string(),
        is_remaining,
        is_fill,
        is_wire: true,
    }
}
