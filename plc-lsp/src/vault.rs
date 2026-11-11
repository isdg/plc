//! The vault as the server sees it: every note's name and outbound links,
//! plus the ledger names declared in `.plc/config`. Pure and synchronous;
//! the server decides when to load, patch or rebuild it.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use plc_core::ledger::normalize_name;
use plc_core::settings::{self, Declaration, Settings};
use plc_core::{fold_name, links, normalize_target, notes};
use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::pos::{Encoding, LineIndex};

/// An outbound link: where it sits and the folded name it points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkRef {
    pub range: Range,
    pub target: String,
}

#[derive(Debug)]
pub struct Note {
    /// The file name without `.md`, spelled as on disk.
    pub stem: String,
    pub links: Vec<LinkRef>,
}

pub struct Vault {
    root: PathBuf,
    enc: Encoding,
    notes: BTreeMap<PathBuf, Note>,
    names: HashMap<String, Vec<PathBuf>>,
    ledger: Vec<Declaration>,
}

/// Where a link sends `gd`.
#[derive(Debug, PartialEq, Eq)]
pub enum Target<'a> {
    Notes(Vec<(&'a Path, Option<String>)>),
    Declaration(&'a Declaration),
    Missing,
}

impl Vault {
    /// Walk `root` for notes, skipping hidden directories, and read the
    /// ledger config. Unreadable or non-UTF-8 files are left out.
    pub fn load(root: &Path, enc: Encoding) -> Self {
        let mut vault = Vault {
            root: root.to_path_buf(),
            enc,
            notes: BTreeMap::new(),
            names: HashMap::new(),
            ledger: Vec::new(),
        };
        for path in notes::walk(root) {
            if let Ok(text) = fs::read_to_string(&path) {
                vault.upsert(&path, &text);
            }
        }
        vault.reload_ledger();
        vault
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn encoding(&self) -> Encoding {
        self.enc
    }

    pub fn config_path(&self) -> PathBuf {
        Settings::path(&self.root)
    }

    /// Whether `path` is a note this vault indexes: a `.md` file under the
    /// root, outside any hidden directory.
    pub fn is_note(&self, path: &Path) -> bool {
        notes::is_note(&self.root, path)
    }

    pub fn upsert(&mut self, path: &Path, text: &str) {
        let Some(stem) = notes::stem(path) else { return };
        let idx = LineIndex::new(text, self.enc);
        let links = links(text)
            .map(|l| LinkRef {
                range: Range::new(
                    idx.position(l.range.start),
                    idx.position(l.range.end),
                ),
                target: normalize_target(l.inner),
            })
            .collect();
        let note = Note {
            stem: stem.to_string(),
            links,
        };
        if self.notes.insert(path.to_path_buf(), note).is_none() {
            let paths = self.names.entry(fold_name(stem)).or_default();
            let at = paths.binary_search_by(|p| p.as_path().cmp(path));
            if let Err(at) = at {
                paths.insert(at, path.to_path_buf());
            }
        }
    }

    pub fn remove(&mut self, path: &Path) {
        let Some(note) = self.notes.remove(path) else {
            return;
        };
        let key = fold_name(&note.stem);
        if let Some(paths) = self.names.get_mut(&key) {
            paths.retain(|p| p != path);
            if paths.is_empty() {
                self.names.remove(&key);
            }
        }
    }

    pub fn reload_ledger(&mut self) {
        self.ledger = fs::read_to_string(self.config_path())
            .map(|text| settings::declarations(&text))
            .unwrap_or_default();
    }

    pub fn notes(&self) -> impl Iterator<Item = (&Path, &Note)> {
        self.notes.iter().map(|(p, n)| (p.as_path(), n))
    }

    pub fn ledger(&self) -> &[Declaration] {
        &self.ledger
    }

    /// The notes named `target` (already folded), in path order.
    pub fn named(&self, target: &str) -> &[PathBuf] {
        self.names.get(target).map_or(&[], Vec::as_slice)
    }

    /// Resolve a link's inner text written in the note at `from`. A link
    /// with only an anchor (`[[#Heading]]`) points into `from` itself.
    pub fn resolve<'a>(&'a self, inner: &str, from: &'a Path) -> Target<'a> {
        let anchor = anchor(inner);
        let target = normalize_target(inner);
        if target.is_empty() {
            return match anchor {
                Some(a) => Target::Notes(vec![(from, Some(a))]),
                None => Target::Missing,
            };
        }
        let notes = self.named(&target);
        if !notes.is_empty() {
            let at = |p: &'a PathBuf| (p.as_path(), anchor.clone());
            return Target::Notes(notes.iter().map(at).collect());
        }
        let name = normalize_name(inner);
        match self.ledger.iter().find(|d| d.name == name) {
            Some(decl) => Target::Declaration(decl),
            None => Target::Missing,
        }
    }

    /// Every link whose target is `target` (folded), in path order.
    pub fn references(&self, target: &str) -> Vec<Location> {
        let mut out = Vec::new();
        for (path, note) in &self.notes {
            let Ok(uri) = Url::from_file_path(path) else {
                continue;
            };
            for link in note.links.iter().filter(|l| l.target == target) {
                out.push(Location::new(uri.clone(), link.range));
            }
        }
        out
    }
}

/// The anchor of a link's inner text: what follows `#` (a heading, or
/// `^id` for a block), or `^id` for the bare `target^id` form.
pub fn anchor(inner: &str) -> Option<String> {
    let body = inner.split('|').next().unwrap_or(inner);
    let at = body.find(['#', '^'])?;
    let rest = body[at + 1..].trim();
    match &body[at..at + 1] {
        "#" => (!rest.is_empty()).then(|| rest.to_string()),
        _ => (!rest.is_empty()).then(|| format!("^{rest}")),
    }
}

/// The 0-based line `anchor` names in `text`: the line ending in `^id` for a
/// block, else the ATX heading whose text folds equal to the anchor's last
/// `#` segment. Fenced code is skipped, so `# comment` in a shell block is
/// not mistaken for a heading.
pub fn find_anchor(text: &str, anchor: &str) -> Option<u32> {
    if let Some(id) = anchor.strip_prefix('^') {
        let mark = format!("^{id}");
        return text
            .lines()
            .position(|line| {
                let line = line.trim_end();
                line.strip_suffix(&mark).is_some_and(|before| {
                    before.is_empty() || before.ends_with(char::is_whitespace)
                })
            })
            .map(|n| n as u32);
    }
    let want = fold_name(anchor.rsplit('#').next().unwrap_or(anchor).trim());
    let mut fence: Option<&str> = None;
    for (n, line) in text.lines().enumerate() {
        let body = line.trim_start();
        if line.len() - body.len() > 3 {
            continue;
        }
        if let Some(open) = fence {
            if body.starts_with(open) {
                fence = None;
            }
            continue;
        }
        if body.starts_with("```") || body.starts_with("~~~") {
            fence = Some(&body[..3]);
            continue;
        }
        if heading_text(body).is_some_and(|h| fold_name(h) == want) {
            return Some(n as u32);
        }
    }
    None
}

fn heading_text(line: &str) -> Option<&str> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    let rest = &line[level..];
    if !(1..=6).contains(&level)
        || !(rest.is_empty() || rest.starts_with([' ', '\t']))
    {
        return None;
    }
    let rest = rest.trim();
    let closed = rest.trim_end_matches('#');
    let text = if closed.is_empty() || closed.ends_with([' ', '\t']) {
        closed
    } else {
        rest
    };
    Some(text.trim())
}

/// A location at the start of `line` in `path`.
pub fn line_location(path: &Path, line: u32) -> Option<Location> {
    let uri = Url::from_file_path(path).ok()?;
    let at = Position::new(line, 0);
    Some(Location::new(uri, Range::new(at, at)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(files: &[(&str, &str)], config: &str) -> Vault {
        let mut v = Vault {
            root: PathBuf::from("/v"),
            enc: Encoding::Utf16,
            notes: BTreeMap::new(),
            names: HashMap::new(),
            ledger: settings::declarations(config),
        };
        for (path, text) in files {
            v.upsert(Path::new(path), text);
        }
        v
    }

    #[test]
    fn anchors() {
        assert_eq!(anchor("a#Head|alias"), Some("Head".into()));
        assert_eq!(anchor("a#^blk"), Some("^blk".into()));
        assert_eq!(anchor("a^blk"), Some("^blk".into()));
        assert_eq!(anchor("#Local"), Some("Local".into()));
        assert_eq!(anchor("a|x#y"), None);
        assert_eq!(anchor("a#"), None);
    }

    #[test]
    fn finds_headings_outside_fences() {
        let text = "# B\n```sh\n# Second\n```\n## Second ##\nx ^blk\n";
        assert_eq!(find_anchor(text, "second"), Some(4));
        assert_eq!(find_anchor(text, "B#Second"), Some(4));
        assert_eq!(find_anchor(text, "^blk"), Some(5));
        assert_eq!(find_anchor(text, "^bl"), None);
        assert_eq!(find_anchor("#nospace", "nospace"), None);
    }

    #[test]
    fn heading_text_drops_only_a_closing_sequence() {
        assert_eq!(heading_text("## A ##"), Some("A"));
        assert_eq!(heading_text("# C#"), Some("C#"));
        assert_eq!(heading_text("#"), Some(""));
        assert_eq!(heading_text("####### x"), None);
    }

    #[test]
    fn resolves_notes_case_and_form_insensitively() {
        let v = vault(&[("/v/n/дар.md", ""), ("/v/a.md", "")], "");
        let from = Path::new("/v/a.md");
        let want = Target::Notes(vec![(Path::new("/v/n/дар.md"), None)]);
        assert_eq!(v.resolve("Дар", from), want);
        assert_eq!(v.resolve("x/ДАР|alias", from), want);
        let local = Target::Notes(vec![(from, Some("H".into()))]);
        assert_eq!(v.resolve("#H", from), local);
        assert_eq!(v.resolve("", from), Target::Missing);
    }

    #[test]
    fn falls_back_to_ledger_by_full_name() {
        let config = "[categories]\nfood/groceries\n[accounts]\nrevolut\n";
        let v = vault(&[], config);
        let from = Path::new("/v/a.md");
        let Target::Declaration(d) = v.resolve("Food/Groceries", from) else {
            panic!("expected a declaration");
        };
        assert_eq!((d.name.as_str(), d.line), ("food/groceries", 1));
        assert_eq!(v.resolve("groceries", from), Target::Missing);
        assert!(matches!(v.resolve("revolut", from), Target::Declaration(_)));
    }

    #[test]
    fn note_beats_ledger_name() {
        let v = vault(&[("/v/revolut.md", "")], "[accounts]\nrevolut\n");
        let from = Path::new("/v/a.md");
        assert!(matches!(v.resolve("revolut", from), Target::Notes(_)));
    }

    #[test]
    fn references_report_ranges_in_path_order() {
        let v = vault(
            &[("/v/b.md", "[[Дар]]"), ("/v/a.md", "дар [[x|y]] [[дар#h]]")],
            "",
        );
        let got: Vec<_> = v
            .references("дар")
            .into_iter()
            .map(|l| (l.uri.path().to_string(), l.range))
            .collect();
        let r = |l, a, b| Range::new(Position::new(l, a), Position::new(l, b));
        assert_eq!(
            got,
            [
                ("/v/a.md".into(), r(0, 12, 21)),
                ("/v/b.md".into(), r(0, 0, 7))
            ]
        );
    }

    #[test]
    fn upsert_and_remove_keep_names_in_step() {
        let mut v = vault(&[("/v/a.md", ""), ("/v/x/a.md", "")], "");
        assert_eq!(v.named("a").len(), 2);
        v.upsert(Path::new("/v/a.md"), "[[b]]");
        assert_eq!(v.named("a").len(), 2);
        v.remove(Path::new("/v/a.md"));
        v.remove(Path::new("/v/x/a.md"));
        assert!(v.named("a").is_empty());
        assert!(v.names.is_empty());
    }

    #[test]
    fn is_note_rejects_hidden_and_outside() {
        let v = vault(&[], "");
        assert!(v.is_note(Path::new("/v/n/a.md")));
        assert!(!v.is_note(Path::new("/v/.git/a.md")));
        assert!(!v.is_note(Path::new("/v/n/a.txt")));
        assert!(!v.is_note(Path::new("/w/a.md")));
    }
}
