use tower_lsp::lsp_types::*;

use crate::position::offset_to_position;
use wirespec_syntax::ast::*;
use wirespec_syntax::span::Span;

// Token type indices — must match the legend order.
const KEYWORD: u32 = 0;
const TYPE: u32 = 1;
const PROPERTY: u32 = 2;
// const VARIABLE: u32 = 3;
const NUMBER: u32 = 4;
const STRING: u32 = 5;
const COMMENT: u32 = 6;
const DECORATOR: u32 = 7;
const ENUM: u32 = 8;
const ENUM_MEMBER: u32 = 9;

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,           // 0
            SemanticTokenType::TYPE,              // 1
            SemanticTokenType::PROPERTY,          // 2
            SemanticTokenType::VARIABLE,          // 3
            SemanticTokenType::NUMBER,            // 4
            SemanticTokenType::STRING,            // 5
            SemanticTokenType::COMMENT,           // 6
            SemanticTokenType::DECORATOR,         // 7
            SemanticTokenType::new("enum"),       // 8
            SemanticTokenType::new("enumMember"), // 9
        ],
        token_modifiers: vec![],
    }
}

/// A raw token with absolute byte offset and length, before delta encoding.
struct RawToken {
    offset: usize,
    length: usize,
    token_type: u32,
}

pub fn compute_semantic_tokens(source: &str, ast: &AstModule) -> Vec<SemanticToken> {
    let mut raw: Vec<RawToken> = Vec::new();

    // Collect comments
    collect_comments(source, &mut raw);

    // Module declaration keyword
    if let Some(ref decl) = ast.module_decl
        && let Some(span) = &decl.span
    {
        emit_keyword_at_start(source, span, "module", &mut raw);
    }

    // Top-level annotations (before any item)
    for ann in &ast.annotations {
        collect_annotation(ann, &mut raw);
    }

    // Items
    for item in &ast.items {
        collect_top_item(source, item, &mut raw);
    }

    // Sort by offset
    raw.sort_by_key(|t| t.offset);

    // Delta-encode
    delta_encode(source, &raw)
}

fn collect_comments(source: &str, raw: &mut Vec<RawToken>) {
    let mut i = 0;
    let bytes = source.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'#' || (i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/') {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            raw.push(RawToken {
                offset: start,
                length: i - start,
                token_type: COMMENT,
            });
        } else if bytes[i] == b'"' {
            // Skip string literals so we don't match # inside strings
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1; // skip escaped char
                }
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // closing quote
            }
        } else {
            i += 1;
        }
    }
}

fn collect_annotation(ann: &AstAnnotation, raw: &mut Vec<RawToken>) {
    if let Some(span) = &ann.span {
        // The annotation span covers `@name` (or `@name(args)`)
        raw.push(RawToken {
            offset: span.offset as usize,
            length: span.len as usize,
            token_type: DECORATOR,
        });
    }
}

fn collect_top_item(source: &str, item: &AstTopItem, raw: &mut Vec<RawToken>) {
    match item {
        AstTopItem::Const(c) => collect_const(source, c, raw),
        AstTopItem::Enum(e) => collect_enum(source, e, raw),
        AstTopItem::Flags(f) => collect_flags(source, f, raw),
        AstTopItem::Type(t) => collect_type_decl(source, t, raw),
        AstTopItem::Packet(p) => collect_packet(source, p, raw),
        AstTopItem::Frame(f) => collect_frame(source, f, raw),
        AstTopItem::Capsule(c) => collect_capsule(source, c, raw),
        AstTopItem::ContinuationVarInt(v) => collect_varint(source, v, raw),
        AstTopItem::StateMachine(sm) => collect_state_machine(source, sm, raw),
        AstTopItem::StaticAssert(sa) => collect_static_assert(source, sa, raw),
        AstTopItem::ExternAsn1(ext) => collect_extern_asn1(source, ext, raw),
    }
}

