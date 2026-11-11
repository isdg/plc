//! `plc config` — show the resolved vault location.
//!
//! Bare, it prints the resolved `PALACE_DIR` so dotfiles can wire it up without
//! hardcoding the path:
//!
//! ```sh
//! export PALACE_DIR="$(plc config)"
//! ```

use crate::config;

pub fn run() -> Result<String, String> {
    config::palace_dir()
        .map(|p| p.display().to_string())
        .ok_or_else(|| "config: PALACE_DIR is not set (environment or ~/.plcrc)".to_string())
}
