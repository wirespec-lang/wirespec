use tower_lsp::lsp_types::*;
use wirespec_syntax::ast::AstTopItem;

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
        let (name, span) = match item {
            AstTopItem::Packet(p) => (&p.name, &p.span),
            AstTopItem::Frame(f) => (&f.name, &f.span),
            AstTopItem::Capsule(c) => (&c.name, &c.span),
            AstTopItem::Enum(e) => (&e.name, &e.span),
            AstTopItem::Flags(f) => (&f.name, &f.span),
            AstTopItem::Const(c) => (&c.name, &c.span),
            AstTopItem::Type(t) => (&t.name, &t.span),
            AstTopItem::StateMachine(sm) => (&sm.name, &sm.span),
            AstTopItem::ContinuationVarInt(v) => (&v.name, &v.span),
            _ => continue,
        };
        if name == word
            && let Some(span) = span
        {
            let start = crate::position::offset_to_position(source, span.offset as usize);
            let end = Position::new(start.line, start.character + span.len);
            return Some(Range::new(start, end));
        }
    }
    None
}
