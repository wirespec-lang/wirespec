use tower_lsp::lsp_types::*;

use crate::position::position_to_offset;

// ── Keyword lists ──

const TOP_LEVEL_KEYWORDS: &[(&str, &str)] = &[
    ("packet", "packet ${1:Name} {\n\t$0\n}"),
    ("frame", "frame ${1:Name} (${2:tag}: ${3:u8}) {\n\t$0\n}"),
    ("capsule", "capsule ${1:Name} {\n\t$0\n}"),
    ("enum", "enum ${1:Name} : ${2:u8} {\n\t$0\n}"),
    ("flags", "flags ${1:Name} : ${2:u8} {\n\t$0\n}"),
    ("const", "const ${1:NAME}: ${2:u8} = ${3:0}"),
    ("type", "type ${1:Name} (varint)"),
    ("state machine", "state machine ${1:Name} {\n\t$0\n}"),
    ("extern asn1", "extern asn1 \"${1:path}\" {\n\t$0\n}"),
    ("import", "import ${1:module}"),
    ("module", "module ${1:name}"),
];

const TYPE_KEYWORDS: &[&str] = &[
    "u8",
    "u16",
    "u24",
    "u32",
    "u64",
    "i8",
    "i16",
    "i32",
    "i64",
    "bool",
    "bytes",
    "bits",
    "bit",
    "remaining",
    "fill",
];

const ANNOTATION_NAMES: &[(&str, &str)] = &[
    ("endian", "endian(${1:big})"),
    ("checksum", "checksum(${1:algorithm})"),
    ("strict", "strict"),
    ("doc", "doc(\"${1:description}\")"),
    ("max_len", "max_len(${1:255})"),
    ("derive", "derive(${1:Debug})"),
];

const SM_KEYWORDS: &[&str] = &[
    "transition",
    "initial",
    "state",
    "on",
    "guard",
    "action",
    "delegate",
];

const ENCODING_NAMES: &[&str] = &["uper", "ber", "der", "aper", "oer", "coer"];

// ── Context detection ──

#[derive(Debug, PartialEq)]
enum CompletionContext {
    /// After `@` — suggest annotation names
    Annotation,
    /// After `:` in a field/type position — suggest type keywords + user types
    Type,
    /// After `encoding:` — suggest codec names
    Encoding,
    /// Inside `state machine { }` body
    StateMachine,
    /// Top-level (beginning of a declaration) or unknown context
    TopLevel,
}

/// Detect what kind of completion is appropriate at the given offset.
fn detect_context(source: &str, offset: usize) -> CompletionContext {
    let prefix = &source[..offset.min(source.len())];

    // Walk backwards over whitespace to find the last non-whitespace character/token.
    let trimmed = prefix.trim_end();

    if trimmed.ends_with('@') {
        return CompletionContext::Annotation;
    }

    // Check if we're after `encoding:` (case-sensitive, wirespec keyword)
    if let Some(colon_pos) = trimmed.rfind(':') {
        let before_colon = trimmed[..colon_pos].trim_end();
        if before_colon.ends_with("encoding") {
            return CompletionContext::Encoding;
        }
        // Generic `:` => type context
        return CompletionContext::Type;
    }

    // Check brace depth to detect if we're inside a `state machine { }` block.
    if inside_state_machine(source, offset) {
        return CompletionContext::StateMachine;
    }

    // Default: top-level
    CompletionContext::TopLevel
}

/// Returns true if `offset` is inside a `state machine { }` body (but not
/// nested inside an inner brace pair that would belong to a sub-item).
fn inside_state_machine(source: &str, offset: usize) -> bool {
    let prefix = &source[..offset.min(source.len())];
    // Find the last occurrence of `state machine` before the cursor
    let sm_kw = "state machine";
    let Some(sm_pos) = rfind_keyword(prefix, sm_kw) else {
        return false;
    };
    // Count braces from that position to the cursor
    let after_sm = &prefix[sm_pos + sm_kw.len()..];
    let mut depth = 0i32;
    for ch in after_sm.chars() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth > 0
}

