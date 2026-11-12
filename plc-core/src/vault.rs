//! Resolve and validate the palace vault location.
//!
//! Mirrors the staged checks in `_palace_check` (palace.zsh) so a failure
//! points at the real problem: missing wrapper repo vs missing palace vs
//! a palace that hasn't been decrypted yet.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::rc::{self, expand_tilde, Rc};

/// A validated handle to the palace vault root (the directory that contains
/// `notes/`). Construct via [`Palace::resolve`].
pub struct Palace {
    root: PathBuf,
}

/// The configured vault path — the single source of truth for where the
/// vault lives: `$PALACE_DIR` (when non-empty), else `vault` in `.plcrc`,
/// each with a leading `~` expanded. The error says how to set one.
pub fn palace_dir() -> Result<PathBuf, String> {
    match env::var("PALACE_DIR") {
        Ok(raw) if !raw.trim().is_empty() => Ok(expand_tilde(&raw)),
        _ => {
            let path = rc::path()?;
            let rc = Rc::read(&path)?;
            match rc.vault {
                Some(vault) => Ok(vault),
                None => Err(no_vault(&path, &rc)),
            }
        }
    }
}

/// The categories of the vault at `root`: the directories in `notes/`,
/// sorted, leaving out hidden ones.
pub fn categories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let notes = root.join("notes");
    let read = |e| format!("read {}: {e}", rc::tilde(&notes));
    let mut dirs = Vec::new();
    for entry in fs::read_dir(&notes).map_err(read)? {
        let path = entry.map_err(read)?.path();
        let hidden = path
            .file_name()
            .is_some_and(|n| n.as_encoded_bytes().starts_with(b"."));
        if !hidden && path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn no_vault(path: &Path, rc: &Rc) -> String {
    let add = "add `vault = /path/to/vault` to";
    let msg = format!("no vault: set PALACE_DIR, or {add} {}", rc::tilde(path));
    if rc.has_legacy_key() {
        format!("{msg} (rename its PALACE_DIR key to `vault`)")
    } else {
        msg
    }
}

impl Palace {
    /// Resolve the vault path (see [`palace_dir`]), then validate in stages.
    pub fn resolve() -> Result<Palace, String> {
        let dir = palace_dir().map_err(|e| format!("palace: {e}"))?;
        Self::validate(dir)
    }

    /// Staged validation, split out so it is unit-testable without env state.
    fn validate(root: PathBuf) -> Result<Palace, String> {
        if let Some(parent) = root.parent() {
            if !parent.as_os_str().is_empty() && !parent.is_dir() {
                return Err(format!(
                    "palace: parent '{}' does not exist, clone it",
                    parent.display()
                ));
            }
        }
        if !root.is_dir() {
            return Err(format!(
                "palace: '{}' does not exist (palace itself missing, decrypt it)",
                root.display()
            ));
        }
        if !root.join("notes").is_dir() {
            return Err(format!(
                "palace: '{}' has no 'notes/' inside (not decrypted yet?)",
                root.display()
            ));
        }
        Ok(Palace { root })
    }

    /// The validated vault root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The vault's own plc directory, `<root>/.plc`: settings and pointers
    /// that travel with the vault (`config`, `last-do`). A pure path:
    /// callers `create_dir_all` before writing into it.
    pub fn plc_dir(&self) -> PathBuf {
        self.root.join(".plc")
    }

    /// This machine's state for the vault, `<plc home>/<vault name>`, named
    /// after the root's basename: caches rebuilt locally and never synced.
    /// A pure path, like [`Palace::plc_dir`].
    pub fn state_dir(&self) -> Result<PathBuf, String> {
        let home = rc::home()?;
        let root = std::path::absolute(&self.root);
        let root = root.map_err(|e| format!("palace: {e}"))?;
        // A dot name could collide with `.plcrc` or other plc files.
        let usable = |n: &&OsStr| !n.as_encoded_bytes().starts_with(b".");
        match root.file_name().filter(usable) {
            Some(name) => Ok(home.join(name)),
            None => Err(format!(
                "palace: vault '{}' can't name a state dir in {}: \
                 its name must not be empty or start with '.'",
                root.display(),
                rc::tilde(&home)
            )),
        }
    }
}
