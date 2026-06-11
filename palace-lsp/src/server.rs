//! `Backend` is the LSP entry point. Every handler is intentionally
//! thin: parse params, ask the index/planner for a result, translate
//! back into LSP types. All real logic lives in `palace-core` (graph)
//! and `planner` (intent -> WorkspaceEdit).

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::index::Index;
use crate::planner;

pub struct Backend {
    pub client: Client,
    pub index: Index,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self { client, index: Index::new() }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(
        &self,
        _p: InitializeParams,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "palace-lsp".into(),
                version: Some(
                    env!("CARGO_PKG_VERSION").into(),
                ),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(
                    TextDocumentSyncCapability::Kind(
                        TextDocumentSyncKind::FULL,
                    ),
                ),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(
                    CodeActionProviderCapability::Simple(true),
                ),
                execute_command_provider: Some(
                    ExecuteCommandOptions {
                        commands: vec![
                            "palace.createNote".into(),
                            "palace.deleteNote".into(),
                            "palace.linkNotes".into(),
                            "palace.unlinkNotes".into(),
                        ],
                        ..Default::default()
                    },
                ),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["[".into()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(
                MessageType::INFO,
                "palace-lsp ready",
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    // ---------- document sync ----------

    async fn did_open(&self, p: DidOpenTextDocumentParams) {
        self.index
            .open(&p.text_document.uri, p.text_document.text);
    }

    async fn did_change(
        &self,
        p: DidChangeTextDocumentParams,
    ) {
        // FULL sync: the last change carries the whole buffer.
        if let Some(change) = p.content_changes.into_iter().last()
        {
            self.index.update(&p.text_document.uri, change.text);
        }
    }

    async fn did_close(&self, p: DidCloseTextDocumentParams) {
        self.index.close(&p.text_document.uri);
    }

    // ---------- read-side stubs ----------

    async fn goto_definition(
        &self,
        _p: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(None)
    }

    async fn references(
        &self,
        _p: ReferenceParams,
    ) -> Result<Option<Vec<Location>>> {
        Ok(None)
    }

    async fn completion(
        &self,
        _p: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        Ok(None)
    }

    async fn rename(
        &self,
        _p: RenameParams,
    ) -> Result<Option<WorkspaceEdit>> {
        Ok(None)
    }

    async fn code_action(
        &self,
        _p: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        Ok(Some(vec![]))
    }

    // ---------- write-side: command dispatch ----------

    async fn execute_command(
        &self,
        p: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        let edit = match p.command.as_str() {
            "palace.createNote" => {
                planner::plan_create_note(&self.index, &p.arguments)
            }
            "palace.deleteNote" => {
                planner::plan_delete_note(&self.index, &p.arguments)
            }
            "palace.linkNotes" => {
                planner::plan_link(&self.index, &p.arguments)
            }
            "palace.unlinkNotes" => {
                planner::plan_unlink(&self.index, &p.arguments)
            }
            other => {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("unknown command: {other}"),
                    )
                    .await;
                return Ok(None);
            }
        };

        if let Some(edit) = edit {
            // Ask the client to apply. The resulting did_change
            // events will converge the graph — one writer.
            let _ = self.client.apply_edit(edit).await;
        }
        Ok(None)
    }
}
