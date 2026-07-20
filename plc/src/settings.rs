//! Per-vault `plc` settings, stored as plain text at `<PALACE_DIR>/.plc/config`.
//!
//! The format is deliberately minimal and hand-editable: `key = value` lines at
//! the top, then `[categories]` / `[accounts]` sections with one normalized name
//! per line. `#` starts a comment; blank lines are ignored.
//!
//!     # plc settings
//!     currency = EUR
//!
//!     [categories]
//!     food/groceries
//!     rent
//!
//!     [accounts]
//!     revolut
//!     cash
//!
//! Names are normalized with [`plc_core::finance::normalize_name`], the same
//! rule the ledger uses, so declared names match how transactions store them.

use std::fs;
use std::path::{Path, PathBuf};

use plc_core::finance::normalize_name;

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

#[derive(Clone, Copy)]
enum Section {
    None,
    Categories,
    Accounts,
}

impl Settings {
    /// The config path for a vault root.
    pub fn path(root: &Path) -> PathBuf {
        root.join(".plc").join("config")
    }

    /// Load `<root>/.plc/config`, or defaults when it is absent/unreadable.
    pub fn load(root: &Path) -> Settings {
        let Ok(text) = fs::read_to_string(Self::path(root)) else {
            return Settings::default();
        };
        let mut s = Settings::default();
        let mut section = Section::None;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = match inner.trim() {
                    "categories" => Section::Categories,
                    "accounts" => Section::Accounts,
                    _ => Section::None,
                };
                continue;
            }
            match section {
                Section::None => {
                    if let Some((k, v)) = line.split_once('=') {
                        if k.trim() == "currency" {
                            let v = v.trim();
                            if !v.is_empty() {
                                s.currency = Some(v.to_uppercase());
                            }
                        }
                    }
                }
                Section::Categories => insert_name(&mut s.categories, line),
                Section::Accounts => insert_name(&mut s.accounts, line),
            }
        }
        s
    }

    /// Write the config in canonical form (creating `.plc/`). Comments a user may
    /// have added are not preserved — this is a managed file.
    pub fn save(&self, root: &Path) -> Result<(), String> {
        let dir = root.join(".plc");
        fs::create_dir_all(&dir).map_err(|e| format!("settings: {e}"))?;
        let mut out = String::from("# plc settings — managed by `plc fin` (safe to hand-edit)\n");
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
