//! Resolve and validate the palace vault location.
//!
//! Mirrors the staged checks in `_palace_check` (palace.zsh) so a failure
//! points at the real problem: missing wrapper repo vs missing palace vs
//! a palace that hasn't been decrypted yet.

use std::env;
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

    /// The per-vault state/settings directory (`<root>/.plc`) — home to the
    /// `plc ledger` config, pointer files, and logs. A pure path: callers
    /// `create_dir_all` before writing into it.
    pub fn state_dir(&self) -> PathBuf {
        self.root.join(".plc")
    }
}