fn collect_const(source: &str, c: &AstConstDecl, raw: &mut Vec<RawToken>) {
    for ann in &c.annotations {
        collect_annotation(ann, raw);
    }
    if let Some(span) = &c.span {
        // Keyword: "const" (or "export const")
        if c.exported {
            emit_keyword_at_start(source, span, "export", raw);
            let export_end = span.offset as usize + "export".len();
            if let Some(kw_off) = find_word_after(source, export_end, "const") {
                raw.push(RawToken {
                    offset: kw_off,
                    length: "const".len(),
                    token_type: KEYWORD,
                });
            }
        } else {
            emit_keyword_at_start(source, span, "const", raw);
        }
        // Name
        if let Some(name_off) = find_word_in_span(source, span, &c.name) {
            raw.push(RawToken {
                offset: name_off,
                length: c.name.len(),
                token_type: PROPERTY,
            });
        }
        // Type
        if let Some(type_off) = find_word_in_span(source, span, &c.type_name) {
            raw.push(RawToken {
                offset: type_off,
                length: c.type_name.len(),
                token_type: TYPE,
            });
        }
        // Value
        collect_literal_value(source, span, &c.value, raw);
    }
}

fn collect_enum(source: &str, e: &AstEnumDecl, raw: &mut Vec<RawToken>) {
    for ann in &e.annotations {
        collect_annotation(ann, raw);
    }
    if let Some(span) = &e.span {
        if e.exported {
            emit_keyword_at_start(source, span, "export", raw);
            let export_end = span.offset as usize + "export".len();
            if let Some(kw_off) = find_word_after(source, export_end, "enum") {
                raw.push(RawToken {
                    offset: kw_off,
                    length: "enum".len(),
                    token_type: KEYWORD,
                });
            }
        } else {
            emit_keyword_at_start(source, span, "enum", raw);
        }
        // Enum name
        if let Some(name_off) = find_word_in_span(source, span, &e.name) {
            raw.push(RawToken {
                offset: name_off,
                length: e.name.len(),
                token_type: ENUM,
            });
        }
        // Underlying type
        if let Some(type_off) = find_word_in_span(source, span, &e.underlying_type) {
            raw.push(RawToken {
                offset: type_off,
                length: e.underlying_type.len(),
                token_type: TYPE,
            });
        }
    }
    // Enum members
    for member in &e.members {
        collect_enum_member(source, member, raw);
    }
}

fn collect_flags(source: &str, f: &AstFlagsDecl, raw: &mut Vec<RawToken>) {
    for ann in &f.annotations {
        collect_annotation(ann, raw);
    }
    if let Some(span) = &f.span {
        if f.exported {
            emit_keyword_at_start(source, span, "export", raw);
            let export_end = span.offset as usize + "export".len();
            if let Some(kw_off) = find_word_after(source, export_end, "flags") {
                raw.push(RawToken {
                    offset: kw_off,
                    length: "flags".len(),
                    token_type: KEYWORD,
                });
            }
        } else {
            emit_keyword_at_start(source, span, "flags", raw);
        }
        if let Some(name_off) = find_word_in_span(source, span, &f.name) {
            raw.push(RawToken {
                offset: name_off,
                length: f.name.len(),
                token_type: ENUM,
            });
        }
        if let Some(type_off) = find_word_in_span(source, span, &f.underlying_type) {
            raw.push(RawToken {
                offset: type_off,
                length: f.underlying_type.len(),
                token_type: TYPE,
            });
        }
    }
    for member in &f.members {
        collect_enum_member(source, member, raw);
    }
}

fn collect_enum_member(source: &str, member: &AstEnumMember, raw: &mut Vec<RawToken>) {
    if let Some(span) = &member.span {
        // Member name
        if let Some(name_off) = find_word_in_span(source, span, &member.name) {
            raw.push(RawToken {
                offset: name_off,
                length: member.name.len(),
                token_type: ENUM_MEMBER,
            });
        }
        // Member value (a number)
        let value_str = format!("{}", member.value);
        // Look for the number representation in the span (could be hex, binary, or decimal)
        let span_text = &source[span.offset as usize..(span.offset + span.len) as usize];
        // Find the number after '=' sign
        if let Some(eq_pos) = span_text.find('=') {
            let after_eq = &span_text[eq_pos + 1..];
            let trimmed = after_eq.trim_start();
            let num_offset = span.offset as usize + eq_pos + 1 + (after_eq.len() - trimmed.len());
            // Determine the length of the number literal
            let num_len = trimmed
                .find(|c: char| !c.is_ascii_alphanumeric() && c != 'x' && c != 'b' && c != '_')
                .unwrap_or(trimmed.len());
            if num_len > 0 {
                raw.push(RawToken {
                    offset: num_offset,
                    length: num_len,
                    token_type: NUMBER,
                });
            }
        } else {
            // No '=', try to find the number in the span
            if let Some(num_off) = find_word_in_span(source, span, &value_str) {
                raw.push(RawToken {
                    offset: num_off,
                    length: value_str.len(),
                    token_type: NUMBER,
                });
            }
        }
    }
}

