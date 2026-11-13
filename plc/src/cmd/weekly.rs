//! `plc weekly` — create/resolve this week's note and print its path.
//!
//! Ports `weekly()` (palace.zsh): target
//! `notes/management/weekly/<YYYY-Www>.md`, tag `weekly`, the week starting
//! on `week_start` (see [`week::label`]).

use chrono::Local;
use plc_core::{rc, week};

use crate::config::Palace;
use crate::note;

pub fn run(palace: &Palace) -> Result<String, String> {
    let start = rc::week_start().map_err(|e| format!("weekly: {e}"))?;
    let today = Local::now().date_naive();
    let filename = format!("{}.md", week::label(today, start));
    note::ensure_note(
        palace.root(),
        "notes/management/weekly",
        &filename,
        "weekly",
        None,
        note::SIGNATURE,
    )
    .map(|p| p.display().to_string())
    .map_err(|e| format!("weekly: {e}"))
}
