//! The vault as a DAG: every note a row, newest first; a note's parents
//! are the older notes it links to, in the order its text links them.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local, NaiveDate, TimeZone};

use super::layout::{layout, Line};
use crate::ledger::TIMESTAMP_FMT;
use crate::{fold_name, links, normalize_target, notes};

/// A note as read from disk: what [`Dag::build`] needs of it.
pub struct Source {
    pub path: PathBuf,
    pub text: String,
    pub modified: SystemTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub path: PathBuf,
    /// The file name without `.md`, as spelled on disk.
    pub name: String,
    /// `YYYY-MM-DD` of the note's stamp, file name or mtime.
    pub date: String,
    /// Rows of the older notes it links to, in text order.
    pub parents: Vec<usize>,
    /// Rows of the newer notes linking to it, newest first.
    pub children: Vec<usize>,
    /// Link names that are no note (tags, accounts, dangling), in text
    /// order, as first spelled.
    pub refs: Vec<String>,
    /// How many newer notes it links to; these draw no line.
    pub forward: usize,
    linked: bool,
}

impl Note {
    /// Whether any link joins it to another note, either way in time.
    pub fn is_linked(&self) -> bool {
        self.linked
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dag {
    /// Newest first: by when each note was written, then by path.
    pub notes: Vec<Note>,
}

impl Dag {
    /// Every note under `root` (see [`notes::walk`]). Unreadable or
    /// non-UTF-8 files are left out.
    pub fn scan(root: &Path) -> Self {
        let sources = notes::walk(root).filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            let modified = fs::metadata(&path).ok()?.modified().ok()?;
            Some(Source {
                path,
                text,
                modified,
            })
        });
        Self::build(sources)
    }

    pub fn build(sources: impl IntoIterator<Item = Source>) -> Self {
        let mut rows: Vec<(i64, String, Source)> = sources
            .into_iter()
            .filter(|s| notes::stem(&s.path).is_some())
            .map(|s| {
                let (when, date) = written(&s);
                (when, date, s)
            })
            .collect();
        rows.sort_by(|a, b| (b.0, &b.2.path).cmp(&(a.0, &a.2.path)));

        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, (_, _, s)) in rows.iter().enumerate() {
            let name = notes::stem(&s.path).unwrap_or_default();
            by_name.entry(fold_name(name)).or_default().push(i);
        }

        let mut out: Vec<Note> = Vec::with_capacity(rows.len());
        let mut linked_from_older = vec![false; rows.len()];
        for (i, (_, date, s)) in rows.iter().enumerate() {
            let mut parents = Vec::new();
            let mut newer = HashSet::new();
            let mut refs = Vec::new();
            let mut seen_refs = HashSet::new();
            for link in links(&s.text) {
                let name = link_name(link.inner);
                if name.is_empty() {
                    continue;
                }
                match by_name.get(&normalize_target(link.inner)) {
                    Some(rows) => {
                        for &j in rows.iter().filter(|&&j| j != i) {
                            if j > i {
                                if !parents.contains(&j) {
                                    parents.push(j);
                                }
                            } else if newer.insert(j) {
                                linked_from_older[j] = true;
                            }
                        }
                    }
                    None => {
                        if seen_refs.insert(fold_name(name)) {
                            refs.push(name.to_string());
                        }
                    }
                }
            }
            out.push(Note {
                path: s.path.clone(),
                name: notes::stem(&s.path).unwrap_or_default().to_string(),
                date: date.clone(),
                parents,
                children: Vec::new(),
                refs,
                forward: newer.len(),
                linked: false,
            });
        }
        for i in 0..out.len() {
            for k in 0..out[i].parents.len() {
                let p = out[i].parents[k];
                out[p].children.push(i);
            }
        }
        for (i, note) in out.iter_mut().enumerate() {
            note.linked = !note.parents.is_empty()
                || !note.children.is_empty()
                || note.forward > 0
                || linked_from_older[i];
        }
        Dag { notes: out }
    }

    /// The same graph without the notes [`Note::is_linked`] rejects.
    /// Nothing that remains loses a parent or a child.
    pub fn linked(&self) -> Self {
        let mut row = vec![usize::MAX; self.notes.len()];
        let mut kept = 0;
        for (i, note) in self.notes.iter().enumerate() {
            if note.linked {
                row[i] = kept;
                kept += 1;
            }
        }
        let remap = |rows: &[usize]| rows.iter().map(|&r| row[r]).collect();
        let notes = self
            .notes
            .iter()
            .filter(|n| n.linked)
            .map(|n| Note {
                parents: remap(&n.parents),
                children: remap(&n.children),
                ..n.clone()
            })
            .collect();
        Dag { notes }
    }

    /// The drawing, one [`Line`] per graph line; see [`layout`].
    pub fn lines(&self) -> Vec<Line> {
        let parents: Vec<Vec<usize>> =
            self.notes.iter().map(|n| n.parents.clone()).collect();
        layout(&parents)
    }
}

