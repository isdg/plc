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
//!
//! The notes were do-notes once: `do-*.md` under `notes/management/do`, the
//! pointer `last-do`, the tag `[[do]]`. [`doctor`] renames a vault's, and
//! `plc todo` refuses to run until it has.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use clap::Args;
use plc_core::{fold_name, notes, rc, rename_links, week};

use crate::config::Palace;
use crate::note;

const SUBDIR: &str = "notes/management/todo";
/// Pointer basename inside the vault's `.plc` dir.
const POINTER: &str = "last-todo";

/// Where do-notes lived, relative to the vault root.
const OLD_SUBDIR: &str = "notes/management/do";
/// The do-note pointer inside `.plc`, and its older place at the vault root.
const OLD_POINTER: &str = "last-do";
const OLDER_POINTER: &str = ".last-do";

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
    if old_layout(palace) {
        return Err(format!(
            "todo: do-notes are still under {OLD_SUBDIR} or named by \
             last-do; run `plc doctor --fix` to rename them to todo"
        ));
    }
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

/// Whether the vault still has a do-note folder or pointer.
fn old_layout(palace: &Palace) -> bool {
    let root = palace.root();
    root.join(OLD_SUBDIR).exists()
        || palace.plc_dir().join(OLD_POINTER).exists()
        || root.join(OLDER_POINTER).exists()
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

/// Every entry's name in `dir`, files or not; none when it does not exist.
fn entry_names(dir: &Path) -> std::io::Result<Vec<String>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    Ok(fs::read_dir(dir)?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect())
}

/// `do-…` as `todo-…`, keeping the rest as written; anything else as is.
fn renamed(name: &str) -> String {
    match fold_name(name).starts_with("do-") || fold_name(name) == "do.md" {
        true => format!("todo{}", &name[2..]),
        false => name.to_string(),
    }
}

/// What renaming a vault's do-notes to todo notes takes.
#[derive(Default)]
struct Plan {
    /// Notes to move, from → to.
    moves: Vec<(PathBuf, PathBuf)>,
    /// Targets that already exist, which stop every move.
    conflicts: Vec<PathBuf>,
    /// The do pointers to remove, and what `last-todo` should then hold.
    pointers: Vec<PathBuf>,
    last: Option<String>,
    /// Notes whose links to do-notes change, with their new text and count.
    links: Vec<(PathBuf, String, usize)>,
}

impl Plan {
    fn is_empty(&self) -> bool {
        self.moves.is_empty()
            && self.pointers.is_empty()
            && self.links.is_empty()
    }
}

fn plan(palace: &Palace) -> Result<Plan, String> {
    let root = palace.root();
    let err = |e: std::io::Error| format!("todo doctor: {e}");
    let (old_dir, new_dir) = (root.join(OLD_SUBDIR), root.join(SUBDIR));
    let mut plan = Plan::default();
    for name in entry_names(&old_dir).map_err(err)? {
        plan.moves.push((old_dir.join(&name), new_dir.join(renamed(&name))));
    }
    for name in md_names(&new_dir).map_err(err)? {
        if renamed(&name) != name {
            let to = new_dir.join(renamed(&name));
            plan.moves.push((new_dir.join(&name), to));
        }
    }
    for path in notes::walk(root).filter(|p| !p.starts_with(&old_dir)) {
        let is_do = notes::stem(&path).is_some_and(|s| fold_name(s) == "do");
        if is_do {
            plan.moves.push((path.clone(), path.with_file_name("todo.md")));
        }
    }
    plan.conflicts = plan
        .moves
        .iter()
        .filter(|(from, to)| to != from && to.exists())
        .map(|(_, to)| to.clone())
        .collect();

    let pointers =
        [palace.plc_dir().join(OLD_POINTER), root.join(OLDER_POINTER)];
    plan.pointers = pointers.into_iter().filter(|p| p.is_file()).collect();
    let current = note::read_pointer(&palace.plc_dir().join(POINTER));
    let old = plan.pointers.iter().find_map(|p| note::read_pointer(p));
    if current.is_none() {
        plan.last = old.map(|name| renamed(&name));
    }

    let stems: HashSet<String> = plan
        .moves
        .iter()
        .filter_map(|(from, _)| notes::stem(from).map(fold_name))
        .chain(["do".to_string()])
        .collect();
    let rename = |name: &str| {
        let folded = fold_name(name);
        let renames = stems.contains(&folded) && folded.starts_with("do");
        renames.then(|| format!("todo{}", &name[2..]))
    };
    for path in notes::walk(root) {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let (new, n) = rename_links(&text, rename);
        if n > 0 {
            plan.links.push((path, new, n));
        }
    }
    Ok(plan)
}

