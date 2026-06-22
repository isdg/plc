//! `plc murmur` — manage free-form "murmur" notes.
//!
//! Ports the file half of `tg()` (palace.zsh). Notes live in
//! `notes/me/writing/murmur/`.
//!
//!   `plc murmur -l`          list notes newest-first → zsh pipes to fzf
//!   `plc murmur -n NAME`     create/resolve NAME (".md" appended) → prints path
//!   `plc murmur NAME`        same, positional form
//!
//! The interactive name prompt and the fzf picker stay in the zsh wrapper;
//! this command is non-interactive and only emits text.

use std::fs;

use clap::Args;

use crate::config::Palace;
use crate::note;

const SUBDIR: &str = "notes/me/writing/murmur";

#[derive(Args)]
pub struct MurmurArgs {
    /// List murmur notes newest-first (zsh pipes this through fzf).
    #[arg(short = 'l', long = "list")]
    list: bool,
    /// Name of the note to create/resolve (".md" appended if missing).
    #[arg(short = 'n', long = "new", value_name = "NAME")]
    name: Option<String>,
    /// Positional note name (alternative to -n NAME).
    #[arg(value_name = "NAME", conflicts_with = "name")]
    positional: Option<String>,
}

pub fn run(palace: &Palace, args: MurmurArgs) -> Result<String, String> {
    let note_dir = palace.root().join(SUBDIR);
    fs::create_dir_all(&note_dir).map_err(|e| format!("murmur: {e}"))?;

    if args.list {
        return note::list_md_by_recency(&note_dir)
            .map(|v| v.join("\n"))
            .map_err(|e| format!("murmur: {e}"));
    }

    let name = args
        .name
        .or(args.positional)
        .ok_or_else(|| "murmur: a note name is required (or pass -l to list)".to_string())?;
    let filename = ensure_md(name.trim())?;
    note::ensure_note(palace.root(), SUBDIR, &filename, "murmur", None)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("murmur: {e}"))
}

/// Append `.md` to a note name when absent. Errors on an empty name.
fn ensure_md(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("murmur: empty name".to_string());
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
    fn ensure_md_appends() {
        assert_eq!(ensure_md("foo").unwrap(), "foo.md");
    }

    #[test]
    fn ensure_md_keeps_extension() {
        assert_eq!(ensure_md("foo.md").unwrap(), "foo.md");
    }

    #[test]
    fn ensure_md_rejects_empty() {
        assert!(ensure_md("").is_err());
    }
}