/// A link's target as written: before any `|alias`, `#heading` or
/// `^block`, trimmed.
fn link_name(inner: &str) -> &str {
    let end = inner.find(['|', '#', '^']).unwrap_or(inner.len());
    inner[..end].trim()
}

/// When a note was written, as Unix seconds plus the `YYYY-MM-DD` shown:
/// its first line's `<signature> <stamp>`, else a date in its file name
/// (local midnight), else its mtime.
fn written(s: &Source) -> (i64, String) {
    if let Some(at) = stamp(&s.text) {
        return (at.timestamp(), at.format("%Y-%m-%d").to_string());
    }
    let name = notes::stem(&s.path).unwrap_or_default();
    if let Some(day) = name_date(name) {
        let midnight = day.and_hms_opt(0, 0, 0).unwrap_or_default();
        let at = Local
            .from_local_datetime(&midnight)
            .earliest()
            .map_or(midnight.and_utc().timestamp(), |t| t.timestamp());
        return (at, day.format("%Y-%m-%d").to_string());
    }
    let at: DateTime<Local> = s.modified.into();
    (at.timestamp(), at.format("%Y-%m-%d").to_string())
}

fn stamp(text: &str) -> Option<DateTime<chrono::FixedOffset>> {
    let first = text.lines().next()?;
    let mut tokens = first.split_whitespace().skip(1);
    let stamp = [tokens.next()?, tokens.next()?, tokens.next()?].join(" ");
    DateTime::parse_from_str(&stamp, TIMESTAMP_FMT).ok()
}

