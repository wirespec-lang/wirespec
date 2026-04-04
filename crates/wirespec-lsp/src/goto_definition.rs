use tower_lsp::lsp_types::*;
use wirespec_syntax::span::Span;

use crate::ast_helpers::extract_item_info;
use crate::position::{offset_to_position, span_to_range};

/// Find the definition range of the type name under the cursor.
///
/// Returns `None` if the word at the cursor does not match any top-level
/// definition in the file.
pub fn find_definition(source: &str, position: Position) -> Option<Range> {
    let offset = crate::position::position_to_offset(source, position);
    let (_, _, word) = crate::position::word_at_offset(source, offset);
    if word.is_empty() {
        return None;
    }

    let ast = wirespec_syntax::parse(source).ok()?;

    for item in &ast.items {
        let info = match extract_item_info(item) {
            Some(i) => i,
            None => continue,
        };
        if info.name == word
            && let Some(span) = &info.span
        {
            return find_name_range(source, span, &info.name);
        }
    }
    None
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
