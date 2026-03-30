use wirespec_syntax::ast::*;
use wirespec_syntax::parse;

#[test]
fn parse_extern_asn1_single_type() {
    let ast = parse(r#"extern asn1 "supl/SUPL.asn1" { SuplPosInit }"#).unwrap();
    assert_eq!(ast.items.len(), 1);
    match &ast.items[0] {
        wirespec_syntax::ast::AstTopItem::ExternAsn1(e) => {
            assert_eq!(e.path, "supl/SUPL.asn1");
            assert_eq!(e.rust_module, None);
            assert_eq!(e.type_names, vec!["SuplPosInit"]);
        }
        other => panic!("expected ExternAsn1, got {:?}", other),
    }
}

#[test]
fn parse_extern_asn1_with_use() {
    let ast = parse(r#"extern asn1 "SUPL.asn1" use supl_types { SuplPosInit }"#).unwrap();
    match &ast.items[0] {
        wirespec_syntax::ast::AstTopItem::ExternAsn1(e) => {
            assert_eq!(e.path, "SUPL.asn1");
            assert_eq!(e.rust_module.as_deref(), Some("supl_types"));
            assert_eq!(e.type_names, vec!["SuplPosInit"]);
        }
        other => panic!("expected ExternAsn1, got {:?}", other),
    }
}

#[test]
fn parse_asn1_field_with_length() {
    let ast = parse(
        r#"
        extern asn1 "s.asn1" { Foo }
        packet P {
            length: u16,
            payload: asn1(Foo, encoding: uper, length: length),
        }
    "#,
    )
    .unwrap();
    let packet = match &ast.items[1] {
        AstTopItem::Packet(p) => p,
        other => panic!("expected Packet, got {:?}", other),
    };
    let payload_field = packet
        .fields
        .iter()
        .find(|f| matches!(f, AstFieldItem::Field(fd) if fd.name == "payload"))
        .unwrap();
    match payload_field {
        AstFieldItem::Field(fd) => match &fd.type_expr {
            AstTypeExpr::Asn1 {
                type_name,
                encoding,
                length,
                ..
            } => {
                assert_eq!(type_name, "Foo");
                assert_eq!(encoding, "uper");
                assert!(matches!(length, Asn1Length::Expr(_)));
            }
            other => panic!("expected Asn1 type, got {:?}", other),
        },
        _ => panic!("expected Field"),
    }
}

#[test]
fn parse_asn1_field_with_remaining() {
    let ast = parse(
        r#"
        extern asn1 "s.asn1" { Bar }
        packet P {
            payload: asn1(Bar, encoding: uper, remaining),
        }
    "#,
    )
    .unwrap();
    let packet = match &ast.items[1] {
        AstTopItem::Packet(p) => p,
        other => panic!("expected Packet, got {:?}", other),
    };
    let payload_field = packet
        .fields
        .iter()
        .find(|f| matches!(f, AstFieldItem::Field(fd) if fd.name == "payload"))
        .unwrap();
    match payload_field {
        AstFieldItem::Field(fd) => match &fd.type_expr {
            AstTypeExpr::Asn1 {
                type_name,
                encoding,
                length,
                ..
            } => {
                assert_eq!(type_name, "Bar");
                assert_eq!(encoding, "uper");
                assert!(matches!(length, Asn1Length::Remaining));
            }
            other => panic!("expected Asn1 type, got {:?}", other),
        },
        _ => panic!("expected Field"),
    }
}

#[test]
fn parse_extern_asn1_multiple_types() {
    let ast = parse(r#"extern asn1 "schema.asn1" { TypeA, TypeB, TypeC }"#).unwrap();
    match &ast.items[0] {
        wirespec_syntax::ast::AstTopItem::ExternAsn1(e) => {
            assert_eq!(e.type_names, vec!["TypeA", "TypeB", "TypeC"]);
        }
        other => panic!("expected ExternAsn1, got {:?}", other),
    }
}
