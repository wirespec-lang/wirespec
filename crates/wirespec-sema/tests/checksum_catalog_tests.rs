use wirespec_sema::checksum_catalog;

#[test]
fn catalog_known_algorithms() {
    assert!(checksum_catalog::lookup("internet").is_some());
    assert!(checksum_catalog::lookup("crc32").is_some());
    assert!(checksum_catalog::lookup("crc32c").is_some());
    assert!(checksum_catalog::lookup("fletcher16").is_some());
}

#[test]
fn catalog_unknown_algorithm() {
    assert!(checksum_catalog::lookup("sha256").is_none());
}

#[test]
fn catalog_strict_profile() {
    let algos = checksum_catalog::algorithms_for_profile("phase2_strict_v1_0");
    assert!(algos.contains(&"internet"));
    assert!(algos.contains(&"crc32"));
    assert!(algos.contains(&"crc32c"));
    assert!(!algos.contains(&"fletcher16")); // extension only
}

#[test]
fn catalog_extended_profile() {
    let algos = checksum_catalog::algorithms_for_profile("phase2_extended_current");
    assert!(algos.contains(&"internet"));
    assert!(algos.contains(&"crc32"));
    assert!(algos.contains(&"crc32c"));
    assert!(algos.contains(&"fletcher16")); // included in extended
}

#[test]
fn catalog_field_metadata() {
    let internet = checksum_catalog::lookup("internet").unwrap();
    assert_eq!(internet.required_field_type, "u16");
    assert_eq!(internet.field_width_bytes, 2);

    let crc32 = checksum_catalog::lookup("crc32").unwrap();
    assert_eq!(crc32.required_field_type, "u32");
    assert_eq!(crc32.field_width_bytes, 4);
}

#[test]
fn catalog_verify_modes() {
    use checksum_catalog::{ChecksumInputModel, ChecksumVerifyMode};

    let internet = checksum_catalog::lookup("internet").unwrap();
    assert_eq!(internet.verify_mode, ChecksumVerifyMode::ZeroSum);
    assert_eq!(internet.input_model, ChecksumInputModel::ZeroSumWholeScope);

    let crc32 = checksum_catalog::lookup("crc32").unwrap();
    assert_eq!(crc32.verify_mode, ChecksumVerifyMode::RecomputeCompare);
    assert_eq!(
        crc32.input_model,
        ChecksumInputModel::RecomputeWithSkippedField
    );

    let fletcher16 = checksum_catalog::lookup("fletcher16").unwrap();
    assert_eq!(fletcher16.verify_mode, ChecksumVerifyMode::RecomputeCompare);
    assert_eq!(
        fletcher16.input_model,
        ChecksumInputModel::RecomputeWithSkippedField
    );
}
