use tower_lsp::lsp_types::*;
use wirespec_syntax::span::Span;

use crate::ast_helpers::extract_item_info;
use crate::position::{offset_to_position, span_to_range};

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
        let info = match extract_item_info(item) {
            Some(i) => i,
            None => continue,
        };

        let kind = item_symbol_kind(info.kind);

        if let Some(span) = &info.span {
            // `range` covers the keyword span (best we can do without a closing-brace span).
            let range = span_to_range(source, span);
            // `selection_range` points at the name identifier itself.
            let selection_range = find_name_range(source, span, &info.name)
                .unwrap_or(range);

            #[allow(deprecated)]
            symbols.push(DocumentSymbol {
                name: info.name,
                detail: None,
                kind,
                tags: None,
                deprecated: None,
                range,
                selection_range,
                children: None,
            });
        }
    }

    symbols
}

fn item_symbol_kind(kind: &str) -> SymbolKind {
    match kind {
        "packet" | "capsule" => SymbolKind::STRUCT,
        "frame" | "enum" | "flags" => SymbolKind::ENUM,
        "const" => SymbolKind::CONSTANT,
        "type" | "varint" => SymbolKind::TYPE_PARAMETER,
        "state machine" => SymbolKind::CLASS,
        _ => SymbolKind::OBJECT,
    }
}

/// Find the range of a named definition (skips the keyword to locate the name).
fn find_name_range(source: &str, item_span: &Span, name: &str) -> Option<Range> {
    let start_offset = item_span.offset as usize;
    let search_area = &source[start_offset..];
    if let Some(relative_pos) = search_area.find(name) {
        let name_offset = start_offset + relative_pos;
        let name_start = offset_to_position(source, name_offset);
        let name_end = offset_to_position(source, name_offset + name.len());
        Some(Range::new(name_start, name_end))
    } else {
        // Fallback to the full keyword span
        Some(span_to_range(source, item_span))
    }
}
