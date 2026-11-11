//! plc-lsp — LSP server for palace notes.
//!
//! Speaks LSP over stdin/stdout: go-to-definition, references and `[[`
//! completion over the vault's `[[wikilinks]]`, with link semantics from
//! `plc-core`.

mod pos;
mod server;
mod vault;

use tower_lsp::{LspService, Server};

use crate::server::Backend;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);

    Server::new(stdin, stdout, socket).serve(service).await;
}
