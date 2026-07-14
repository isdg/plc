//! `plc shot` — create a timestamped snapshot note and print its path.
//!
//! Writes `<%Y-%m-%dT%H.%M>.md`, tagged `shots`, into a directory chosen by
//! `-p/--path` (created if absent):
//!   * omitted        → the vault daily dir, `notes/management/daily/%Y/%m`
//!   * `@notes/inbox` → the vault root
//!   * any other path → used as-is if absolute, else relative to the cwd
//!
//! The default and `@` paths resolve `$PALACE_DIR`; a filesystem `-p` does not.
//!
//! `-i/--inline TEXT` seeds the body inline (under the stamp, in place of the
//! `[[shots]]` tag). `--no-header` drops the stamp; the two are independent, so
//! `--no-header` alone creates an empty note and `-i … --no-header` writes just
//! the text.

use std::env;
use std::path::{Path, PathBuf};

use chrono::Local;
use clap::Args;

use crate::config::Palace;
use crate::note;

#[derive(Args)]
pub struct ShotArgs {
    /// Target directory, created if absent. Default: the vault daily dir.
    /// A leading `@` resolves against the vault root (e.g. `@notes/inbox`);
    /// any other path is relative to the current directory (or absolute).
    #[arg(short = 'p', long = "path", value_name = "PATH")]
    path: Option<String>,
    /// Inline note body: written under the stamp, in place of the `[[shots]]`
    /// tag. Without this, `shot` seeds the usual tagged header.
    #[arg(short = 'i', long = "inline", value_name = "TEXT")]
    inline: Option<String>,
    /// Omit the stamp header. On its own creates an empty note; with `-i`,
    /// writes just the inline text.
    #[arg(short = 'H', long = "no-header")]
    no_header: bool,
}

pub fn run(args: ShotArgs) -> Result<String, String> {
    let now = Local::now();
    let daily = now.format("notes/management/daily/%Y/%m").to_string();
    let dir = target_dir(args.path.as_deref(), &daily)?;
    let filename = now.format("%Y-%m-%dT%H.%M.md").to_string();

    // The plain `shot` (stamp + `[[shots]]` tag) is the seeded default; any
    // `-i`/`--no-header` combination assembles its own body instead.
    let result = match (args.inline.as_deref(), args.no_header) {
        (None, false) => note::ensure_note(&dir, "", &filename, "shots", None, note::SIGNATURE),
        (inline, no_header) => {
            let body = build_body(&note::stamp_line(note::SIGNATURE), inline, !no_header);
            note::ensure_file(&dir, "", &filename, &body)
        }
    };
    result
        .map(|p| p.display().to_string())
        .map_err(|e| format!("shot: {e}"))
}

/// Assemble the note body from a stamp, optional inline text, and whether to
/// keep the stamp header. The stamp and the text are joined by a blank line;
/// each present piece ends in a newline (no header + no text → empty file).
fn build_body(stamp: &str, inline: Option<&str>, header: bool) -> String {
    match (header, inline) {
        (true, Some(text)) => format!("{stamp}\n\n{text}\n"),
        (true, None) => format!("{stamp}\n"),
        (false, Some(text)) => format!("{text}\n"),
        (false, None) => String::new(),
    }
}

/// Resolve the directory the note lands in. No `-p` → the vault `daily` dir;
/// an `@`-prefixed value → the vault root; anything else → the filesystem,
/// relative to the cwd (or used as-is when absolute). Vault targets resolve
/// `$PALACE_DIR` (and only then).
fn target_dir(path: Option<&str>, daily: &str) -> Result<PathBuf, String> {
    match path {
        None => Ok(Palace::resolve()?.root().join(daily)),
        Some(p) => match p.strip_prefix('@') {
            Some(rest) => Ok(Palace::resolve()?.root().join(rest.trim_matches('/'))),
            None => {
                let cwd = env::current_dir().map_err(|e| format!("shot: {e}"))?;
                Ok(join_fs(&cwd, p))
            }
        },
    }
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

    #[test]
    fn body_header_and_text() {
        assert_eq!(build_body("isg 2026", Some("note"), true), "isg 2026\n\nnote\n");
    }

    #[test]
    fn body_header_only() {
        assert_eq!(build_body("isg 2026", None, true), "isg 2026\n");
    }

    #[test]
    fn body_text_only() {
        assert_eq!(build_body("isg 2026", Some("note"), false), "note\n");
    }

    #[test]
    fn body_empty_when_no_header_no_text() {
        assert_eq!(build_body("isg 2026", None, false), "");
    }
}
