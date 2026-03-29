use wirespec_sema::resolve::*;
use wirespec_sema::types::*;

#[test]
fn primitive_byte_widths() {
    assert_eq!(PrimitiveWireType::U8.byte_width(), 1);
    assert_eq!(PrimitiveWireType::U16.byte_width(), 2);
    assert_eq!(PrimitiveWireType::U32.byte_width(), 4);
    assert_eq!(PrimitiveWireType::U64.byte_width(), 8);
    assert_eq!(PrimitiveWireType::Bit.byte_width(), 0);
}

#[test]
fn integer_like_checks() {
    assert!(PrimitiveWireType::U8.is_integer_like());
    assert!(PrimitiveWireType::U64.is_integer_like());
    assert!(!PrimitiveWireType::Bool.is_integer_like());
    assert!(!PrimitiveWireType::Bit.is_integer_like());
}

#[test]
fn registry_resolves_primitives() {
    let reg = TypeRegistry::new(Endianness::Big);
    let resolved = reg.resolve_type_name("u16").unwrap();
    assert_eq!(
        resolved,
        ResolvedType::Primitive(PrimitiveWireType::U16, Some(Endianness::Big))
    );
}

#[test]
fn registry_resolves_explicit_endian() {
    let reg = TypeRegistry::new(Endianness::Big);
    let resolved = reg.resolve_type_name("u16le").unwrap();
    assert_eq!(
        resolved,
        ResolvedType::Primitive(PrimitiveWireType::U16, Some(Endianness::Little))
    );
}

#[test]
fn registry_user_type() {
    let mut reg = TypeRegistry::new(Endianness::Big);
    reg.register("VarInt", DeclKind::VarInt).unwrap();
    let resolved = reg.resolve_type_name("VarInt").unwrap();
    assert_eq!(
        resolved,
        ResolvedType::UserDefined("VarInt".into(), DeclKind::VarInt)
    );
}

#[test]
fn registry_unknown_type() {
    let reg = TypeRegistry::new(Endianness::Big);
    assert!(reg.resolve_type_name("Unknown").is_none());
}

#[test]
fn registry_alias_resolution() {
    let mut reg = TypeRegistry::new(Endianness::Little);
    reg.register_alias("AttHandle", "u16le");
    let resolved = reg.resolve_type_name("AttHandle").unwrap();
    assert_eq!(
        resolved,
        ResolvedType::Primitive(PrimitiveWireType::U16, Some(Endianness::Little))
    );
}
