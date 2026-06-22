//! Create-or-keep a note file and return its path.
//!
//! Ports the file half of `_palace_note` (palace.zsh): make the parent
//! directory, and when the note is absent or empty seed a header of the form
//!
//! ```text
//! isg 2026-06-20 14:03:11 +0200
//!
//! [[daily]]
//! ```
//!
//! The editor is intentionally *not* launched here — `plc` prints the path
//! and the zsh wrapper opens it with `$EDITOR`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;

/// Ensure a note exists at `<root>/<subdir>/<filename>`, seeding the header
/// (stamped with the current local time and the `[[tag]]` line) only when the
/// file is absent or empty. Returns the note's path.
///
/// `marker` is an optional suffix appended to the stamp line — used to flag
/// back-dated daily notes with `*`.
pub fn ensure_note(
    root: &Path,
    subdir: &str,
    filename: &str,
    tag: &str,
    marker: Option<&str>,
) -> std::io::Result<PathBuf> {
    let dir = root.join(subdir);
    fs::create_dir_all(&dir)?;
    let note = dir.join(filename);

    // zsh seeds on `[ ! -s "$note" ]` — absent or zero-length.
    let empty = fs::metadata(&note).map(|m| m.len() == 0).unwrap_or(true);
    if empty {
        fs::write(&note, seed_body(tag, marker))?;
    }
    Ok(note)
}

/// The seeded file contents: stamp line, blank line, `[[tag]]`.
fn seed_body(tag: &str, marker: Option<&str>) -> String {
    let stamp = Local::now().format("isg %Y-%m-%d %H:%M:%S %z").to_string();
    let stamp = match marker {
        Some(m) if !m.is_empty() => format!("{stamp} {m}"),
        _ => stamp,
    };
    format!("{stamp}\n\n[[{tag}]]\n")
}

/// List `*.md` basenames in `dir`, newest-first (mtime desc, name asc on ties).
/// Shared by the `murmur` and `isg` list/pick flows.
pub fn list_md_by_recency(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut entries: Vec<(String, SystemTime)> = fs::read_dir(dir)?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !name.ends_with(".md") {
                return None;
            }
            let mtime = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
            Some((name, mtime))
        })
        .collect();
    Ok(order_by_recency(&mut entries))
}

/// Sort entries newest-first, breaking mtime ties by name ascending, and
/// return just the names.
fn order_by_recency(entries: &mut [(String, SystemTime)]) -> Vec<String> {
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.iter().map(|(n, _)| n.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn t(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn recency_newest_first() {
        let mut e = vec![
            ("old.md".to_string(), t(100)),
            ("new.md".to_string(), t(300)),
            ("mid.md".to_string(), t(200)),
        ];
        assert_eq!(order_by_recency(&mut e), ["new.md", "mid.md", "old.md"]);
    }

    #[test]
    fn recency_ties_break_by_name() {
        let mut e = vec![
            ("b.md".to_string(), t(100)),
            ("a.md".to_string(), t(100)),
        ];
        assert_eq!(order_by_recency(&mut e), ["a.md", "b.md"]);
    }
}
