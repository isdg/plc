//! `plc shot` — create a timestamped daily snapshot note and print its path.
//!
//! Ports `shot()` (palace.zsh): target
//! `notes/management/daily/%Y/%m/<%Y-%m-%dT%H.%M>.md`, tag `shots`.

use chrono::Local;

use crate::config::Palace;
use crate::note;

pub fn run(palace: &Palace) -> Result<String, String> {
    let now = Local::now();
    let subdir = now.format("notes/management/daily/%Y/%m").to_string();
    let filename = now.format("%Y-%m-%dT%H.%M.md").to_string();
    note::ensure_note(palace.root(), &subdir, &filename, "shots", None, note::SIGNATURE)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("shot: {e}"))
}
