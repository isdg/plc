//! `plc config` — show the resolved vault location.
//!
//! Prints `$PALACE_DIR` with a leading `~` expanded, for editors and scripts
//! that need the absolute vault path.

use crate::config;

pub fn run() -> Result<String, String> {
    config::palace_dir()
        .map(|p| p.display().to_string())
        .map_err(|e| format!("config: {e}"))
}
