use wirespec_syntax::ast::AstTopItem;
use wirespec_syntax::span::Span;

/// Information extracted from a top-level AST item.
pub struct ItemInfo {
    pub name: String,
    pub span: Option<Span>,
    pub kind: &'static str,
}

/// Extract name, span, and kind string from a top-level AST item.
///
/// Returns `None` for items that do not carry a user-visible name
/// (e.g. `StaticAssert`, `ExternAsn1`).
pub fn extract_item_info(item: &AstTopItem) -> Option<ItemInfo> {
    match item {
        AstTopItem::Packet(p) => Some(ItemInfo {
            name: p.name.clone(),
            span: p.span,
            kind: "packet",
        }),
        AstTopItem::Frame(f) => Some(ItemInfo {
            name: f.name.clone(),
            span: f.span,
            kind: "frame",
        }),
        AstTopItem::Capsule(c) => Some(ItemInfo {
            name: c.name.clone(),
            span: c.span,
            kind: "capsule",
        }),
        AstTopItem::Enum(e) => Some(ItemInfo {
            name: e.name.clone(),
            span: e.span,
            kind: "enum",
        }),
        AstTopItem::Flags(f) => Some(ItemInfo {
            name: f.name.clone(),
            span: f.span,
            kind: "flags",
        }),
        AstTopItem::Const(c) => Some(ItemInfo {
            name: c.name.clone(),
            span: c.span,
            kind: "const",
        }),
        AstTopItem::Type(t) => Some(ItemInfo {
            name: t.name.clone(),
            span: t.span,
            kind: "type",
        }),
        AstTopItem::StateMachine(sm) => Some(ItemInfo {
            name: sm.name.clone(),
            span: sm.span,
            kind: "state machine",
        }),
        AstTopItem::ContinuationVarInt(v) => Some(ItemInfo {
            name: v.name.clone(),
            span: v.span,
            kind: "varint",
        }),
        _ => None,
    }
}
