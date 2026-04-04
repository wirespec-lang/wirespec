use tower_lsp::lsp_types::*;
use wirespec_syntax::ast::AstTopItem;

/// Compute document symbols for the outline view.
///
/// Each top-level definition (packet, frame, capsule, enum, flags, const,
/// type, state machine, varint) becomes a symbol entry.
pub fn compute_document_symbols(source: &str) -> Vec<DocumentSymbol> {
    let ast = match wirespec_syntax::parse(source) {
        Ok(ast) => ast,
        Err(_) => return vec![],
    };

    let mut symbols = Vec::new();

    for item in &ast.items {
        let (name, kind, span) = match item {
            AstTopItem::Packet(p) => (p.name.clone(), SymbolKind::STRUCT, &p.span),
            AstTopItem::Frame(f) => (f.name.clone(), SymbolKind::ENUM, &f.span),
            AstTopItem::Capsule(c) => (c.name.clone(), SymbolKind::STRUCT, &c.span),
            AstTopItem::Enum(e) => (e.name.clone(), SymbolKind::ENUM, &e.span),
            AstTopItem::Flags(f) => (f.name.clone(), SymbolKind::ENUM, &f.span),
            AstTopItem::Const(c) => (c.name.clone(), SymbolKind::CONSTANT, &c.span),
            AstTopItem::Type(t) => (t.name.clone(), SymbolKind::TYPE_PARAMETER, &t.span),
            AstTopItem::StateMachine(sm) => (sm.name.clone(), SymbolKind::CLASS, &sm.span),
            AstTopItem::ContinuationVarInt(v) => {
                (v.name.clone(), SymbolKind::TYPE_PARAMETER, &v.span)
            }
            _ => continue,
        };

        if let Some(span) = span {
            let start = crate::position::offset_to_position(source, span.offset as usize);
            let end = Position::new(start.line, start.character + span.len);
            let range = Range::new(start, end);

            #[allow(deprecated)]
            symbols.push(DocumentSymbol {
                name,
                detail: None,
                kind,
                tags: None,
                deprecated: None,
                range,
                selection_range: range,
                children: None,
            });
        }
    }

    symbols
}
