//! `plc do` — manage week-based do-notes with a "last" pointer.
//!
//! Ports `dn()` (palace.zsh). Do-notes live in `notes/management/do/` named
//! `do-<%G-W%V>.md`. A pointer file at `<PALACE_DIR>/.last-do` records the
//! basename of the most recently used note.
//!
//!   `plc do -n`          create this ISO week's note, mark it last → prints path
//!   `plc do -l FILE`     re-point "last" at FILE (basename)     → prints confirmation
//!   `plc do -L`          list notes, last marked with `*`       → prints listing
//!   `plc do`             resolve the last note                  → prints path
//!
//! As with every command, `plc` only emits text; the zsh wrapper opens the
//! printed path (for `-n` / no-arg) with `$EDITOR` and just echoes the
//! informational output of `-l` / `-L`.

use std::fs;
use std::path::Path;

use chrono::Local;
use clap::Args;

use crate::config::Palace;
use crate::note;

const SUBDIR: &str = "notes/management/do";
const POINTER: &str = ".last-do";

#[derive(Args)]
pub struct DoArgs {
    /// Create a new do-note for the current ISO week and mark it last.
    #[arg(short = 'n', long = "new", conflicts_with_all = ["last", "list"])]
    new: bool,
    /// Mark FILE (basename) as the last do-note.
    #[arg(short = 'l', long = "last", value_name = "FILE", conflicts_with = "list")]
    last: Option<String>,
    /// List do-notes (last marked with `*`).
    #[arg(short = 'L', long = "list")]
    list: bool,
}

pub fn run(palace: &Palace, args: DoArgs) -> Result<String, String> {
    let root = palace.root();
    let note_dir = root.join(SUBDIR);
    let pointer = root.join(POINTER);
    fs::create_dir_all(&note_dir).map_err(|e| format!("do: {e}"))?;

    if args.new {
        new_note(root, &pointer)
    } else if let Some(file) = args.last {
        mark_last(&note_dir, &pointer, &file)
    } else if args.list {
        list_notes(&note_dir, &pointer)
    } else {
        open_last(&note_dir, &pointer)
    }
}

/// `-n`: create this ISO week's note, point "last" at it, return its path.
fn new_note(root: &Path, pointer: &Path) -> Result<String, String> {
    let name = Local::now().format("do-%G-W%V.md").to_string();
    write_pointer(pointer, &name)?;
    note::ensure_note(root, SUBDIR, &name, "do", None, note::SIGNATURE)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("do: {e}"))
}

/// `-l FILE`: re-point "last" at an existing note.
fn mark_last(note_dir: &Path, pointer: &Path, file: &str) -> Result<String, String> {
    let target = note_dir.join(file);
    if !target.is_file() {
        return Err(format!("do: no such note: {}", target.display()));
    }
    write_pointer(pointer, file)?;
    Ok(format!("Marked as last: {file}"))
}

/// `-L`: list `*.md` notes (byte-sorted), marking the "last" one with `*`.
fn list_notes(note_dir: &Path, pointer: &Path) -> Result<String, String> {
    let last = read_pointer(pointer);
    let mut names: Vec<String> = fs::read_dir(note_dir)
        .map_err(|e| format!("do: {e}"))?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".md"))
        .collect();
    names.sort(); // byte order, matching `LC_ALL=C sort`
    Ok(format_list(&names, last.as_deref()))
}

/// no-arg: resolve the "last" note's path, erroring if unset or stale.
fn open_last(note_dir: &Path, pointer: &Path) -> Result<String, String> {
    let last = read_pointer(pointer)
        .ok_or_else(|| "do: no last note recorded. Run 'plc do -n' first.".to_string())?;
    let target = note_dir.join(&last);
    if !target.is_file() {
        return Err(format!("do: stale pointer (no file at {})", target.display()));
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

/// Read the pointer's basename, trimming the trailing newline; `None` if the
/// file is absent or blank.
fn read_pointer(pointer: &Path) -> Option<String> {
    let s = fs::read_to_string(pointer).ok()?;
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn write_pointer(pointer: &Path, name: &str) -> Result<(), String> {
    fs::write(pointer, format!("{name}\n")).map_err(|e| format!("do: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_marks_last() {
        let names = vec!["do-2026-W24.md".to_string(), "do-2026-W25.md".to_string()];
        let out = format_list(&names, Some("do-2026-W25.md"));
        assert_eq!(out, "    do-2026-W24.md\n  * do-2026-W25.md");
    }

    #[test]
    fn list_no_last() {
        let names = vec!["do-2026-W24.md".to_string()];
        assert_eq!(format_list(&names, None), "    do-2026-W24.md");
    }

    #[test]
    fn list_empty() {
        assert_eq!(format_list(&[], Some("x.md")), "");
    }

    #[test]
    fn pointer_roundtrip_and_trim() {
        let dir = std::env::temp_dir().join(format!("plc-do-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(".last-do");

        assert_eq!(read_pointer(&p), None); // absent
        write_pointer(&p, "do-2026-W25.md").unwrap();
        assert_eq!(read_pointer(&p), Some("do-2026-W25.md".to_string())); // newline trimmed

        fs::write(&p, "   \n").unwrap();
        assert_eq!(read_pointer(&p), None); // blank → None

        fs::remove_dir_all(&dir).ok();
    }
}
