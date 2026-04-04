pub mod ast_helpers;
pub mod backend;
pub mod completion;
pub mod diagnostics;
pub mod document_symbols;
pub mod goto_definition;
pub mod hover;
pub mod position;
pub mod semantic_tokens;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_definition_packet() {
        // source:  "packet Foo { x: u8 }\npacket Bar { f: Foo }"
        // line 1:   packet Bar { f: Foo }
        //           0123456789012345678
        //                           ^-- col 16 is 'F' in "Foo"
        let source = "packet Foo { x: u8 }\npacket Bar { f: Foo }";
        let pos = tower_lsp::lsp_types::Position::new(1, 16); // 'F' in "Foo"
        let range = goto_definition::find_definition(source, pos);
        assert!(range.is_some(), "should find definition of Foo");
        let range = range.unwrap();
        // Should point at "Foo" in "packet Foo", not at "packet"
        assert_eq!(range.start.line, 0);
    }

    #[test]
    fn test_find_definition_not_found() {
        let source = "packet Foo { x: u8 }";
        let pos = tower_lsp::lsp_types::Position::new(0, 15); // inside field type "u8"
        let range = goto_definition::find_definition(source, pos);
        // u8 is a primitive, not a user definition
        assert!(range.is_none());
    }

    #[test]
    fn test_document_symbols() {
        let source = "packet Foo { x: u8 }\nenum Bar: u8 { A = 0 }\nconst MAX: u8 = 10";
        let symbols = document_symbols::compute_document_symbols(source);
        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[0].name, "Foo");
        assert_eq!(symbols[1].name, "Bar");
        assert_eq!(symbols[2].name, "MAX");
    }

    #[test]
    fn test_document_symbols_empty_source() {
        let symbols = document_symbols::compute_document_symbols("");
        assert!(symbols.is_empty());
    }

    #[test]
    fn test_document_symbols_parse_error() {
        let symbols = document_symbols::compute_document_symbols("packet { broken");
        assert!(symbols.is_empty());
    }
}
