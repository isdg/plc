//! A vault's notes: the `.md` files under its root, outside any hidden
//! directory. The one definition `plc-lsp` indexes and `plc graph` draws.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, FixedOffset, NaiveDate};
use walkdir::{DirEntry, WalkDir};

use crate::ledger::TIMESTAMP_FMT;

/// The note name of `path`: its file name without `.md`, if it has one.
pub fn stem(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()?.strip_suffix(".md")
}

/// The time a stamp line holds: `<signature> <date> <time> <zone>`, the
/// line plc seeds every note with.
pub fn stamp(line: &str) -> Option<DateTime<FixedOffset>> {
    let mut tokens = line.split_whitespace().skip(1);
    let stamp = [tokens.next()?, tokens.next()?, tokens.next()?].join(" ");
    DateTime::parse_from_str(&stamp, TIMESTAMP_FMT).ok()
}

/// The first `YYYY-MM-DD` in a note name, as daily notes and shots have.
pub fn name_date(name: &str) -> Option<NaiveDate> {
    let bytes = name.as_bytes();
    (0..bytes.len().saturating_sub(9)).find_map(|i| {
        let s = name.get(i..i + 10)?;
        let shaped = s.bytes().enumerate().all(|(k, b)| match k {
            4 | 7 => b == b'-',
            _ => b.is_ascii_digit(),
        });
        shaped
            .then(|| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .flatten()
    })
}

fn hidden(name: &OsStr) -> bool {
    name.to_str().is_some_and(|s| s.starts_with('.'))
}

/// Every note under `root`, in directory-walk order. Unreadable entries are
/// left out.
pub fn walk(root: &Path) -> impl Iterator<Item = PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !hidden(e.file_name()))
        .flatten()
        .filter(|e| e.file_type().is_file() && stem(e.path()).is_some())
        .map(DirEntry::into_path)
}

/// Whether `path` is a note of the vault at `root`, by the rule [`walk`]
/// follows.
pub fn is_note(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    stem(path).is_some()
        && rel.components().all(|c| match c {
            Component::Normal(name) => !hidden(name),
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn walk_skips_hidden_and_non_notes() {
        let dir = std::env::temp_dir()
            .join(format!("plc-notes-walk-{}", std::process::id()));
        for rel in ["a.md", "sub/b.md", "sub/c.txt", ".git/d.md", ".e.md"] {
            let path = dir.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "").unwrap();
        }
        let mut found: Vec<_> = walk(&dir)
            .map(|p| p.strip_prefix(&dir).unwrap().to_path_buf())
            .collect();
        found.sort();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found, [PathBuf::from("a.md"), PathBuf::from("sub/b.md")]);
    }

    #[test]
    fn is_note_follows_the_walk() {
        let root = Path::new("/v");
        assert!(is_note(root, Path::new("/v/n/a.md")));
        assert!(!is_note(root, Path::new("/v/.git/a.md")));
        assert!(!is_note(root, Path::new("/v/n/a.txt")));
        assert!(!is_note(root, Path::new("/elsewhere/a.md")));
        assert!(!is_note(root, Path::new("/v/../x/a.md")));
    }
}
