use tower_lsp::lsp_types::*;

use crate::position::offset_to_position;

pub fn compute_diagnostics(
    text: &str,
) -> (Option<wirespec_syntax::ast::AstModule>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();

    let ast = match wirespec_syntax::parse(text) {
        Ok(ast) => ast,
        Err(e) => {
            let range = span_to_range(text, e.span.as_ref());
            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("wirespec".into()),
                message: e.msg,
                ..Default::default()
            });
            return (None, diagnostics);
        }
    };

    match wirespec_sema::analyze(
        &ast,
        wirespec_sema::ComplianceProfile::default(),
        &Default::default(),
    ) {
        Ok(sem) => {
            for warning in &sem.warnings {
                let range = span_to_range(text, warning.span.as_ref());
                diagnostics.push(Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    source: Some("wirespec".into()),
                    message: warning.msg.clone(),
                    ..Default::default()
                });
            }
        }
        Err(e) => {
            let range = span_to_range(text, e.span.as_ref());
            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("wirespec".into()),
                message: e.msg,
                ..Default::default()
            });
        }
    }

    (Some(ast), diagnostics)
}

fn span_to_range(text: &str, span: Option<&wirespec_syntax::span::Span>) -> Range {
    if let Some(span) = span {
        let start = offset_to_position(text, span.offset as usize);
        let end = Position::new(start.line, start.character + span.len.max(1));
        Range::new(start, end)
    } else {
        Range::new(Position::new(0, 0), Position::new(0, 0))
    }
}
