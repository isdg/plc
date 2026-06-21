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
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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
        return list_recent(&note_dir);
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

/// List `*.md` notes newest-first (mtime desc, name asc on ties), one per line.
fn list_recent(note_dir: &Path) -> Result<String, String> {
    let mut entries: Vec<(String, SystemTime)> = fs::read_dir(note_dir)
        .map_err(|e| format!("murmur: {e}"))?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !name.ends_with(".md") {
                return None;
            }
            let mtime = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
            Some((name, mtime))
        })
        .collect();
    Ok(order_by_recency(&mut entries).join("\n"))
}

/// Sort entries newest-first, breaking mtime ties by name ascending, and
/// return just the names.
fn order_by_recency(entries: &mut [(String, SystemTime)]) -> Vec<String> {
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.iter().map(|(n, _)| n.clone()).collect()
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
    use std::time::Duration;

    fn t(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

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

    #[test]
    fn recency_newest_first() {
        let mut e = vec![
            ("old.md".to_string(), t(100)),
            ("new.md".to_string(), t(300)),
            ("mid.md".to_string(), t(200)),
        ];
        assert_eq!(order_by_recency(&mut e), ["new.md", "mid.md", "old.md"]);
    }

    #[test]
    fn recency_ties_break_by_name() {
        let mut e = vec![
            ("b.md".to_string(), t(100)),
            ("a.md".to_string(), t(100)),
        ];
        assert_eq!(order_by_recency(&mut e), ["a.md", "b.md"]);
    }
}
