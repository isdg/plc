//! `plc config [KEY]` — show resolved locations.
//!
//! Prints the paths a key names, with a leading `~` expanded, for editors and
//! scripts: `vault` (the default), `notes`, the directory `plc add` writes
//! to, `cats`, one line per category, or the dir of a kind plc lists:
//! `todo`, `isg`, `murmur`. None validates the vault, so they work before it
//! is decrypted.

use std::path::PathBuf;

use clap::{Args, ValueEnum};
use plc_core::rc::{self, Rc};

use crate::cmd::{isg, murmur, todo};
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
    /// Where todo notes live: <vault>/notes/management/todo.
    Todo,
    /// Where isg notes live: <vault>/notes/me/writing/isg.
    Isg,
    /// Where murmur notes live: <vault>/notes/me/writing/murmur.
    Murmur,
}

fn in_vault(subdir: &str) -> Result<Vec<PathBuf>, String> {
    config::palace_dir().map(|root| vec![root.join(subdir)])
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
        Key::Todo => in_vault(todo::SUBDIR),
        Key::Isg => in_vault(isg::SUBDIR),
        Key::Murmur => in_vault(murmur::SUBDIR),
    };
    let lines: Vec<String> = dirs
        .map_err(|e| format!("config: {e}"))?
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    Ok(lines.join("\n"))
}
