use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct Backend {
    client: Client,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self { client }
    }

    async fn diagnose(&self, uri: Url, text: &str) {
        let diagnostics = self.compute_diagnostics(text);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    fn compute_diagnostics(&self, text: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Parse
        let ast = match wirespec_syntax::parse(text) {
            Ok(ast) => ast,
            Err(e) => {
                let (position, end_position) = if let Some(span) = &e.span {
                    let (line, col) =
                        wirespec_sema::error::offset_to_line_col(text, span.offset as usize);
                    let pos = Position::new((line - 1) as u32, (col - 1) as u32);
                    let end = Position::new(
                        (line - 1) as u32,
                        (col - 1 + (span.len as usize).max(1)) as u32,
                    );
                    (pos, end)
                } else {
                    let pos = Position::new(0, 0);
                    (pos, pos)
                };
                diagnostics.push(Diagnostic {
                    range: Range::new(position, end_position),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("wirespec".into()),
                    message: e.msg,
                    ..Default::default()
                });
                return diagnostics;
            }
        };

        // Semantic analysis
        match wirespec_sema::analyze(
            &ast,
            wirespec_sema::ComplianceProfile::default(),
            &Default::default(),
        ) {
            Ok(_) => {}
            Err(e) => {
                let (position, end_position) = if let Some(span) = &e.span {
                    let (line, col) =
                        wirespec_sema::error::offset_to_line_col(text, span.offset as usize);
                    let pos = Position::new((line - 1) as u32, (col - 1) as u32);
                    let end =
                        Position::new((line - 1) as u32, (col - 1 + span.len as usize) as u32);
                    (pos, end)
                } else {
                    let pos = Position::new(0, 0);
                    (pos, pos)
                };
                diagnostics.push(Diagnostic {
                    range: Range::new(position, end_position),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("wirespec".into()),
                    message: e.msg,
                    ..Default::default()
                });
            }
        }

        diagnostics
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "wirespec LSP initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.diagnose(params.text_document.uri, &params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.diagnose(params.text_document.uri, &change.text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        // Clear diagnostics on close
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
