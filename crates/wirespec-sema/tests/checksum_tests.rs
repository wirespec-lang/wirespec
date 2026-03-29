use wirespec_sema::ComplianceProfile;
use wirespec_sema::analyze;
use wirespec_syntax::parse;

#[test]
fn checksum_internet_accepted() {
    let src = r#"
        packet IPv4Header {
            version_ihl: u8,
            dscp_ecn: u8,
            total_length: u16,
            identification: u16,
            flags_fragment: u16,
            ttl: u8,
            protocol: u8,
            @checksum(internet)
            header_checksum: u16,
            src_addr: u32,
            dst_addr: u32,
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(sem.packets.len(), 1);
    let checksum_field = sem.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "header_checksum")
        .expect("header_checksum field should exist");
    assert_eq!(
        checksum_field.checksum_algorithm,
        Some("internet".to_string()),
        "internet checksum should be recorded on the field"
    );
}

#[test]
fn checksum_fletcher16_rejected_under_strict() {
    let src = r#"
        packet P {
            data: u8,
            @checksum(fletcher16)
            chk: u16,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(
        result.is_err(),
        "fletcher16 should be rejected under Phase2StrictV1_0 profile"
    );
}

#[test]
fn checksum_fletcher16_accepted_under_extended() {
    let src = r#"
        packet P {
            data: u8,
            @checksum(fletcher16)
            chk: u16,
        }
    "#;
    let ast = parse(src).unwrap();
    let sem = analyze(
        &ast,
        ComplianceProfile::Phase2ExtendedCurrent,
        &Default::default(),
    )
    .unwrap();
    let checksum_field = sem.packets[0]
        .fields
        .iter()
        .find(|f| f.name == "chk")
        .expect("chk field should exist");
    assert_eq!(
        checksum_field.checksum_algorithm,
        Some("fletcher16".to_string()),
        "fletcher16 should be accepted under Phase2ExtendedCurrent"
    );
}

#[test]
fn checksum_wrong_field_type_error() {
    // internet checksum requires u16 field type, but we use u32.
    let src = r#"
        packet P {
            data: u8,
            @checksum(internet)
            chk: u32,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(
        result.is_err(),
        "checksum field type mismatch should be rejected"
    );
}

#[test]
fn duplicate_checksum_error() {
    // Spec says at most one @checksum per packet/scope.
    let src = r#"
        packet P {
            @checksum(internet)
            chk1: u16,
            data: u8,
            @checksum(internet)
            chk2: u16,
        }
    "#;
    let ast = parse(src).unwrap();
    let result = analyze(
        &ast,
        ComplianceProfile::Phase2StrictV1_0,
        &Default::default(),
    );
    assert!(result.is_err(), "duplicate checksum should be rejected");
}