fn apply(palace: &Palace, plan: &Plan) -> Result<(), String> {
    let err = |e: std::io::Error| format!("todo doctor: {e}");
    for (from, to) in &plan.moves {
        if let Some(dir) = to.parent() {
            fs::create_dir_all(dir).map_err(err)?;
        }
        fs::rename(from, to).map_err(err)?;
    }
    let old_dir = palace.root().join(OLD_SUBDIR);
    if old_dir.is_dir() {
        fs::remove_dir(&old_dir).map_err(err)?;
    }
    if let Some(last) = &plan.last {
        write_pointer(&palace.plc_dir().join(POINTER), last)?;
    }
    for pointer in &plan.pointers {
        fs::remove_file(pointer).map_err(err)?;
    }
    for (path, text, _) in &plan.links {
        let moved = plan.moves.iter().find(|(from, _)| from == path);
        let path = moved.map_or(path, |(_, to)| to);
        fs::write(path, text).map_err(err)?;
    }
    Ok(())
}

/// `plc doctor`'s todo section: the do-notes, pointer and links still named
/// the old way, renamed with `fix` unless a target already exists.
pub fn doctor(palace: &Palace, fix: bool) -> Result<String, String> {
    let plan = plan(palace)?;
    if plan.is_empty() {
        return Ok("\n  Todo — all good  OK".to_string());
    }
    let root = palace.root();
    let rel =
        |p: &Path| p.strip_prefix(root).unwrap_or(p).display().to_string();
    let mut out = vec![
        String::new(),
        "  Todo — do-notes to rename".to_string(),
        String::new(),
    ];
    if !plan.moves.is_empty() {
        out.push(format!("  ! {} note(s) to rename:", plan.moves.len()));
        for (from, to) in &plan.moves {
            out.push(format!("      {} → {}", rel(from), rel(to)));
        }
    }
    if !plan.pointers.is_empty() {
        let names: Vec<String> = plan.pointers.iter().map(|p| rel(p)).collect();
        out.push(format!("  ! {} → .plc/{POINTER}", names.join(", ")));
    }
    if !plan.links.is_empty() {
        let total: usize = plan.links.iter().map(|(_, _, n)| n).sum();
        let notes = plan.links.len();
        out.push(format!("  ! {total} link(s) to rename in {notes} note(s):"));
        for (path, _, n) in &plan.links {
            out.push(format!("      {} ({n})", rel(path)));
        }
    }
    for to in &plan.conflicts {
        out.push(format!("  ! {} already exists", rel(to)));
    }
    out.push(String::new());
    if !plan.conflicts.is_empty() {
        let first = "  nothing renamed: move or merge what exists first";
        out.push(first.to_string());
    } else if fix {
        apply(palace, &plan)?;
        let done = "  fixed: renamed the do-notes, pointer and links";
        out.push(done.to_string());
    } else {
        out.push("  run `plc doctor --fix` to rename them".to_string());
    }
    Ok(out.join("\n"))
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

    #[test]
    fn do_names_become_todo_names() {
        assert_eq!(renamed("do-2026-W25.md"), "todo-2026-W25.md");
        assert_eq!(renamed("Do-2026-W25.md"), "todo-2026-W25.md");
        assert_eq!(renamed("do.md"), "todo.md");
        assert_eq!(renamed("dog.md"), "dog.md");
        assert_eq!(renamed("todo-2026-W25.md"), "todo-2026-W25.md");
    }
}
