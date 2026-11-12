//! `plc add NAME` — create a stamped note in the notes directory.
//!
//! The directory is `plc config notes`: `notes` in `.plcrc`, under the vault
//! unless absolute, else `<vault>/notes`. The note is seeded with only the
//! stamp line; an existing note is kept and its path printed.

use std::path::{MAIN_SEPARATOR, PathBuf};

use clap::Args;
use plc_core::rc::{self, Rc};

use crate::config::Palace;
use crate::note;

#[derive(Args)]
pub struct AddArgs {
    /// The note's name (".md" appended if missing).
    #[arg(value_name = "NAME")]
    name: String,
}

pub fn run(args: AddArgs) -> Result<String, String> {
    let filename = md_name(&args.name)?;
    let rc = Rc::read(&rc::path().map_err(add)?).map_err(add)?;
    let dir = rc.notes_dir(vault_root)?;
    note::ensure_stamped(&dir, "", &filename)
        .map(|p| p.display().to_string())
        .map_err(add)
}

/// A relative notes dir lands in the vault, so it must be a valid one.
fn vault_root() -> Result<PathBuf, String> {
    Palace::resolve().map(|p| p.root().to_path_buf())
}

fn add(e: impl std::fmt::Display) -> String {
    format!("add: {e}")
}

/// `name` as one file name ending in `.md`: trimmed, and neither blank nor
/// holding a path separator.
fn md_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(add("empty name"));
    }
    if name.contains(['/', MAIN_SEPARATOR]) {
        let what = "name must not contain a path separator";
        return Err(add(format!("{what}: {name}")));
    }
    if name.ends_with(".md") {
        Ok(name.to_string())
    } else {
        Ok(format!("{name}.md"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md_name_cases() {
        assert_eq!(md_name(" idea ").unwrap(), "idea.md");
        assert_eq!(md_name("idea.md").unwrap(), "idea.md");
        assert_eq!(md_name("v0.2").unwrap(), "v0.2.md");
        assert!(md_name("  ").is_err());
        assert!(md_name("a/b").is_err());
        assert!(md_name("../b").is_err());
    }
}
