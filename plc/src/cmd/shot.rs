//! `plc shot` — create a timestamped snapshot note and print its path.
//!
//! Writes `<%Y-%m-%dT%H.%M>.md`, tagged `shots`, into a directory chosen by
//! `-p/--path` (created if absent):
//!   * omitted        → the current directory
//!   * `@notes/inbox` → the vault root (needs `$PALACE_DIR`)
//!   * any other path → used as-is if absolute, else relative to the cwd
//!
//! Only `@` paths touch the vault; a plain `shot` needs no `$PALACE_DIR`.
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
    /// Target directory, created if absent. Default: the current directory.
    /// A leading `@` resolves against the vault root (e.g. `@notes/inbox`).
    #[arg(short = 'p', long = "path", value_name = "PATH")]
    path: Option<String>,
    /// Inline note body: written under the stamp, in place of the `[[shots]]`
    /// tag. Without this, `shot` seeds the usual tagged header.
    #[arg(short = 'i', long = "inline", value_name = "TEXT")]
    inline: Option<String>,
    /// Omit the stamp header. On its own creates an empty note; with `-i`,
    /// writes just the inline text.
    #[arg(long = "no-header")]
    no_header: bool,
}

pub fn run(args: ShotArgs) -> Result<String, String> {
    let now = Local::now();
    let dir = target_dir(args.path.as_deref())?;
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
