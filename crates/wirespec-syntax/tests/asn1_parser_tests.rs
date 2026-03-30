use wirespec_syntax::parse;

#[test]
fn parse_extern_asn1_single_type() {
    let ast = parse(r#"extern asn1 "supl/SUPL.asn1" { SuplPosInit }"#).unwrap();
    assert_eq!(ast.items.len(), 1);
    match &ast.items[0] {
        wirespec_syntax::ast::AstTopItem::ExternAsn1(e) => {
            assert_eq!(e.path, "supl/SUPL.asn1");
            assert_eq!(e.type_names, vec!["SuplPosInit"]);
        }
        other => panic!("expected ExternAsn1, got {:?}", other),
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
