//! `plc shot` — create a timestamped snapshot note and print its path.
//!
//! Writes `<%Y-%m-%dT%H.%M>.md`, tagged `shots`, into a directory chosen by
//! `-p/--path` (created if absent):
//!   * omitted        → the current directory
//!   * `@notes/inbox` → the vault root (needs `$PALACE_DIR`)
//!   * any other path → used as-is if absolute, else relative to the cwd
//!
//! Only `@` paths touch the vault; a plain `shot` needs no `$PALACE_DIR`.

use std::env;
use std::path::{Path, PathBuf};

use chrono::Local;
use clap::Args;

use crate::config::Palace;
use crate::note;

#[derive(Args)]
pub struct ShotArgs {
    /// Target directory, created if absent. Default: the current directory.
    /// A leading `@` resolves against the vault root (e.g. `@notes/inbox`).
    #[arg(short = 'p', long = "path", value_name = "PATH")]
    path: Option<String>,
}

pub fn run(args: ShotArgs) -> Result<String, String> {
    let now = Local::now();
    let dir = target_dir(args.path.as_deref())?;
    let filename = now.format("%Y-%m-%dT%H.%M.md").to_string();

    note::ensure_note(&dir, "", &filename, "shots", None, note::SIGNATURE)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("shot: {e}"))
}

/// Resolve the directory the note lands in from `-p`. `@`-prefixed values
/// anchor at the vault root (resolving `$PALACE_DIR` only then); everything
/// else is filesystem-relative to the current directory.
fn target_dir(path: Option<&str>) -> Result<PathBuf, String> {
    let path = match path {
        None => return env::current_dir().map_err(|e| format!("shot: {e}")),
        Some(p) => p,
    };
    if let Some(rest) = path.strip_prefix('@') {
        let palace = Palace::resolve()?;
        return Ok(palace.root().join(rest.trim_matches('/')));
    }
    let cwd = env::current_dir().map_err(|e| format!("shot: {e}"))?;
    Ok(join_fs(&cwd, path))
}

/// A filesystem `-p` value: used as-is when absolute, else joined onto `cwd`.
fn join_fs(cwd: &Path, path: &str) -> PathBuf {
    let pb = PathBuf::from(path);
    if pb.is_absolute() {
        pb
    } else {
        cwd.join(pb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_joins_cwd() {
        assert_eq!(
            join_fs(Path::new("/home/u"), "notes"),
            Path::new("/home/u/notes")
        );
    }

    #[test]
    fn absolute_path_used_as_is() {
        assert_eq!(join_fs(Path::new("/home/u"), "/tmp/x"), Path::new("/tmp/x"));
    }
}
