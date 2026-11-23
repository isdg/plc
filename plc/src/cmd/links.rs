//! `plc doctor`'s links section: note names hold no whitespace.
//!
//! Every run of whitespace in a `[[link]]` target, and in the name of a
//! file the links could reach, becomes one [`SEPARATOR`]. A path, `|alias`,
//! `#heading` or `^block` stays as written.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use plc_core::{notes, rename_links};

use crate::config::Palace;

/// Not `+`, which already joins a note to its `--postfix`.
const SEPARATOR: char = '_';

/// `name` with each whitespace run made one [`SEPARATOR`], when it has any.
/// A line break or `[` means an unclosed `[[` swallowed prose: not a name.
fn unspaced(name: &str) -> Option<String> {
    let broken = name.contains(['\n', '\r', '[']);
    if broken || !name.contains(char::is_whitespace) {
        return None;
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    Some(words.join(&SEPARATOR.to_string()))
}

#[derive(Default)]
struct Plan {
    /// Files to move, from → to.
    moves: Vec<(PathBuf, PathBuf)>,
    /// Targets that exist or that two moves share, which stop every move.
    conflicts: Vec<PathBuf>,
    /// Notes whose links change, with their new text and count.
    links: Vec<(PathBuf, String, usize)>,
    /// Each link target renamed, from → to.
    names: BTreeSet<(String, String)>,
}

impl Plan {
    fn is_empty(&self) -> bool {
        self.moves.is_empty() && self.links.is_empty()
    }
}

fn plan(root: &Path) -> Plan {
    let mut plan = Plan::default();
    for path in notes::files(root) {
        let name = path.file_name().and_then(|n| n.to_str());
        if let Some(new) = name.and_then(unspaced) {
            plan.moves.push((path.clone(), path.with_file_name(new)));
        }
    }
    let mut taken = HashSet::new();
    plan.conflicts = plan
        .moves
        .iter()
        .filter(|(_, to)| to.exists() || !taken.insert(to.clone()))
        .map(|(_, to)| to.clone())
        .collect();

    for path in notes::walk(root) {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let mut names = Vec::new();
        let (new, n) = rename_links(&text, |name| {
            let new = unspaced(name)?;
            names.push((name.to_string(), new.clone()));
            Some(new)
        });
        if n > 0 {
            plan.names.extend(names);
            plan.links.push((path, new, n));
        }
    }
    plan
}

fn apply(plan: &Plan) -> Result<(), String> {
    let err = |e: std::io::Error| format!("links doctor: {e}");
    for (from, to) in &plan.moves {
        fs::rename(from, to).map_err(err)?;
    }
    for (path, text, _) in &plan.links {
        let moved = plan.moves.iter().find(|(from, _)| from == path);
        let path = moved.map_or(path, |(_, to)| to);
        fs::write(path, text).map_err(err)?;
    }
    Ok(())
}

/// `plc doctor`'s links section: the files and link targets whose names
/// hold whitespace, renamed with `fix` unless a new name is already taken.
pub fn doctor(palace: &Palace, fix: bool) -> Result<String, String> {
    let root = palace.root();
    let mut plan = plan(root);
    if plan.is_empty() {
        return Ok("\n  Links — all good  OK".to_string());
    }
    let rel =
        |p: &Path| p.strip_prefix(root).unwrap_or(p).display().to_string();
    let mut out = vec![
        String::new(),
        "  Links — whitespace in names".to_string(),
        String::new(),
    ];
    if !plan.moves.is_empty() {
        plan.moves.sort();
        out.push(format!("  ! {} file(s) to rename:", plan.moves.len()));
        for (from, to) in &plan.moves {
            out.push(format!("      {} → {}", rel(from), rel(to)));
        }
    }
    if !plan.links.is_empty() {
        let total: usize = plan.links.iter().map(|(_, _, n)| n).sum();
        let notes = plan.links.len();
        out.push(format!("  ! {total} link(s) to rewrite in {notes} note(s):"));
        for (from, to) in &plan.names {
            out.push(format!("      [[{from}]] → [[{to}]]"));
        }
    }
    for to in &plan.conflicts {
        out.push(format!("  ! {} already taken", rel(to)));
    }
    out.push(String::new());
    if !plan.conflicts.is_empty() {
        let first = "  nothing renamed: move or merge what is taken first";
        out.push(first.to_string());
    } else if fix {
        apply(&plan)?;
        out.push("  fixed: renamed the files and links".to_string());
    } else {
        out.push("  run `plc doctor --fix` to rename them".to_string());
    }
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_runs_become_one_separator() {
        assert_eq!(unspaced("foo bar").as_deref(), Some("foo_bar"));
        assert_eq!(unspaced("a  b\tc").as_deref(), Some("a_b_c"));
        assert_eq!(unspaced("Дар  речи.md").as_deref(), Some("Дар_речи.md"));
        assert_eq!(unspaced("nb\u{a0}sp").as_deref(), Some("nb_sp"));
        assert_eq!(unspaced("foo_bar"), None);
    }

    #[test]
    fn swallowed_prose_is_no_name() {
        assert_eq!(unspaced("todo\n- [[x"), None);
        assert_eq!(unspaced("a [[b"), None);
    }
}
