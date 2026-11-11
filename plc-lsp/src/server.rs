//! `Backend` translates LSP requests into questions for the [`Vault`]. It
//! owns when the vault is built (once, on first need), patched (buffers,
//! watched files) and rebuilt (a `gd` miss), and nothing else.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

use dashmap::DashMap;
use plc_core::settings::Kind;
use plc_core::{fold_name, links, normalize_target, vault::palace_dir};
use serde_json::Value;
use tokio::sync::OnceCell;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::pos::{Encoding, LineIndex};
use crate::vault::{find_anchor, line_location, Target, Vault};

const WATCH_ID: &str = "plc-watch";
const PREVIEW_LINES: usize = 12;

struct Setup {
    root: Option<PathBuf>,
    enc: Encoding,
    watch: bool,
}

pub struct Backend {
    client: Client,
    setup: OnceLock<Setup>,
    vault: OnceCell<RwLock<Vault>>,
    /// Open buffers, by URI. Their text wins over what is on disk.
    docs: DashMap<Url, String>,
    /// Set once the client accepted our file watchers.
    watching: AtomicBool,
}

/// `path` with symlinks resolved, so a vault reached through one (or
/// macOS's `/tmp` → `/private/tmp`) still matches. A deleted file resolves
/// through its parent.
fn real(path: &Path) -> PathBuf {
    if let Ok(p) = fs::canonicalize(path) {
        return p;
    }
    match (path.parent().map(fs::canonicalize), path.file_name()) {
        (Some(Ok(dir)), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

fn file_path(uri: &Url) -> Option<PathBuf> {
    uri.to_file_path().ok().map(|p| real(&p))
}

fn workspace_root(p: &InitializeParams) -> Option<PathBuf> {
    let folder = p.workspace_folders.as_ref().and_then(|f| f.first());
    #[allow(deprecated)]
    let uri = folder.map(|f| &f.uri).or(p.root_uri.as_ref())?;
    uri.to_file_path().ok()
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            setup: OnceLock::new(),
            vault: OnceCell::new(),
            docs: DashMap::new(),
            watching: AtomicBool::new(false),
        }
    }

    fn enc(&self) -> Encoding {
        self.setup.get().map_or(Encoding::Utf16, |s| s.enc)
    }

    /// The vault, built on first use; `None` when no vault was found.
    async fn vault(&self) -> Option<&RwLock<Vault>> {
        let setup = self.setup.get()?;
        let root = setup.root.clone()?;
        let enc = setup.enc;
        let vault = self
            .vault
            .get_or_init(|| async move {
                let load = move || Vault::load(&root, enc);
                let vault = tokio::task::spawn_blocking(load).await;
                RwLock::new(vault.expect("vault load panicked"))
            })
            .await;
        Some(vault)
    }

    /// Rebuild from disk, then lay the open buffers back over it.
    async fn rescan(&self) {
        let Some(lock) = self.vault().await else {
            return;
        };
        let (root, enc) = {
            let v = lock.read().unwrap();
            (v.root().to_path_buf(), v.encoding())
        };
        let load = move || Vault::load(&root, enc);
        let Ok(mut fresh) = tokio::task::spawn_blocking(load).await else {
            return;
        };
        for doc in self.docs.iter() {
            if let Some(path) = file_path(doc.key()) {
                if fresh.is_note(&path) {
                    fresh.upsert(&path, doc.value());
                }
            }
        }
        *lock.write().unwrap() = fresh;
    }

    /// The text of `path`: its open buffer, else the file.
    fn text_of(&self, path: &Path) -> Option<String> {
        let uri = Url::from_file_path(path).ok()?;
        if let Some(doc) = self.docs.get(&uri) {
            return Some(doc.clone());
        }
        fs::read_to_string(path).ok()
    }

    /// Re-index `uri` from its buffer (if open) or from disk.
    async fn refresh(&self, uri: &Url) {
        let Some(path) = file_path(uri) else { return };
        let Some(lock) = self.vault().await else {
            return;
        };
        let mut v = lock.write().unwrap();
        if !v.is_note(&path) {
            return;
        }
        let text = self.docs.get(uri).map(|d| d.clone());
        match text.or_else(|| fs::read_to_string(&path).ok()) {
            Some(text) => v.upsert(&path, &text),
            None => v.remove(&path),
        }
    }

    /// The document text at `uri` and the byte offset of `pos` in it.
    fn locate(&self, uri: &Url, pos: Position) -> Option<(String, usize)> {
        let text = match self.docs.get(uri) {
            Some(doc) => doc.clone(),
            None => fs::read_to_string(uri.to_file_path().ok()?).ok()?,
        };
        let offset = LineIndex::new(&text, self.enc()).offset(pos);
        Some((text, offset))
    }

    fn definitions(
        &self,
        v: &Vault,
        inner: &str,
        from: &Path,
    ) -> Vec<Location> {
        match v.resolve(inner, from) {
            Target::Notes(notes) => notes
                .into_iter()
                .filter_map(|(path, anchor)| {
                    let line = anchor
                        .and_then(|a| find_anchor(&self.text_of(path)?, &a));
                    line_location(path, line.unwrap_or(0))
                })
                .collect(),
            Target::Declaration(d) => {
                line_location(&v.config_path(), d.line as u32)
                    .into_iter()
                    .collect()
            }
            Target::Missing => Vec::new(),
        }
    }

    async fn register_watchers(&self) {
        let watchers = ["**/*.md", "**/.plc/config"]
            .into_iter()
            .map(|glob| FileSystemWatcher {
                glob_pattern: GlobPattern::String(glob.into()),
                kind: None,
            })
            .collect();
        let options = DidChangeWatchedFilesRegistrationOptions { watchers };
        let registration = Registration {
            id: WATCH_ID.into(),
            method: "workspace/didChangeWatchedFiles".into(),
            register_options: serde_json::to_value(options).ok(),
        };
        let registered =
            self.client.register_capability(vec![registration]).await;
        match registered {
            Ok(()) => self.watching.store(true, Ordering::Relaxed),
            Err(e) => {
                let msg = format!("plc-lsp: file watching unavailable: {e}");
                self.client.log_message(MessageType::WARNING, msg).await;
            }
        }
    }
}

fn completion_items(
    v: &Vault,
    range: Range,
    close: &str,
) -> Vec<CompletionItem> {
    let edit = |text: &str| {
        Some(CompletionTextEdit::Edit(TextEdit::new(
            range,
            format!("{text}{close}"),
        )))
    };
    let notes = v.notes().map(|(path, note)| {
        let rel = path.strip_prefix(v.root()).unwrap_or(path);
        CompletionItem {
            label: note.stem.clone(),
            kind: Some(CompletionItemKind::FILE),
            detail: Some(rel.display().to_string()),
            text_edit: edit(&note.stem),
            data: Some(Value::String(path.display().to_string())),
            ..Default::default()
        }
    });
    let ledger = v.ledger().iter().map(|d| {
        let (kind, detail) = match d.kind {
            Kind::Account => (CompletionItemKind::ENUM_MEMBER, "account"),
            Kind::Category => (CompletionItemKind::VALUE, "category"),
        };
        CompletionItem {
            label: d.name.clone(),
            kind: Some(kind),
            detail: Some(detail.into()),
            text_edit: edit(&d.name),
            ..Default::default()
        }
    });
    notes.chain(ledger).collect()
}

/// The open `[[` before `col` in `line`: the byte where the typed name
/// starts. `None` once the link is closed or has reached its alias/anchor.
fn open_link(line: &str, col: usize) -> Option<usize> {
    let before = &line[..col];
    let start = before.rfind("[[")? + 2;
    let typed = &before[start..];
    (!typed.contains([']', '|', '#', '^'])).then_some(start)
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(
        &self,
        p: InitializeParams,
    ) -> Result<InitializeResult> {
        let general = p.capabilities.general.as_ref();
        let offered = general.and_then(|g| g.position_encodings.as_deref());
        let enc = Encoding::negotiate(offered);
        let watch = p
            .capabilities
            .workspace
            .as_ref()
            .and_then(|w| w.did_change_watched_files)
            .and_then(|w| w.dynamic_registration)
            .unwrap_or(false);
        let root = palace_dir()
            .ok()
            .filter(|d| d.is_dir())
            .or_else(|| workspace_root(&p))
            .map(|d| real(&d));
        let _ = self.setup.set(Setup { root, enc, watch });

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "plc-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                position_encoding: Some(enc.kind()),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["[".into()]),
                    resolve_provider: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        if self.vault().await.is_none() {
            let msg = "plc-lsp: no vault (PALACE_DIR, .plcrc or workspace)";
            self.client.log_message(MessageType::WARNING, msg).await;
            return;
        }
        if self.setup.get().is_some_and(|s| s.watch) {
            self.register_watchers().await;
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, p: DidOpenTextDocumentParams) {
        let uri = p.text_document.uri;
        self.docs.insert(uri.clone(), p.text_document.text);
        self.refresh(&uri).await;
    }

    async fn did_change(&self, p: DidChangeTextDocumentParams) {
        let uri = p.text_document.uri;
        let Some(change) = p.content_changes.into_iter().last() else {
            return;
        };
        self.docs.insert(uri.clone(), change.text);
        self.refresh(&uri).await;
    }

    async fn did_close(&self, p: DidCloseTextDocumentParams) {
        let uri = p.text_document.uri;
        self.docs.remove(&uri);
        self.refresh(&uri).await;
    }

    async fn did_change_watched_files(&self, p: DidChangeWatchedFilesParams) {
        let Some(lock) = self.vault().await else {
            return;
        };
        let config = lock.read().unwrap().config_path();
        for change in p.changes {
            if file_path(&change.uri).as_ref() == Some(&config) {
                lock.write().unwrap().reload_ledger();
            } else if !self.docs.contains_key(&change.uri) {
                self.refresh(&change.uri).await;
            }
        }
    }

    async fn goto_definition(
        &self,
        p: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let at = p.text_document_position_params;
        let Some((text, offset)) =
            self.locate(&at.text_document.uri, at.position)
        else {
            return Ok(None);
        };
        let Some(link) = links(&text).find(|l| l.range.contains(&offset))
        else {
            return Ok(None);
        };
        let (Some(from), Some(lock)) =
            (file_path(&at.text_document.uri), self.vault().await)
        else {
            return Ok(None);
        };
        let mut found =
            self.definitions(&lock.read().unwrap(), link.inner, &from);
        // A tag like [[ledger]] never resolves; with file events flowing
        // the index is current, so only a blind server pays for a rescan.
        if found.is_empty() && !self.watching.load(Ordering::Relaxed) {
            self.rescan().await;
            found = self.definitions(&lock.read().unwrap(), link.inner, &from);
        }
        Ok((!found.is_empty()).then_some(GotoDefinitionResponse::Array(found)))
    }

    async fn references(
        &self,
        p: ReferenceParams,
    ) -> Result<Option<Vec<Location>>> {
        let at = p.text_document_position;
        let uri = &at.text_document.uri;
        let (Some((text, offset)), Some(from), Some(lock)) = (
            self.locate(uri, at.position),
            file_path(uri),
            self.vault().await,
        ) else {
            return Ok(None);
        };
        let on_link = links(&text)
            .find(|l| l.range.contains(&offset))
            .map(|l| normalize_target(l.inner))
            .filter(|t| !t.is_empty());
        let Some(target) = on_link.or_else(|| {
            let name = from.file_name()?.to_str()?.strip_suffix(".md")?;
            Some(fold_name(name))
        }) else {
            return Ok(None);
        };
        let v = lock.read().unwrap();
        let mut found = Vec::new();
        if p.context.include_declaration {
            let notes = v.named(&target).iter();
            found.extend(notes.filter_map(|path| line_location(path, 0)));
        }
        found.extend(v.references(&target));
        Ok(Some(found))
    }

    async fn completion(
        &self,
        p: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let at = p.text_document_position;
        let Some((text, _)) = self.locate(&at.text_document.uri, at.position)
        else {
            return Ok(None);
        };
        let idx = LineIndex::new(&text, self.enc());
        let line = idx.line(at.position.line as usize);
        let line_start = idx.offset(Position::new(at.position.line, 0));
        let col = idx.offset(at.position) - line_start;
        let Some(start) = open_link(line, col) else {
            return Ok(None);
        };
        let Some(lock) = self.vault().await else {
            return Ok(None);
        };
        let range = Range::new(idx.position(line_start + start), at.position);
        let close = if line[col..].starts_with("]]") {
            ""
        } else {
            "]]"
        };
        let items = completion_items(&lock.read().unwrap(), range, close);
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn completion_resolve(
        &self,
        mut item: CompletionItem,
    ) -> Result<CompletionItem> {
        let Some(Value::String(path)) = &item.data else {
            return Ok(item);
        };
        if let Some(text) = self.text_of(Path::new(path)) {
            let head: Vec<&str> = text.lines().take(PREVIEW_LINES).collect();
            item.documentation =
                Some(Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: head.join("\n"),
                }));
        }
        Ok(item)
    }
}

#[cfg(test)]
mod tests {
    use super::open_link;

    #[test]
    fn open_link_finds_the_typed_name() {
        assert_eq!(open_link("see [[да", 10), Some(6));
        assert_eq!(open_link("[[", 2), Some(2));
        assert_eq!(open_link("[[a]] b", 7), None);
        assert_eq!(open_link("[[a|al", 6), None);
        assert_eq!(open_link("[[a]] [[b", 9), Some(8));
        assert_eq!(open_link("no link", 7), None);
    }
}