fn collect_type_decl(source: &str, t: &AstTypeDecl, raw: &mut Vec<RawToken>) {
    for ann in &t.annotations {
        collect_annotation(ann, raw);
    }
    if let Some(span) = &t.span {
        if t.exported {
            emit_keyword_at_start(source, span, "export", raw);
            let export_end = span.offset as usize + "export".len();
            if let Some(kw_off) = find_word_after(source, export_end, "type") {
                raw.push(RawToken {
                    offset: kw_off,
                    length: "type".len(),
                    token_type: KEYWORD,
                });
            }
        } else {
            emit_keyword_at_start(source, span, "type", raw);
        }
        if let Some(name_off) = find_word_in_span(source, span, &t.name) {
            raw.push(RawToken {
                offset: name_off,
                length: t.name.len(),
                token_type: TYPE,
            });
        }
    }
    match &t.body {
        AstTypeDeclBody::Fields { fields } => {
            for field in fields {
                collect_field_def(source, field, raw);
            }
        }
        AstTypeDeclBody::Alias { .. } => {}
    }
}

fn collect_packet(source: &str, p: &AstPacketDecl, raw: &mut Vec<RawToken>) {
    for ann in &p.annotations {
        collect_annotation(ann, raw);
    }
    if let Some(span) = &p.span {
        if p.exported {
            emit_keyword_at_start(source, span, "export", raw);
            let export_end = span.offset as usize + "export".len();
            if let Some(kw_off) = find_word_after(source, export_end, "packet") {
                raw.push(RawToken {
                    offset: kw_off,
                    length: "packet".len(),
                    token_type: KEYWORD,
                });
            }
        } else {
            emit_keyword_at_start(source, span, "packet", raw);
        }
        if let Some(name_off) = find_word_in_span(source, span, &p.name) {
            raw.push(RawToken {
                offset: name_off,
                length: p.name.len(),
                token_type: TYPE,
            });
        }
    }
    for field_item in &p.fields {
        collect_field_item(source, field_item, raw);
    }
}

fn collect_frame(source: &str, f: &AstFrameDecl, raw: &mut Vec<RawToken>) {
    for ann in &f.annotations {
        collect_annotation(ann, raw);
    }
    if let Some(span) = &f.span {
        if f.exported {
            emit_keyword_at_start(source, span, "export", raw);
            let export_end = span.offset as usize + "export".len();
            if let Some(kw_off) = find_word_after(source, export_end, "frame") {
                raw.push(RawToken {
                    offset: kw_off,
                    length: "frame".len(),
                    token_type: KEYWORD,
                });
            }
        } else {
            emit_keyword_at_start(source, span, "frame", raw);
        }
        if let Some(name_off) = find_word_in_span(source, span, &f.name) {
            raw.push(RawToken {
                offset: name_off,
                length: f.name.len(),
                token_type: TYPE,
            });
        }
    }
    for branch in &f.branches {
        for field_item in &branch.fields {
            collect_field_item(source, field_item, raw);
        }
    }
}

fn collect_capsule(source: &str, c: &AstCapsuleDecl, raw: &mut Vec<RawToken>) {
    for ann in &c.annotations {
        collect_annotation(ann, raw);
    }
    if let Some(span) = &c.span {
        if c.exported {
            emit_keyword_at_start(source, span, "export", raw);
            let export_end = span.offset as usize + "export".len();
            if let Some(kw_off) = find_word_after(source, export_end, "capsule") {
                raw.push(RawToken {
                    offset: kw_off,
                    length: "capsule".len(),
                    token_type: KEYWORD,
                });
            }
        } else {
            emit_keyword_at_start(source, span, "capsule", raw);
        }
        if let Some(name_off) = find_word_in_span(source, span, &c.name) {
            raw.push(RawToken {
                offset: name_off,
                length: c.name.len(),
                token_type: TYPE,
            });
        }
    }
    for field_item in &c.fields {
        collect_field_item(source, field_item, raw);
    }
    for branch in &c.branches {
        for field_item in &branch.fields {
            collect_field_item(source, field_item, raw);
        }
    }
}

