//! Resolve and validate the palace vault location.
//!
//! Mirrors the staged checks in `_palace_check` (palace.zsh) so a failure
//! points at the real problem: missing wrapper repo vs missing palace vs
//! a palace that hasn't been decrypted yet.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// A validated handle to the palace vault root (the directory that contains
/// `notes/`). Construct via [`Palace::resolve`].
pub struct Palace {
    root: PathBuf,
}

/// The `~/.plcrc` path (`$HOME/.plcrc`), or `None` when `$HOME` is unset.
pub fn plcrc_path() -> Option<PathBuf> {
    env::var_os("HOME").map(|h| PathBuf::from(h).join(".plcrc"))
}

/// Read the `PALACE_DIR` value from `~/.plcrc`, if the file sets one.
pub fn read_plcrc_palace_dir() -> Option<String> {
    parse_plcrc(&fs::read_to_string(plcrc_path()?).ok()?)
}

/// Extract `PALACE_DIR` from `~/.plcrc` text. The file is `key = value` lines
/// (like `.plc/config`); `#` comments, blank lines, surrounding quotes, and a
/// tolerated leading `export ` are all handled.
fn parse_plcrc(text: &str) -> Option<String> {
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, val)) = line.split_once('=') else { continue };
        if key.trim() != "PALACE_DIR" {
            continue;
        }
        let val = val.trim().trim_matches(['"', '\'']).trim();
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }
    None
}

/// The configured vault path: `$PALACE_DIR` (when non-empty) else `~/.plcrc`'s
/// `PALACE_DIR`, with a leading `~` expanded to `$HOME`. `None` when neither is
/// set — the single source of truth for where the vault lives.
pub fn palace_dir() -> Option<PathBuf> {
    let raw = env::var("PALACE_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(read_plcrc_palace_dir)?;
    Some(expand_tilde(&raw))
}

/// Persist `dir` as the vault path in `~/.plcrc` (`export PALACE_DIR="dir"`),
/// replacing any existing `PALACE_DIR` line and preserving the rest. A leading
/// `~` is expanded so the stored path is absolute (portable to other readers).
/// Returns the file written.
pub fn write_plcrc_palace_dir(dir: &str) -> Result<PathBuf, String> {
    let path = plcrc_path().ok_or_else(|| "config: $HOME is not set".to_string())?;
    let abs = expand_tilde(dir);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut kept: Vec<String> = existing.lines().filter(|l| !is_palace_dir_line(l)).map(str::to_string).collect();
    kept.push(format!("PALACE_DIR = {}", abs.display()));
    fs::write(&path, kept.join("\n") + "\n").map_err(|e| format!("config: {e}"))?;
    Ok(path)
}

/// Whether a `~/.plcrc` line sets `PALACE_DIR` (with or without `export`).
fn is_palace_dir_line(line: &str) -> bool {
    let t = line.trim().strip_prefix("export ").unwrap_or(line.trim());
    t.split_once('=').is_some_and(|(k, _)| k.trim() == "PALACE_DIR")
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

    #[test]
    fn parse_plcrc_variants() {
        assert_eq!(parse_plcrc("export PALACE_DIR=\"~/x\"\n"), Some("~/x".into()));
        assert_eq!(parse_plcrc("PALACE_DIR=/y\n"), Some("/y".into()));
        assert_eq!(parse_plcrc("# a comment\nexport PALACE_DIR = '/z'\n"), Some("/z".into()));
        assert_eq!(parse_plcrc("# nothing here\nFOO=bar\n"), None);
        assert_eq!(parse_plcrc("PALACE_DIR=\n"), None); // empty value
    }

    #[test]
    fn expand_tilde_cases() {
        let home = Some(std::ffi::OsString::from("/home/x"));
        assert_eq!(expand_tilde_home("~/vault", home.clone()), PathBuf::from("/home/x/vault"));
        assert_eq!(expand_tilde_home("~", home.clone()), PathBuf::from("/home/x"));
        assert_eq!(expand_tilde_home("/abs/path", home), PathBuf::from("/abs/path"));
        // `~user` (no slash) is left untouched.
        assert_eq!(expand_tilde_home("~bob/x", Some("/h".into())), PathBuf::from("~bob/x"));
    }
}

impl Palace {
    /// Resolve the vault path (env or `~/.plcrc`), then validate in stages.
    pub fn resolve() -> Result<Palace, String> {
        let dir = palace_dir()
            .ok_or_else(|| "palace: PALACE_DIR is not set (environment or ~/.plcrc)".to_string())?;
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
