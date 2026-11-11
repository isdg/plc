//! Resolve and validate the palace vault location.
//!
//! Mirrors the staged checks in `_palace_check` (palace.zsh) so a failure
//! points at the real problem: missing wrapper repo vs missing palace vs
//! a palace that hasn't been decrypted yet.

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// A validated handle to the palace vault root (the directory that contains
/// `notes/`). Construct via [`Palace::resolve`].
pub struct Palace {
    root: PathBuf,
}

/// The configured vault path: `$PALACE_DIR` (when non-empty), with a leading
/// `~` expanded to `$HOME` — the single source of truth for where the vault
/// lives. The error says how to set it.
pub fn palace_dir() -> Result<PathBuf, String> {
    match env::var("PALACE_DIR") {
        Ok(raw) if !raw.trim().is_empty() => Ok(expand_tilde(&raw)),
        _ => Err(palace_dir_unset(env::var_os("HOME").as_deref())),
    }
}

/// Why [`palace_dir`] failed, given `$HOME`; split out so it is testable
/// without touching the process environment.
fn palace_dir_unset(home: Option<&OsStr>) -> String {
    let msg = "PALACE_DIR is not set; \
               add `export PALACE_DIR=/path/to/vault` to your shell rc";
    // plc used to read the path from ~/.plcrc; a leftover one explains why a
    // setup that worked before now fails.
    if home.is_some_and(|h| Path::new(h).join(".plcrc").exists()) {
        format!("{msg} (~/.plcrc is no longer read)")
    } else {
        msg.to_string()
    }
}

/// Expand a leading `~` / `~/` to `$HOME`; otherwise the path as-is.
fn expand_tilde(s: &str) -> PathBuf {
    expand_tilde_home(s, env::var_os("HOME"))
}

/// [`expand_tilde`] with an injected home, so it is testable without touching
/// the process environment.
fn expand_tilde_home(s: &str, home: Option<std::ffi::OsString>) -> PathBuf {
    let s = s.trim();
    match s.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => match home {
            Some(h) => PathBuf::from(h).join(rest.trim_start_matches('/')),
            None => PathBuf::from(s),
        },
        _ => PathBuf::from(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn expand_tilde_cases() {
        let home = Some(std::ffi::OsString::from("/home/x"));
        assert_eq!(expand_tilde_home("~/vault", home.clone()), PathBuf::from("/home/x/vault"));
        assert_eq!(expand_tilde_home("~", home.clone()), PathBuf::from("/home/x"));
        assert_eq!(expand_tilde_home("/abs/path", home), PathBuf::from("/abs/path"));
        // `~user` (no slash) is left untouched.
        assert_eq!(expand_tilde_home("~bob/x", Some("/h".into())), PathBuf::from("~bob/x"));
    }

    #[test]
    fn unset_message_names_a_leftover_plcrc() {
        let tag = format!("plc-unset-{}", std::process::id());
        let home = std::env::temp_dir().join(tag);
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        let bare = palace_dir_unset(Some(home.as_os_str()));
        assert!(bare.starts_with("PALACE_DIR is not set"));
        assert!(!bare.contains(".plcrc"));
        assert_eq!(palace_dir_unset(None), bare);

        fs::write(home.join(".plcrc"), "PALACE_DIR = /v\n").unwrap();
        let hinted = palace_dir_unset(Some(home.as_os_str()));
        assert_eq!(hinted, format!("{bare} (~/.plcrc is no longer read)"));
        fs::remove_dir_all(&home).ok();
    }
}

impl Palace {
    /// Resolve the vault path from `$PALACE_DIR`, then validate in stages.
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
