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
