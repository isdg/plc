//! `plc todo` — manage week-based todo notes with a "last" pointer.
//!
//! Ports `dn()` (palace.zsh). Todo notes live in `notes/management/todo/`
//! named `todo-<YYYY-Www>.md`, the week starting on `week_start` (see
//! [`week::label`]). A pointer file at `<PALACE_DIR>/.plc/last-todo` records
//! the basename of the most recently used note.
//!
//!   `plc todo -n`         create this week's note, mark it last → its path
//!   `plc todo -l`         list notes, the last marked with `*`  → a list
//!   `plc todo -L NAME`    mark NAME (basename) last             → a notice
//!   `plc todo`, `--last`  resolve the last note                 → its path
//!
//! As with every command, `plc` only emits text; the zsh wrapper opens the
//! printed path (for `-n`, `--last` / no-arg) with `$EDITOR` and just echoes
//! the informational output of `-l` / `-L`.

use std::fs;
use std::path::Path;

use chrono::Local;
use clap::Args;
use plc_core::{rc, week};

use crate::config::Palace;
use crate::note;

pub(crate) const SUBDIR: &str = "notes/management/todo";
/// Pointer basename inside the vault's `.plc` dir.
const POINTER: &str = "last-todo";

#[derive(Args)]
pub struct TodoArgs {
    /// Create a new todo note for the current week and mark it last.
    #[arg(
        short = 'n',
        long = "new",
        conflicts_with_all = ["list", "mark", "last"]
    )]
    new: bool,
    /// List todo notes, the last marked with `*`.
    #[arg(short = 'l', long = "list", conflicts_with_all = ["mark", "last"])]
    list: bool,
    /// Mark NAME (basename) as the last todo note, without opening it.
    #[arg(
        short = 'L',
        long = "mark",
        value_name = "NAME",
        conflicts_with = "last"
    )]
    mark: Option<String>,
    /// Open the last todo note, as no flag does.
    #[arg(long = "last")]
    last: bool,
    #[command(flatten)]
    pub note: note::NoteArgs,
}

pub fn run(palace: &Palace, args: TodoArgs) -> Result<String, String> {
    let root = palace.root();
    let note_dir = root.join(SUBDIR);
    let pointer = palace.plc_dir().join(POINTER);
    fs::create_dir_all(&note_dir).map_err(|e| format!("todo: {e}"))?;

    if args.new {
        new_note(root, &pointer)
    } else if let Some(file) = args.mark {
        mark_last(&note_dir, &pointer, &file)
    } else if args.list {
        list_notes(&note_dir, &pointer)
    } else {
        open_last(&note_dir, &pointer)
    }
}

/// `-n`: create this week's note, point "last" at it, return its path.
fn new_note(root: &Path, pointer: &Path) -> Result<String, String> {
    let start = rc::week_start().map_err(|e| format!("todo: {e}"))?;
    let today = Local::now().date_naive();
    let name = format!("todo-{}.md", week::label(today, start));
    write_pointer(pointer, &name)?;
    note::ensure_note(root, SUBDIR, &name, "todo", None, note::SIGNATURE)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("todo: {e}"))
}

/// `-L NAME`: re-point "last" at an existing note, `.md` optional.
fn mark_last(
    note_dir: &Path,
    pointer: &Path,
    name: &str,
) -> Result<String, String> {
    let name = name.trim();
    let file = match name.ends_with(".md") {
        true => name.to_string(),
        false => format!("{name}.md"),
    };
    let target = note_dir.join(&file);
    if !target.is_file() {
        return Err(format!("todo: no such note: {}", target.display()));
    }
    write_pointer(pointer, &file)?;
    Ok(format!("Marked as last: {file}"))
}

/// `-l`: list `*.md` notes (byte-sorted), marking the "last" one with `*`.
fn list_notes(note_dir: &Path, pointer: &Path) -> Result<String, String> {
    let last = note::read_pointer(pointer);
    let mut names = md_names(note_dir).map_err(|e| format!("todo: {e}"))?;
    names.sort(); // byte order, matching `LC_ALL=C sort`
    Ok(format_list(&names, last.as_deref()))
}

/// no-arg: resolve the "last" note's path, erroring if unset or stale.
fn open_last(note_dir: &Path, pointer: &Path) -> Result<String, String> {
    let last = note::read_pointer(pointer).ok_or_else(|| {
        "todo: no last note recorded. Run 'plc todo -n' first.".to_string()
    })?;
    let target = note_dir.join(&last);
    if !target.is_file() {
        let at = target.display();
        return Err(format!("todo: stale pointer (no file at {at})"));
    }
    Ok(target.display().to_string())
}

/// Render the listing: `  * <name>` for the last note, `    <name>` otherwise.
fn format_list(names: &[String], last: Option<&str>) -> String {
    names
        .iter()
        .map(|n| {
            if Some(n.as_str()) == last {
                format!("  * {n}")
            } else {
                format!("    {n}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_pointer(pointer: &Path, name: &str) -> Result<(), String> {
    note::write_pointer(pointer, name).map_err(|e| format!("todo: {e}"))
}

/// The `*.md` file names in `dir`; none when it does not exist.
fn md_names(dir: &Path) -> std::io::Result<Vec<String>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    Ok(fs::read_dir(dir)?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".md"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_marks_last() {
        let names = vec!["todo-2026-W24.md".into(), "todo-2026-W25.md".into()];
        let out = format_list(&names, Some("todo-2026-W25.md"));
        assert_eq!(out, "    todo-2026-W24.md\n  * todo-2026-W25.md");
    }

    #[test]
    fn list_no_last() {
        let names = vec!["todo-2026-W24.md".to_string()];
        assert_eq!(format_list(&names, None), "    todo-2026-W24.md");
    }

    #[test]
    fn list_empty() {
        assert_eq!(format_list(&[], Some("x.md")), "");
    }
}
