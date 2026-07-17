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
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;

/// Default stamp prefix — the author's handle, used by every note type except
/// `isg` notes (which lead with their own index, e.g. `isg20`).
pub const SIGNATURE: &str = "isg";

/// The filename postfix applied to every note created this run, set once from
/// `main` (the global `--postfix` flag). Kept here so it reaches the single
/// choke point every command shares — [`ensure_file`] and [`would_create`] —
/// without threading a parameter through each subcommand.
static POSTFIX: OnceLock<Option<String>> = OnceLock::new();

/// Record the run's filename postfix. Idempotent per process; call once, before
/// any note is created. `None` (or never calling this) leaves filenames as-is.
pub fn set_postfix(postfix: Option<String>) {
    let _ = POSTFIX.set(postfix);
}

/// Apply the run's postfix (if any) to `filename`, inserting it before the
/// extension: `2026-07-14T20.28.md` + `review` → `2026-07-14T20.28+review.md`.
fn apply_postfix(filename: &str) -> String {
    match POSTFIX.get().and_then(|p| p.as_deref()) {
        Some(postfix) => postfixed(filename, postfix),
        None => filename.to_string(),
    }
}

/// Insert `postfix` (joined with `+`) before the final `.ext` of `filename`,
/// or at the end when there is no extension.
fn postfixed(filename: &str, postfix: &str) -> String {
    match filename.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}+{postfix}.{ext}"),
        None => format!("{filename}+{postfix}"),
    }
}

/// Ensure a note exists at `<root>/<subdir>/<filename>`, seeding the header
/// (stamped with the current local time and the `[[tag]]` line) only when the
/// file is absent or empty. Returns the note's path.
///
/// `stamp_prefix` leads the stamp line (`<prefix> <date>`): usually
/// [`SIGNATURE`], but `isg` notes pass their basename so the header is
/// self-identifying, e.g. `isg20 2026-06-22 14:03:11 +0200`. `marker` is an
/// optional suffix appended to the stamp line — used to flag back-dated daily
/// notes with `*`.
pub fn ensure_note(
    root: &Path,
    subdir: &str,
    filename: &str,
    tag: &str,
    marker: Option<&str>,
    stamp_prefix: &str,
) -> std::io::Result<PathBuf> {
    ensure_file(root, subdir, filename, &seed_body(stamp_prefix, tag, marker))
}

/// Ensure a file exists at `<root>/<subdir>/<filename>`, writing `body` (making
/// parent dirs as needed) only when the file is absent or empty. Returns its
/// path. The body-agnostic core of [`ensure_note`]; used by commands that seed
/// their own contents (e.g. `shot -i`).
pub fn ensure_file(
    root: &Path,
    subdir: &str,
    filename: &str,
    body: &str,
) -> std::io::Result<PathBuf> {
    let dir = root.join(subdir);
    fs::create_dir_all(&dir)?;
    let note = dir.join(apply_postfix(filename));

    // zsh seeds on `[ ! -s "$note" ]` — absent or zero-length.
    let empty = fs::metadata(&note).map(|m| m.len() == 0).unwrap_or(true);
    if empty {
        fs::write(&note, body)?;
    }
    Ok(note)
}

/// Whether [`ensure_note`] *would* seed a new note here — i.e. the file is
/// absent or empty — without creating anything (not even the parent directory).
/// Returns the flag alongside the note's path. Used by `--check` flows that
/// prompt in the shell before a note is actually created.
pub fn would_create(root: &Path, subdir: &str, filename: &str) -> (bool, PathBuf) {
    let note = root.join(subdir).join(apply_postfix(filename));
    // Mirror the seed condition in `ensure_note`: absent or zero-length.
    let empty = fs::metadata(&note).map(|m| m.len() == 0).unwrap_or(true);
    (empty, note)
}

/// The stamp line leading every seeded note: `<prefix> <local datetime>`, e.g.
/// `isg 2026-06-20 14:03:11 +0200`. The single source of the stamp format;
/// reused by commands that assemble their own body (e.g. `shot -i`).
pub fn stamp_line(prefix: &str) -> String {
    let date = Local::now().format("%Y-%m-%d %H:%M:%S %z").to_string();
    format!("{prefix} {date}")
}

/// The seeded file contents: `<prefix> <date>[ <marker>]`, blank line, `[[tag]]`.
fn seed_body(stamp_prefix: &str, tag: &str, marker: Option<&str>) -> String {
    let stamp = stamp_line(stamp_prefix);
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
    fn postfixed_inserts_before_extension() {
        assert_eq!(postfixed("2026-07-14T20.28.md", "review"), "2026-07-14T20.28+review.md");
        assert_eq!(postfixed("TOP.md", "review"), "TOP+review.md");
    }

    #[test]
    fn postfixed_without_extension_appends() {
        assert_eq!(postfixed("NOTES", "review"), "NOTES+review");
    }

    #[test]
    fn would_create_reports_absent_empty_and_seeded() {
        let dir = std::env::temp_dir().join(format!("plc-note-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let root = &dir;

        // Absent → would create, and nothing is written by the check.
        let (new, path) = would_create(root, "sub", "n.md");
        assert!(new);
        assert!(!path.exists());
        assert!(!dir.join("sub").exists()); // no parent dir created

        // Empty file → still "would create".
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(&path, "").unwrap();
        assert!(would_create(root, "sub", "n.md").0);

        // Non-empty → would keep.
        fs::write(&path, "x").unwrap();
        assert!(!would_create(root, "sub", "n.md").0);

        fs::remove_dir_all(&dir).ok();
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
