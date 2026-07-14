//! `plc top` — create/resolve `TOP.md` at the vault root and print its path.
//!
//! The palace landing page: a single well-known file living directly under the
//! vault root (not under `notes/`), seeded with the usual header when absent or
//! empty. The zsh wrapper opens the printed path with `$EDITOR`.

use crate::config::Palace;
use crate::note;

pub fn run(palace: &Palace) -> Result<String, String> {
    // Empty subdir → the note lands at the vault root itself.
    note::ensure_note(palace.root(), "", "TOP.md", "top", None, note::SIGNATURE)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("top: {e}"))
}
