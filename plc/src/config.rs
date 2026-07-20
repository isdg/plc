//! Resolve and validate the palace vault location.
//!
//! Mirrors the staged checks in `_palace_check` (palace.zsh) so a failure
//! points at the real problem: missing wrapper repo vs missing palace vs
//! a palace that hasn't been decrypted yet.

use std::env;
use std::path::{Path, PathBuf};

/// A validated handle to the palace vault root (the directory that contains
/// `notes/`). Construct via [`Palace::resolve`].
pub struct Palace {
    root: PathBuf,
}

impl Palace {
    /// Resolve `$PALACE_DIR` from the environment, then validate in stages.
    pub fn resolve() -> Result<Palace, String> {
        let dir = env::var_os("PALACE_DIR")
            .ok_or_else(|| "palace: PALACE_DIR is not set".to_string())?;
        Self::validate(PathBuf::from(dir))
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
    /// `plc fin` config, pointer files, and logs. A pure path: callers
    /// `create_dir_all` before writing into it.
    pub fn state_dir(&self) -> PathBuf {
        self.root.join(".plc")
    }
}
