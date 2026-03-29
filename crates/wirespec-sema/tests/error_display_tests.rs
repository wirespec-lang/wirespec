use wirespec_sema::error::*;
use wirespec_syntax::span::Span;

#[test]
fn format_error_with_source_line() {
    let source = "packet Foo {\n    x: Unknown,\n}";
    // "Unknown" starts at byte offset 20, length 7
    let err = SemaError::new(ErrorKind::UndefinedType, "undefined type 'Unknown'")
        .with_span(Some(Span::new(20, 7)))
        .with_context("packet 'Foo'");

    let formatted = format_error(&err, source, "test.wspec");
    assert!(
        formatted.contains("error: undefined type 'Unknown'"),
        "formatted = {formatted}"
    );
    assert!(
        formatted.contains("--> test.wspec:2:"),
        "formatted = {formatted}"
    );
    assert!(formatted.contains("x: Unknown"), "formatted = {formatted}");
    assert!(formatted.contains("^^^^^^^"), "formatted = {formatted}");
    assert!(
        formatted.contains("in packet 'Foo'"),
        "formatted = {formatted}"
    );
}

#[test]
fn format_error_line_col_computation() {
    assert_eq!(offset_to_line_col("hello\nworld", 0), (1, 1));
    assert_eq!(offset_to_line_col("hello\nworld", 5), (1, 6));
    assert_eq!(offset_to_line_col("hello\nworld", 6), (2, 1));
    assert_eq!(offset_to_line_col("hello\nworld", 9), (2, 4));
}

#[test]
fn levenshtein_basic() {
    assert_eq!(levenshtein("kitten", "sitting"), 3);
    assert_eq!(levenshtein("", "abc"), 3);
    assert_eq!(levenshtein("abc", "abc"), 0);
    assert_eq!(levenshtein("VarInt", "Varnt"), 1);
}

#[test]
fn suggest_similar_finds_match() {
    let candidates = &["VarInt", "UdpDatagram", "TcpSegment"];
    assert_eq!(suggest_similar("Varnt", candidates, 2), Some("VarInt"));
    assert_eq!(suggest_similar("VarInt", candidates, 2), None); // exact match excluded
    assert_eq!(suggest_similar("XyzAbc", candidates, 2), None); // too different
}

#[test]
fn suggest_similar_in_error() {
    let source = "packet P { x: Varnt }";
    let ast = wirespec_syntax::parse(source).unwrap();
    let result = wirespec_sema::analyze(
        &ast,
        wirespec_sema::ComplianceProfile::default(),
        &Default::default(),
    );
    // Should fail with UndefinedType (no VarInt defined)
    assert!(result.is_err(), "expected error for undefined type");
}

#[test]
fn format_error_no_span() {
    let err = SemaError::new(ErrorKind::DuplicateDefinition, "duplicate 'Foo'");
    let formatted = format_error(&err, "", "test.wspec");
    assert!(
        formatted.contains("error: duplicate 'Foo'"),
        "formatted = {formatted}"
    );
    // No source location should appear
    assert!(!formatted.contains("-->"), "formatted = {formatted}");
}

#[test]
fn format_error_with_hint() {
    let err = SemaError::new(ErrorKind::UndefinedType, "undefined type 'Varnt'")
        .with_hint("did you mean 'VarInt'?");
    let formatted = format_error(&err, "packet P { x: Varnt }", "test.wspec");
    assert!(
        formatted.contains("hint: did you mean 'VarInt'?"),
        "formatted = {formatted}"
    );
}
