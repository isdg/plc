//! `plc config [KEY]` — show resolved locations.
//!
//! Prints the paths a key names, with a leading `~` expanded, for editors and
//! scripts: `vault` (the default), `notes`, the directory `plc add` writes
//! to, or `cats`, one line per category. None validates the vault, so they
//! work before it is decrypted.

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
    /// The categories: the dirs in <vault>/notes, one per line.
    Cats,
}

pub fn run(args: ConfigArgs) -> Result<String, String> {
    let dirs = match args.key {
        Key::Vault => config::palace_dir().map(|d| vec![d]),
        Key::Notes => rc::path()
            .and_then(|path| Rc::read(&path))
            .and_then(|rc| rc.notes_dir(config::palace_dir))
            .map(|d| vec![d]),
        Key::Cats => config::palace_dir()
            .and_then(|root| config::categories(&root)),
    };
    let lines: Vec<String> = dirs
        .map_err(|e| format!("config: {e}"))?
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    Ok(lines.join("\n"))
}
