// crates/wirespec-sema/src/error.rs
use wirespec_syntax::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    UndefinedType,
    UndefinedState,
    ForwardReference,
    TypeMismatch,
    RemainingNotLast,
    DuplicateDefinition,
    InvalidChecksumType,
    DuplicateChecksum,
    ChecksumProfileViolation,
    InvalidLengthOrRemaining,
    InvalidArrayCount,
    InvalidBytesLength,
    SmUndefinedState,
    SmInvalidInitial,
    SmUnhandledEvent,
    SmDuplicateTransition,
    SmMissingAssignment,
    SmDelegateNotSelfTransition,
    SmDelegateWithAction,
    SmTerminalHasOutgoing,
    CyclicDependency,
    ReservedIdentifier,
    UndefinedAsn1Type,
    UnsupportedAsn1Encoding,
    InvalidEnumUnderlying,
}

#[derive(Debug, Clone)]
pub struct SemaError {
    pub kind: ErrorKind,
    pub msg: String,
    pub span: Option<Span>,
    pub context: Vec<String>,
    pub hint: Option<String>,
}

impl SemaError {
    pub fn new(kind: ErrorKind, msg: impl Into<String>) -> Self {
        Self {
            kind,
            msg: msg.into(),
            span: None,
            context: Vec::new(),
            hint: None,
        }
    }

    pub fn with_span(mut self, span: Option<Span>) -> Self {
        self.span = span;
        self
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context.push(ctx.into());
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl std::fmt::Display for SemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(span) = &self.span {
            write!(f, "error at offset {}: ", span.offset)?;
        }
        write!(f, "{}", self.msg)?;
        for ctx in &self.context {
            write!(f, "\n  in {ctx}")?;
        }
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {hint}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SemaError {}

pub type SemaResult<T> = Result<T, SemaError>;

// ── Megaparsec-inspired error display ──

/// Format an error with source context, matching Python M21 quality.
///
/// Output format:
/// ```text
/// error: undefined type 'Varnt'
///  --> file.wspec:42:15
///   |
/// 42 | packet Foo { x: Varnt }
///   |                  ^^^^^
///   = in packet 'Foo'
///   hint: did you mean 'VarInt'?
/// ```
pub fn format_error(error: &SemaError, source: &str, filename: &str) -> String {
    let mut out = String::new();

    // Error header
    out.push_str(&format!("error: {}\n", error.msg));

    // Source location
    if let Some(span) = &error.span {
        let (line, col) = offset_to_line_col(source, span.offset as usize);
        out.push_str(&format!(" --> {}:{}:{}\n", filename, line, col));

        // Source line with caret
        if let Some(source_line) = get_source_line(source, line) {
            let line_str = format!("{}", line);
            let gutter_width = line_str.len();

            out.push_str(&format!("{:>width$} |\n", "", width = gutter_width));
            out.push_str(&format!("{} | {}\n", line_str, source_line));

            // Caret pointer
            let caret_len = (span.len as usize).max(1);
            let padding = col.saturating_sub(1);
            out.push_str(&format!(
                "{:>width$} | {:>pad$}{}\n",
                "",
                "",
                "^".repeat(caret_len),
                width = gutter_width,
                pad = padding
            ));
        }
    }

    // Context stack
    for ctx in &error.context {
        out.push_str(&format!("  = in {}\n", ctx));
    }

    // Hint
    if let Some(hint) = &error.hint {
        out.push_str(&format!("  hint: {}\n", hint));
    }

    out
}

/// Simple error formatting without span — just show the error with filename.
pub fn format_error_simple(msg: &str, _source: &str, filename: &str) -> String {
    format!("error: {}\n --> {}\n", msg, filename)
}

/// Convert byte offset to 1-indexed (line, column).
pub fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let col = match before.rfind('\n') {
        Some(nl) => offset - nl,
        None => offset + 1,
    };
    (line, col)
}

/// Get the source line at 1-indexed line number.
pub fn get_source_line(source: &str, line: usize) -> Option<&str> {
    source.lines().nth(line - 1)
}

// ── Levenshtein distance + "did you mean?" suggestions ──

/// Compute Levenshtein edit distance between two strings.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Suggest the most similar name from a list of candidates.
pub fn suggest_similar<'a>(
    name: &str,
    candidates: &[&'a str],
    max_distance: usize,
) -> Option<&'a str> {
    candidates
        .iter()
        .map(|c| (*c, levenshtein(name, c)))
        .filter(|(_, d)| *d <= max_distance && *d > 0)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c)
}
