use wirespec_sema::{ComplianceProfile, analyze};
use wirespec_syntax::parse;

fn default_profile() -> ComplianceProfile {
    ComplianceProfile::default()
}

#[test]
fn remaining_not_last_rejected() {
    let ast = parse("packet P { data: bytes[remaining], x: u8 }").unwrap();
    assert!(analyze(&ast, default_profile(), &Default::default()).is_err());
}

#[test]
fn remaining_last_ok() {
    let ast = parse("packet P { x: u8, data: bytes[remaining] }").unwrap();
    assert!(analyze(&ast, default_profile(), &Default::default()).is_ok());
}

#[test]
fn duplicate_checksum_rejected() {
    let src = "packet P { @checksum(internet) a: u16, @checksum(crc32) b: u32 }";
    assert!(analyze(&parse(src).unwrap(), default_profile(), &Default::default()).is_err());
}

#[test]
fn checksum_wrong_type_rejected() {
    let src = "packet P { @checksum(internet) c: u32 }";
    assert!(analyze(&parse(src).unwrap(), default_profile(), &Default::default()).is_err());
}

#[test]
fn checksum_correct_type_ok() {
    let src = "packet P { data: u32, @checksum(internet) c: u16 }";
    assert!(analyze(&parse(src).unwrap(), default_profile(), &Default::default()).is_ok());
}

#[test]
fn cyclic_alias_rejected() {
    let ast = parse("type A = B\ntype B = A\npacket P { x: A }").unwrap();
    assert!(analyze(&ast, default_profile(), &Default::default()).is_err());
}

#[test]
fn derive_traits_extracted() {
    let src = "@derive(debug, compare)\npacket P { x: u8 }";
    let sem = analyze(&parse(src).unwrap(), default_profile(), &Default::default()).unwrap();
    assert!(!sem.packets[0].derive_traits.is_empty());
}

#[test]
fn reserved_identifier_rejected() {
    let ast = parse("packet bool { x: u8 }").unwrap();
    assert!(analyze(&ast, default_profile(), &Default::default()).is_err());
}

#[test]
fn duplicate_definition_rejected() {
    let ast = parse("packet P { x: u8 }\npacket P { y: u16 }").unwrap();
    assert!(analyze(&ast, default_profile(), &Default::default()).is_err());
}

#[test]
fn endianness_explicit_override() {
    let ast = parse("@endian big\nmodule test\npacket P { x: u16le }").unwrap();
    let sem = analyze(&ast, default_profile(), &Default::default()).unwrap();
    if let wirespec_sema::types::SemanticType::Primitive { endianness, .. } =
        &sem.packets[0].fields[0].ty
    {
        assert_eq!(*endianness, Some(wirespec_sema::types::Endianness::Little));
    } else {
        panic!("expected Primitive type");
    }
}
