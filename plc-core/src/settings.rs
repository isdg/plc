//! Per-vault `plc` settings, stored as plain text at `<PALACE_DIR>/.plc/config`.
//!
//! The format is deliberately minimal and hand-editable: `key = value` lines at
//! the top, then `[categories]` / `[accounts]` sections with one normalized name
//! per line. `#` starts a comment; blank lines are ignored.
//!
//! ```text
//! # plc settings
//! currency = EUR
//!
//! [categories]
//! food/groceries
//! rent
//!
//! [accounts]
//! revolut
//! cash
//! ```
//!
//! Names are normalized with [`crate::ledger::normalize_name`], the same
//! rule the ledger uses, so declared names match how transactions store them.

use std::fs;
use std::path::{Path, PathBuf};

use crate::ledger::normalize_name;

/// The parsed `.plc/config`. A missing file loads as [`Settings::default`].
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Vault default currency (already upper-cased); `None` → fall back to `EUR`.
    pub currency: Option<String>,
    /// Declared expense/income categories (`#`), sorted, de-duplicated.
    pub categories: Vec<String>,
    /// Declared accounts (`@`), sorted, de-duplicated.
    pub accounts: Vec<String>,
}

/// The kind of ledger name a `.plc/config` line declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Category,
    Account,
}

/// A declared name, normalized, with its 0-based line in `.plc/config`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub kind: Kind,
    pub line: usize,
}

enum Entry<'a> {
    Currency(&'a str),
    Name(Kind, &'a str),
}

/// Every meaningful line of a config text with its 0-based line number: the
/// one walker [`Settings::parse`] and [`declarations`] share. An unknown
/// `[section]` returns to top-level `key = value` parsing.
fn entries(text: &str) -> impl Iterator<Item = (usize, Entry<'_>)> {
    let mut section: Option<Kind> = None;
    text.lines().enumerate().filter_map(move |(n, raw)| {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let header = line.strip_prefix('[').and_then(|l| l.strip_suffix(']'));
        if let Some(inner) = header {
            section = match inner.trim() {
                "categories" => Some(Kind::Category),
                "accounts" => Some(Kind::Account),
                _ => None,
            };
            return None;
        }
        if let Some(kind) = section {
            return Some((n, Entry::Name(kind, line)));
        }
        let (k, v) = line.split_once('=')?;
        let v = v.trim();
        (k.trim() == "currency" && !v.is_empty())
            .then_some((n, Entry::Currency(v)))
    })
}

/// The declared accounts and categories of a config text, in file order.
/// Names that normalize to nothing are skipped.
pub fn declarations(text: &str) -> Vec<Declaration> {
    entries(text)
        .filter_map(|(line, entry)| {
            let Entry::Name(kind, raw) = entry else {
                return None;
            };
            let name = normalize_name(raw);
            (!name.is_empty()).then_some(Declaration { name, kind, line })
        })
        .collect()
}

impl Settings {
    /// The config path for a vault root.
    pub fn path(root: &Path) -> PathBuf {
        root.join(".plc").join("config")
    }

    /// Load `<root>/.plc/config`, or defaults when it is absent/unreadable.
    pub fn load(root: &Path) -> Settings {
        fs::read_to_string(Self::path(root))
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    /// Parse a config text; unknown keys and sections are ignored.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for (_, entry) in entries(text) {
            match entry {
                Entry::Currency(v) => s.currency = Some(v.to_uppercase()),
                Entry::Name(Kind::Category, raw) => {
                    insert_name(&mut s.categories, raw)
                }
                Entry::Name(Kind::Account, raw) => {
                    insert_name(&mut s.accounts, raw)
                }
            }
        }
        s
    }

    /// Write the config in canonical form (creating `.plc/`). Comments a user may
    /// have added are not preserved — this is a managed file.
    pub fn save(&self, root: &Path) -> Result<(), String> {
        let dir = root.join(".plc");
        fs::create_dir_all(&dir).map_err(|e| format!("settings: {e}"))?;
        let mut out = String::from("# plc settings — managed by `plc ledger` (safe to hand-edit)\n");
        if let Some(c) = &self.currency {
            out.push_str(&format!("currency = {c}\n"));
        }
        out.push_str("\n[categories]\n");
        for c in &self.categories {
            out.push_str(c);
            out.push('\n');
        }
        out.push_str("\n[accounts]\n");
        for a in &self.accounts {
            out.push_str(a);
            out.push('\n');
        }
        fs::write(Self::path(root), out).map_err(|e| format!("settings: {e}"))
    }
}

/// Normalize `raw` and insert into a sorted, de-duplicated name list.
fn insert_name(list: &mut Vec<String>, raw: &str) {
    let name = normalize_name(raw);
    if name.is_empty() {
        return;
    }
    if let Err(pos) = list.binary_search(&name) {
        list.insert(pos, name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("plc-set-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_is_default() {
        let root = tmp("missing");
        assert_eq!(Settings::load(&root), Settings::default());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn parses_currency_and_sections() {
        let root = tmp("parse");
        fs::create_dir_all(root.join(".plc")).unwrap();
        fs::write(
            Settings::path(&root),
            "# a comment\ncurrency = usd\n\n[categories]\nFood/Groceries\nrent\n\n[accounts]\nRevolut\ncash\n",
        )
        .unwrap();
        let s = Settings::load(&root);
        assert_eq!(s.currency.as_deref(), Some("USD"));
        assert_eq!(s.categories, vec!["food/groceries", "rent"]); // normalized
        assert_eq!(s.accounts, vec!["cash", "revolut"]); // sorted
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn declarations_carry_kind_and_line() {
        let text = "currency = EUR\n\n[categories]\nFood/Groceries\n\
                    # note\n[accounts]\nRevolut\n";
        let got: Vec<_> = declarations(text)
            .into_iter()
            .map(|d| (d.name, d.kind, d.line))
            .collect();
        assert_eq!(
            got,
            [
                ("food/groceries".to_string(), Kind::Category, 3),
                ("revolut".to_string(), Kind::Account, 6),
            ]
        );
    }

    #[test]
    fn unknown_section_returns_to_keys() {
        let s = Settings::parse("[accounts]\ncash\n[x]\ncurrency = usd\n");
        assert_eq!(s.currency.as_deref(), Some("USD"));
        assert_eq!(s.accounts, vec!["cash"]);
    }

    #[test]
    fn save_load_round_trip_and_dedup() {
        let root = tmp("rt");
        let s = Settings {
            currency: Some("EUR".into()),
            categories: {
                let mut v = Vec::new();
                insert_name(&mut v, "rent");
                insert_name(&mut v, "food");
                insert_name(&mut v, "rent"); // dup dropped
                v
            },
            accounts: vec!["cash".into()],
        };
        s.save(&root).unwrap();
        assert_eq!(Settings::load(&root), s);
        assert_eq!(s.categories, vec!["food", "rent"]);
        fs::remove_dir_all(&root).ok();
    }
}