fn collect_varint(source: &str, v: &AstContinuationVarIntDecl, raw: &mut Vec<RawToken>) {
    for ann in &v.annotations {
        collect_annotation(ann, raw);
    }
    if let Some(span) = &v.span {
        if v.exported {
            emit_keyword_at_start(source, span, "export", raw);
        }
        // "type" keyword
        let search_start = if v.exported {
            span.offset as usize + "export".len()
        } else {
            span.offset as usize
        };
        if let Some(kw_off) = find_word_after(source, search_start, "type") {
            raw.push(RawToken {
                offset: kw_off,
                length: "type".len(),
                token_type: KEYWORD,
            });
        }
        if let Some(name_off) = find_word_in_span(source, span, &v.name) {
            raw.push(RawToken {
                offset: name_off,
                length: v.name.len(),
                token_type: TYPE,
            });
        }
    }
}

fn collect_state_machine(source: &str, sm: &AstStateMachineDecl, raw: &mut Vec<RawToken>) {
    for ann in &sm.annotations {
        collect_annotation(ann, raw);
    }
    if let Some(span) = &sm.span {
        if sm.exported {
            emit_keyword_at_start(source, span, "export", raw);
        }
        // "state" keyword
        let search_start = if sm.exported {
            span.offset as usize + "export".len()
        } else {
            span.offset as usize
        };
        if let Some(kw_off) = find_word_after(source, search_start, "state") {
            raw.push(RawToken {
                offset: kw_off,
                length: "state".len(),
                token_type: KEYWORD,
            });
        }
        // "machine" keyword
        if let Some(kw_off) = find_word_after(source, search_start, "machine") {
            raw.push(RawToken {
                offset: kw_off,
                length: "machine".len(),
                token_type: KEYWORD,
            });
        }
        if let Some(name_off) = find_word_in_span(source, span, &sm.name) {
            raw.push(RawToken {
                offset: name_off,
                length: sm.name.len(),
                token_type: TYPE,
            });
        }
    }
}

fn collect_static_assert(source: &str, sa: &AstStaticAssertDecl, raw: &mut Vec<RawToken>) {
    if let Some(span) = &sa.span {
        emit_keyword_at_start(source, span, "static_assert", raw);
    }
}

fn collect_extern_asn1(source: &str, ext: &AstExternAsn1, raw: &mut Vec<RawToken>) {
    if let Some(span) = &ext.span {
        emit_keyword_at_start(source, span, "extern", raw);
        // Path is a string
        if let Some(q_off) = find_char_in_span(source, span, '"') {
            let text = &source[q_off..];
            if let Some(end_q) = text[1..].find('"') {
                raw.push(RawToken {
                    offset: q_off,
                    length: end_q + 2,
                    token_type: STRING,
                });
            }
        }
    }
}

fn collect_field_item(source: &str, item: &AstFieldItem, raw: &mut Vec<RawToken>) {
    match item {
        AstFieldItem::Field(f) => collect_field_def(source, f, raw),
        AstFieldItem::Derived(d) => collect_derived_field(source, d, raw),
        AstFieldItem::Require(r) => {
            if let Some(span) = &r.span {
                emit_keyword_at_start(source, span, "require", raw);
            }
        }
    }
}

fn collect_field_def(source: &str, field: &AstFieldDef, raw: &mut Vec<RawToken>) {
    for ann in &field.annotations {
        collect_annotation(ann, raw);
    }
    if let Some(span) = &field.span {
        // Field name (comes before the colon)
        if let Some(name_off) = find_word_at_start_of_span(source, span, &field.name) {
            raw.push(RawToken {
                offset: name_off,
                length: field.name.len(),
                token_type: PROPERTY,
            });
        }
        // Collect type expression tokens
        collect_type_expr(source, &field.type_expr, raw);
    }
}

fn collect_derived_field(source: &str, field: &AstDerivedField, raw: &mut Vec<RawToken>) {
    if let Some(span) = &field.span {
        // "let" keyword
        emit_keyword_at_start(source, span, "let", raw);
        // Field name
        if let Some(name_off) = find_word_in_span(source, span, &field.name) {
            raw.push(RawToken {
                offset: name_off,
                length: field.name.len(),
                token_type: PROPERTY,
            });
        }
        // Type name
        if let Some(type_off) = find_word_in_span(source, span, &field.type_name) {
            raw.push(RawToken {
                offset: type_off,
                length: field.type_name.len(),
                token_type: TYPE,
            });
        }
    }
}

