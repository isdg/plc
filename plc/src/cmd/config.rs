//! `plc config` — show the resolved vault location.
//!
//! Bare, it prints the resolved `PALACE_DIR` so dotfiles can wire it up without
//! hardcoding the path:
//!
//! ```sh
//! export PALACE_DIR="$(plc config)"
//! ```
//!
//! `--rc` prints the `~/.plcrc` file's own path.

use clap::Args;

use crate::config;

#[derive(Args)]
pub struct ConfigArgs {
    /// Print the path of the `~/.plcrc` file itself (not the vault).
    #[arg(long = "rc")]
    rc: bool,
}

pub fn run(args: ConfigArgs) -> Result<String, String> {
    if args.rc {
        return config::plcrc_path()
            .map(|p| p.display().to_string())
            .ok_or_else(|| "config: $HOME is not set".to_string());
    }
    // Bare: the resolved vault path (env or ~/.plcrc), for `$(plc config)`.
    config::palace_dir()
        .map(|p| p.display().to_string())
        .ok_or_else(|| "config: PALACE_DIR is not set (environment or ~/.plcrc)".to_string())
}
