//! plc-lsp — LSP server for palace notes.
//!
//! Speaks LSP over stdin/stdout. Read-side features (definition,
//! references, completion, hover) and write-side features (create,
//! delete, link, rename via WorkspaceEdit) all flow through a single
//! `Backend` that delegates graph logic to `plc-core`.

mod index;
mod planner;
mod server;

use tower_lsp::{LspService, Server};

use crate::server::Backend;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) =
        LspService::new(|client| Backend::new(client));

    Server::new(stdin, stdout, socket).serve(service).await;
}
