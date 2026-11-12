//! `plc config [KEY]` — show a resolved location.
//!
//! Prints the path a key names, with a leading `~` expanded, for editors and
//! scripts: `vault` (the default) or `notes`, the directory `plc add` writes
//! to. Neither validates the vault, so they work before it is decrypted.

use clap::{Args, ValueEnum};
use plc_core::rc::{self, Rc};

use crate::config;

#[derive(Args)]
pub struct ConfigArgs {
    /// The location to print.
    #[arg(value_enum, default_value_t = Key::Vault)]
    key: Key,
}

#[derive(Clone, Copy, ValueEnum)]
enum Key {
    /// The vault root: $PALACE_DIR, else `vault` in .plcrc.
    Vault,
    /// Where `plc add` puts notes: `notes` in .plcrc, else
    /// <vault>/notes/triage.
    Notes,
}

pub fn run(args: ConfigArgs) -> Result<String, String> {
    let dir = match args.key {
        Key::Vault => config::palace_dir(),
        Key::Notes => rc::path()
            .and_then(|path| Rc::read(&path))
            .and_then(|rc| rc.notes_dir(config::palace_dir)),
    };
    dir.map(|p| p.display().to_string())
        .map_err(|e| format!("config: {e}"))
}
