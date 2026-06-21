//! `plc weekly` — create/resolve this ISO week's note and print its path.
//!
//! Ports `weekly()` (palace.zsh): target
//! `notes/management/weekly/<%G-W%V>.md`, tag `weekly`.

use std::path::PathBuf;

use chrono::Local;

use crate::config::Palace;
use crate::note;

pub fn run(palace: &Palace) -> Result<PathBuf, String> {
    let filename = Local::now().format("%G-W%V.md").to_string();
    note::ensure_note(palace.root(), "notes/management/weekly", &filename, "weekly", None)
        .map_err(|e| format!("weekly: {e}"))
}