/// Find the last occurrence of a whole keyword in `text`.
fn rfind_keyword(text: &str, kw: &str) -> Option<usize> {
    let mut search = text;
    let mut base = 0usize;
    let mut last_found: Option<usize> = None;
    while let Some(pos) = search.find(kw) {
        let abs = base + pos;
        let before_ok = abs == 0 || {
            let b = text.as_bytes()[abs - 1];
            !b.is_ascii_alphanumeric() && b != b'_'
        };
        let after = abs + kw.len();
        // `state machine` ends with 'e', next char could be space — just check alpha/underscore
        let after_ok = after >= text.len() || {
            let b = text.as_bytes()[after];
            !b.is_ascii_alphanumeric() && b != b'_'
        };
        if before_ok && after_ok {
            last_found = Some(abs);
        }
        base += pos + 1;
        search = &text[pos + 1..];
    }
    last_found
}

// ── User-defined type collection ──

/// Collect user-defined type names from the semantic IR (if parse+analysis succeeds)
/// or fall back to a lightweight scan of the raw source.
fn collect_user_types(source: &str) -> Vec<(String, CompletionItemKind)> {
    // Try the full pipeline first.
    if let Ok(ast) = wirespec_syntax::parse(source)
        && let Ok(module) = wirespec_sema::analyze(
            &ast,
            wirespec_sema::ComplianceProfile::default(),
            &Default::default(),
        )
    {
        let mut types = Vec::new();
        for p in &module.packets {
            types.push((p.name.clone(), CompletionItemKind::STRUCT));
        }
        for e in &module.enums {
            types.push((e.name.clone(), CompletionItemKind::ENUM));
        }
        for f in &module.frames {
            types.push((f.name.clone(), CompletionItemKind::STRUCT));
        }
        for v in &module.varints {
            types.push((v.name.clone(), CompletionItemKind::CLASS));
        }
        for c in &module.capsules {
            types.push((c.name.clone(), CompletionItemKind::STRUCT));
        }
        return types;
    }

    // Fallback: scan raw source for `packet Name`, `enum Name`, etc.
    let mut types = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        for kw in &["packet", "frame", "capsule"] {
            if let Some(rest) = trimmed.strip_prefix(kw)
                && let Some(name) = extract_ident(rest)
            {
                types.push((name, CompletionItemKind::STRUCT));
            }
        }
        for kw in &["enum", "flags"] {
            if let Some(rest) = trimmed.strip_prefix(kw)
                && let Some(name) = extract_ident(rest)
            {
                types.push((name, CompletionItemKind::ENUM));
            }
        }
        if let Some(rest) = trimmed.strip_prefix("type")
            && let Some(name) = extract_ident(rest)
        {
            types.push((name, CompletionItemKind::CLASS));
        }
    }
    types
}

fn extract_ident(s: &str) -> Option<String> {
    let s = s.trim_start();
    if s.is_empty() || !s.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let end = s
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(s.len());
    if end == 0 {
        None
    } else {
        Some(s[..end].to_owned())
    }
}

// ── CompletionItem builders ──

fn keyword_item(label: &str, snippet: Option<&str>) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        insert_text: snippet.map(str::to_owned),
        insert_text_format: if snippet.is_some() {
            Some(InsertTextFormat::SNIPPET)
        } else {
            None
        },
        ..Default::default()
    }
}

fn type_item(label: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..Default::default()
    }
}

fn annotation_item(label: &str, snippet: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::FUNCTION),
        insert_text: Some(snippet.to_owned()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    }
}

fn user_type_item(label: String, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label,
        kind: Some(kind),
        ..Default::default()
    }
}

// ── Public entry point ──

pub fn compute_completions(source: &str, position: Position) -> Vec<CompletionItem> {
    let offset = position_to_offset(source, position);
    let context = detect_context(source, offset);

    match context {
        CompletionContext::Annotation => ANNOTATION_NAMES
            .iter()
            .map(|(label, snippet)| annotation_item(label, snippet))
            .collect(),

        CompletionContext::Type => {
            let mut items: Vec<CompletionItem> =
                TYPE_KEYWORDS.iter().map(|k| type_item(k)).collect();
            // Add user-defined types
            for (name, kind) in collect_user_types(source) {
                items.push(user_type_item(name, kind));
            }
            items
        }

        CompletionContext::Encoding => ENCODING_NAMES
            .iter()
            .map(|name| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::VALUE),
                ..Default::default()
            })
            .collect(),

        CompletionContext::StateMachine => {
            SM_KEYWORDS.iter().map(|k| keyword_item(k, None)).collect()
        }

        CompletionContext::TopLevel => TOP_LEVEL_KEYWORDS
            .iter()
            .map(|(label, snippet)| keyword_item(label, Some(snippet)))
            .collect(),
    }
}