fn name_date(name: &str) -> Option<NaiveDate> {
    let bytes = name.as_bytes();
    (0..bytes.len().saturating_sub(9)).find_map(|i| {
        let s = name.get(i..i + 10)?;
        let shaped = s.bytes().enumerate().all(|(k, b)| match k {
            4 | 7 => b == b'-',
            _ => b.is_ascii_digit(),
        });
        shaped
            .then(|| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn src(path: &str, text: &str) -> Source {
        Source {
            path: PathBuf::from(path),
            text: text.to_string(),
            modified: UNIX_EPOCH + Duration::from_secs(1_000_000_000),
        }
    }

    fn stamped(day: u32, body: &str) -> String {
        format!("isg 2026-01-{day:02} 10:00:00 +0200\n\n{body}")
    }

    fn names(dag: &Dag, rows: &[usize]) -> Vec<String> {
        rows.iter().map(|&r| dag.notes[r].name.clone()).collect()
    }

    fn row(dag: &Dag, name: &str) -> usize {
        dag.notes.iter().position(|n| n.name == name).unwrap()
    }

    #[test]
    fn rows_are_newest_first_by_stamp_name_then_mtime() {
        let dag = Dag::build([
            src("/v/old.md", "no stamp"),
            src("/v/2026-01-05.md", "no stamp"),
            src("/v/b.md", &stamped(9, "")),
            src("/v/a.md", &stamped(9, "")),
            src("/v/c.md", "[isg 2026-01-07 10:00:00 +0200"),
        ]);
        let order: Vec<_> = dag.notes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(order, ["b", "a", "c", "2026-01-05", "old"]);
        let dates: Vec<_> = dag.notes.iter().map(|n| n.date.as_str()).collect();
        assert_eq!(dates[..4], ["2026-01-09", "2026-01-09", "2026-01-07",
            "2026-01-05"]);
    }

    #[test]
    fn parents_are_older_links_in_text_order() {
        let dag = Dag::build([
            src("/v/new.md", &stamped(9, "[[b]] [[later]] [[a|A]] [[b#x]]")),
            src("/v/a.md", &stamped(1, "")),
            src("/v/b.md", &stamped(2, "")),
            src("/v/later.md", &stamped(10, "")),
        ]);
        let new = &dag.notes[row(&dag, "new")];
        assert_eq!(names(&dag, &new.parents), ["b", "a"]);
        assert_eq!(new.forward, 1);
        let b = &dag.notes[row(&dag, "b")];
        assert_eq!(names(&dag, &b.children), ["new"]);
    }

    #[test]
    fn children_are_newest_first() {
        let dag = Dag::build([
            src("/v/hub.md", &stamped(1, "")),
            src("/v/x.md", &stamped(3, "[[hub]]")),
            src("/v/y.md", &stamped(5, "[[HUB]]")),
        ]);
        let hub = &dag.notes[row(&dag, "hub")];
        assert_eq!(names(&dag, &hub.children), ["y", "x"]);
    }

    #[test]
    fn refs_are_names_without_a_note() {
        let dag = Dag::build([src(
            "/v/n.md",
            &stamped(1, "[[revolut]] [[food/groceries|f]] [[Revolut]] \
             [[#top]] [[n]] [[gone^b1]]"),
        )]);
        assert_eq!(dag.notes[0].refs, ["revolut", "food/groceries", "gone"]);
        assert!(dag.notes[0].parents.is_empty());
        assert!(!dag.notes[0].is_linked());
    }

    #[test]
    fn names_meet_across_case_and_normal_forms() {
        let nfd = "\u{434}\u{430}\u{440}\u{438}\u{306}";
        let dag = Dag::build([
            src("/v/new.md", &stamped(9, "[[ДАРЙ]]")),
            src(&format!("/v/{nfd}.md"), &stamped(1, "")),
        ]);
        assert_eq!(dag.notes[0].parents, [1]);
    }

    #[test]
    fn duplicate_names_link_every_note() {
        let dag = Dag::build([
            src("/v/new.md", &stamped(9, "[[twin]]")),
            src("/v/a/twin.md", &stamped(1, "")),
            src("/v/b/twin.md", &stamped(2, "")),
        ]);
        assert_eq!(dag.notes[0].parents.len(), 2);
    }

    #[test]
    fn linked_drops_only_unlinked_notes() {
        let dag = Dag::build([
            src("/v/lone.md", &stamped(8, "[[tag]]")),
            src("/v/new.md", &stamped(7, "[[old]]")),
            src("/v/index.md", &stamped(1, "[[late]]")),
            src("/v/late.md", &stamped(6, "")),
            src("/v/old.md", &stamped(2, "")),
        ]);
        let kept = dag.linked();
        let order: Vec<_> = kept.notes.iter().map(|n| &n.name).collect();
        assert_eq!(order, ["new", "late", "old", "index"]);
        assert_eq!(names(&kept, &kept.notes[0].parents), ["old"]);
        assert_eq!(names(&kept, &kept.notes[2].children), ["new"]);
    }

    #[test]
    fn lines_draw_the_rows() {
        let dag = Dag::build([
            src("/v/b.md", &stamped(2, "[[a]]")),
            src("/v/a.md", &stamped(1, "")),
        ]);
        let graph: Vec<_> = dag.lines().into_iter().map(|l| l.graph).collect();
        assert_eq!(graph, ["* ", "o "]);
    }
}
