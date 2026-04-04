use tower_lsp::lsp_types::{Position, Range};

/// Convert byte offset to LSP Position (0-indexed).
pub fn offset_to_position(source: &str, offset: usize) -> Position {
    let (line, col) = wirespec_sema::error::offset_to_line_col(source, offset);
    Position::new((line - 1) as u32, (col - 1) as u32)
}

/// Convert a wirespec Span to an LSP Range.
pub fn span_to_range(source: &str, span: &wirespec_syntax::span::Span) -> Range {
    let start = offset_to_position(source, span.offset as usize);
    let end_offset = (span.offset + span.len) as usize;
    let end = offset_to_position(source, end_offset.min(source.len()));
    Range::new(start, end)
}

/// Convert LSP Position to byte offset.
pub fn position_to_offset(source: &str, position: Position) -> usize {
    let target_line = position.line as usize;
    let target_col = position.character as usize;
    let mut current_line = 0;
    let mut offset = 0;
    for (i, ch) in source.char_indices() {
        if current_line == target_line && i - offset >= target_col {
            return i;
        }
        if ch == '\n' {
            current_line += 1;
            offset = i + 1;
        }
    }
    source.len()
}

/// Find the word at a given byte offset. Returns (start, end, word).
pub fn word_at_offset(source: &str, offset: usize) -> (usize, usize, &str) {
    let bytes = source.as_bytes();
    let mut start = offset;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    (start, end, &source[start..end])
}
