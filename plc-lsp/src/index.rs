//! In-memory state owned by the LSP: open document buffers and (later)
//! the resolved note graph. Read by every handler; mutated only by the
//! document-sync handlers and by `apply_edit` round-trips.

use dashmap::DashMap;
use tower_lsp::lsp_types::Url;

pub struct Index {
    /// URI -> current buffer text. Full-resync model for now; swap
    /// to ropey + incremental sync once we need it.
    docs: DashMap<Url, String>,
}

impl Index {
    pub fn new() -> Self {
        Self { docs: DashMap::new() }
    }

    pub fn open(&self, uri: &Url, text: String) {
        self.docs.insert(uri.clone(), text);
    }

    pub fn update(&self, uri: &Url, text: String) {
        self.docs.insert(uri.clone(), text);
    }

    pub fn close(&self, uri: &Url) {
        self.docs.remove(uri);
    }

    pub fn get(&self, uri: &Url) -> Option<String> {
        self.docs.get(uri).map(|r| r.value().clone())
    }
}

impl Default for Index {
    fn default() -> Self {
        Self::new()
    }
}