fn collect_type_expr(source: &str, type_expr: &AstTypeExpr, raw: &mut Vec<RawToken>) {
    match type_expr {
        AstTypeExpr::Named { name, span } => {
            if let Some(span) = span
                && let Some(off) = find_word_in_span(source, span, name)
            {
                raw.push(RawToken {
                    offset: off,
                    length: name.len(),
                    token_type: TYPE,
                });
            }
        }
        AstTypeExpr::Match { branches, span, .. } => {
            if let Some(span) = span {
                emit_keyword_at_start(source, span, "match", raw);
            }
            for branch in branches {
                collect_type_expr(source, &branch.result_type, raw);
            }
        }
        AstTypeExpr::Array {
            element_type, span, ..
        } => {
            if let Some(span) = span {
                // Emit keyword for "fill" if it's a fill array
                let span_text = &source[span.offset as usize..(span.offset + span.len) as usize];
                if span_text.contains("fill")
                    && let Some(off) = find_word_in_span(source, span, "fill")
                {
                    raw.push(RawToken {
                        offset: off,
                        length: "fill".len(),
                        token_type: KEYWORD,
                    });
                }
            }
            collect_type_expr(source, element_type, raw);
        }
        AstTypeExpr::Optional {
            inner_type, span, ..
        } => {
            if let Some(span) = span {
                emit_keyword_at_start(source, span, "if", raw);
            }
            collect_type_expr(source, inner_type, raw);
        }
        AstTypeExpr::Bytes { span, .. } => {
            if let Some(span) = span
                && let Some(off) = find_word_in_span(source, span, "bytes")
            {
                raw.push(RawToken {
                    offset: off,
                    length: "bytes".len(),
                    token_type: TYPE,
                });
            }
        }
        AstTypeExpr::Bits { span, width } => {
            if let Some(span) = span {
                if let Some(off) = find_word_in_span(source, span, "bits") {
                    raw.push(RawToken {
                        offset: off,
                        length: "bits".len(),
                        token_type: TYPE,
                    });
                }
                // Width number
                let w_str = format!("{}", width);
                if let Some(off) = find_word_in_span(source, span, &w_str) {
                    raw.push(RawToken {
                        offset: off,
                        length: w_str.len(),
                        token_type: NUMBER,
                    });
                }
            }
        }
        AstTypeExpr::Asn1 {
            type_name, span, ..
        } => {
            if let Some(span) = span
                && let Some(off) = find_word_in_span(source, span, type_name)
            {
                raw.push(RawToken {
                    offset: off,
                    length: type_name.len(),
                    token_type: TYPE,
                });
            }
        }
    }
}

fn collect_literal_value(
    source: &str,
    span: &Span,
    value: &AstLiteralValue,
    raw: &mut Vec<RawToken>,
) {
    let span_end = (span.offset + span.len) as usize;
    let span_start = span.offset as usize;
    let span_text = &source[span_start..span_end];

    match value {
        AstLiteralValue::Int(v) => {
            // Find the number after '=' sign
            if let Some(eq_pos) = span_text.rfind('=') {
                let after_eq = &span_text[eq_pos + 1..];
                let trimmed = after_eq.trim_start();
                let num_offset = span_start + eq_pos + 1 + (after_eq.len() - trimmed.len());
                let num_len = trimmed
                    .find(|c: char| {
                        !c.is_ascii_alphanumeric() && c != 'x' && c != 'b' && c != '_' && c != '-'
                    })
                    .unwrap_or(trimmed.len());
                if num_len > 0 {
                    raw.push(RawToken {
                        offset: num_offset,
                        length: num_len,
                        token_type: NUMBER,
                    });
                }
            } else {
                let v_str = format!("{}", v);
                if let Some(off) = find_word_in_span(source, span, &v_str) {
                    raw.push(RawToken {
                        offset: off,
                        length: v_str.len(),
                        token_type: NUMBER,
                    });
                }
            }
        }
        AstLiteralValue::Bool(_) => {
            // "true" or "false" — could highlight as keyword
        }
        AstLiteralValue::String(s) => {
            // Find the quoted string
            if let Some(q_off) = find_char_in_span(source, span, '"') {
                raw.push(RawToken {
                    offset: q_off,
                    length: s.len() + 2, // include quotes
                    token_type: STRING,
                });
            }
        }
        AstLiteralValue::Null => {}
    }
}

