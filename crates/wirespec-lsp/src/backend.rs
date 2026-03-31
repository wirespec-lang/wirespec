use std::collections::HashMap;
use std::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

pub struct Backend {
    pub client: Client,
    pub documents: Mutex<HashMap<Url, String>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
        }
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
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: crate::semantic_tokens::legend(),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            ..Default::default()
                        },
                    ),
                ),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![":".into(), "@".into(), " ".into()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
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
        let uri = params.text_document.uri.clone();
        let text = params.text_document.text.clone();
        self.documents
            .lock()
            .unwrap()
            .insert(uri.clone(), text.clone());
        let (_ast, diags) = crate::diagnostics::compute_diagnostics(&text);
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            let uri = params.text_document.uri.clone();
            self.documents
                .lock()
                .unwrap()
                .insert(uri.clone(), change.text.clone());
            let (_ast, diags) = crate::diagnostics::compute_diagnostics(&change.text);
            self.client.publish_diagnostics(uri, diags, None).await;
        }
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let docs = self.documents.lock().unwrap();
        let Some(source) = docs.get(&uri) else {
            return Ok(None);
        };
        let source = source.clone();
        drop(docs);

        let Ok(ast) = wirespec_syntax::parse(&source) else {
            return Ok(None);
        };
        let tokens = crate::semantic_tokens::compute_semantic_tokens(&source, &ast);
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let docs = self.documents.lock().unwrap();
        let Some(source) = docs.get(&uri) else {
            return Ok(None);
        };
        let source = source.clone();
        drop(docs);
        let items = crate::completion::compute_completions(&source, position);
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let docs = self.documents.lock().unwrap();
        let Some(source) = docs.get(&uri) else {
            return Ok(None);
        };
        let source = source.clone();
        drop(docs);
        Ok(crate::hover::compute_hover(&source, position))
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .lock()
            .unwrap()
            .remove(&params.text_document.uri);
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }
}