// ── Helpers ──

/// Emit a keyword token if the span starts with the given keyword.
fn emit_keyword_at_start(source: &str, span: &Span, keyword: &str, raw: &mut Vec<RawToken>) {
    let start = span.offset as usize;
    let end = (span.offset + span.len) as usize;
    if end <= source.len() {
        let text = &source[start..end];
        let trimmed = text.trim_start();
        let ws = text.len() - trimmed.len();
        if trimmed.starts_with(keyword) {
            raw.push(RawToken {
                offset: start + ws,
                length: keyword.len(),
                token_type: KEYWORD,
            });
        }
    }
}

/// Find a whole word within a span's text. Returns the absolute byte offset.
fn find_word_in_span(source: &str, span: &Span, word: &str) -> Option<usize> {
    let start = span.offset as usize;
    let end = ((span.offset + span.len) as usize).min(source.len());
    let text = &source[start..end];
    let mut search_from = 0;
    while let Some(pos) = text[search_from..].find(word) {
        let abs = search_from + pos;
        let before_ok = abs == 0
            || !text.as_bytes()[abs - 1].is_ascii_alphanumeric()
                && text.as_bytes()[abs - 1] != b'_';
        let after = abs + word.len();
        let after_ok = after >= text.len()
            || !text.as_bytes()[after].is_ascii_alphanumeric() && text.as_bytes()[after] != b'_';
        if before_ok && after_ok {
            return Some(start + abs);
        }
        search_from = abs + 1;
    }
    None
}

/// Like find_word_in_span but only checks the beginning of the span (for field names before ':').
fn find_word_at_start_of_span(source: &str, span: &Span, word: &str) -> Option<usize> {
    let start = span.offset as usize;
    let end = ((span.offset + span.len) as usize).min(source.len());
    let text = &source[start..end];
    // Find colon to limit search to the part before it
    let limit = text.find(':').unwrap_or(text.len());
    let prefix = &text[..limit];
    if let Some(pos) = prefix.find(word) {
        let after = pos + word.len();
        let after_ok = after >= prefix.len()
            || !prefix.as_bytes()[after].is_ascii_alphanumeric()
                && prefix.as_bytes()[after] != b'_';
        let before_ok = pos == 0
            || !prefix.as_bytes()[pos - 1].is_ascii_alphanumeric()
                && prefix.as_bytes()[pos - 1] != b'_';
        if before_ok && after_ok {
            return Some(start + pos);
        }
    }
    None
}

/// Find a word occurring after a given absolute offset in source.
fn find_word_after(source: &str, after: usize, word: &str) -> Option<usize> {
    let text = &source[after..];
    let mut search_from = 0;
    while let Some(pos) = text[search_from..].find(word) {
        let abs = search_from + pos;
        let before_ok = abs == 0
            || !text.as_bytes()[abs - 1].is_ascii_alphanumeric()
                && text.as_bytes()[abs - 1] != b'_';
        let word_end = abs + word.len();
        let after_ok = word_end >= text.len()
            || !text.as_bytes()[word_end].is_ascii_alphanumeric()
                && text.as_bytes()[word_end] != b'_';
        if before_ok && after_ok {
            return Some(after + abs);
        }
        search_from = abs + 1;
    }
    None
}

/// Find the first occurrence of a character in a span.
fn find_char_in_span(source: &str, span: &Span, ch: char) -> Option<usize> {
    let start = span.offset as usize;
    let end = ((span.offset + span.len) as usize).min(source.len());
    let text = &source[start..end];
    text.find(ch).map(|pos| start + pos)
}

/// Convert raw tokens (absolute offsets) to delta-encoded LSP SemanticTokens.
fn delta_encode(source: &str, raw: &[RawToken]) -> Vec<SemanticToken> {
    let mut result = Vec::with_capacity(raw.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for tok in raw {
        let pos = offset_to_position(source, tok.offset);
        let delta_line = pos.line - prev_line;
        let delta_start = if delta_line == 0 {
            pos.character - prev_start
        } else {
            pos.character
        };

        result.push(SemanticToken {
            delta_line,
            delta_start,
            length: tok.length as u32,
            token_type: tok.token_type,
            token_modifiers_bitset: 0,
        });

        prev_line = pos.line;
        prev_start = pos.character;
    }

    result
}
