//! `plc isg` — enumerated "isg" notes under `notes/me/writing/isg/`.
//!
//! Like murmur notes, but auto-named with a monotonically increasing index
//! assigned at creation: `isg0.md`, `isg1.md`, … `isg52.md`. The next index is
//! one past the highest any note carries, postfixed or followed up: a gap is
//! never refilled, though deleting every note of the top index frees it.
//!
//! A note can be *followed up* with letter suffixes: `isg52a`, `isg52b`, … are
//! follow-ups of `isg52`. Letters use a bijective base-26 sequence
//! (`a`…`z`, `aa`, `ab`, …) so follow-ups never run out.
//!
//!   `plc isg`, `--last`        open the last note
//!   `plc isg -n` / `--new`     create the next enumerated note → prints path
//!   `plc isg -nf [N]`          follow up note N (or the last) → isg<N><letter>
//!   `plc isg -l` / `--list`    list notes newest-first → zsh pipes to fzf
//!   `plc isg -L NAME`          mark NAME last without opening it
//!   `plc isg NAME`             resolve/open an existing note by basename
//!
//! Every note plc creates or opens becomes the last, recorded in the vault's
//! `.plc/last-isg` as `do` records its own. Without a pointer, or with a
//! stale one, the last note is the most recently modified.

use std::fs;
use std::path::Path;

use clap::Args;

use crate::config::Palace;
use crate::note;

const SUBDIR: &str = "notes/me/writing/isg";
const TAG: &str = "isg";
/// Pointer basename inside the vault's `.plc` dir.
const POINTER: &str = "last-isg";

#[derive(Args)]
pub struct IsgArgs {
    /// List isg notes newest-first (zsh pipes this through fzf).
    #[arg(
        short = 'l',
        long = "list",
        conflicts_with_all = ["new", "name", "mark", "last"]
    )]
    list: bool,
    /// Mark NAME (basename) as the last isg note, without opening it.
    #[arg(
        short = 'L',
        long = "mark",
        value_name = "NAME",
        conflicts_with_all = ["new", "name", "last"]
    )]
    mark: Option<String>,
    /// Open the last isg note, as no flag does.
    #[arg(long = "last", conflicts_with_all = ["new", "name"])]
    last: bool,
    /// Create the next enumerated note (isg0, isg1, …).
    #[arg(short = 'n', long = "new", conflicts_with = "name")]
    new: bool,
    /// With --new: follow up note INDEX (or the last note's) with the next
    /// letter suffix → isg<INDEX>a, isg<INDEX>b, …
    #[arg(
        short = 'f',
        long = "follow",
        value_name = "INDEX",
        num_args = 0..=1,
        requires = "new"
    )]
    follow: Option<Option<u64>>,
    /// Open an existing note by basename (".md" appended if missing).
    #[arg(value_name = "NAME")]
    name: Option<String>,
    #[command(flatten)]
    pub note: note::NoteArgs,
}

pub fn run(palace: &Palace, args: IsgArgs) -> Result<String, String> {
    let note_dir = palace.root().join(SUBDIR);
    fs::create_dir_all(&note_dir).map_err(|e| format!("isg: {e}"))?;

    // --list: emit the machine-readable listing (zsh pipes it through fzf).
    if args.list {
        return note::list_md_by_recency(&note_dir)
            .map(|v| v.join("\n"))
            .map_err(|e| format!("isg: {e}"));
    }

    let names = note::list_md_by_recency(&note_dir).map_err(|e| format!("isg: {e}"))?;
    let pointer = palace.plc_dir().join(POINTER);
    if let Some(name) = args.mark {
        let file = with_md(name.trim())?;
        if !note_dir.join(&file).is_file() {
            return Err(format!("isg: no such note: {file}"));
        }
        note::write_pointer(&pointer, &file).map_err(|e| format!("isg: {e}"))?;
        return Ok(format!("Marked as last: {file}"));
    }
    let last = last_note(&note_dir, &pointer, &names);

    let filename = if let Some(which) = args.follow {
        // -nf [INDEX]: follow up note INDEX (or the last note's) with a letter.
        let base = match which {
            Some(n) => n,
            None => last
                .as_deref()
                .and_then(base_of)
                .ok_or_else(|| "isg: no note to follow up".to_string())?,
        };
        next_continuation(&names, base)
    } else if args.new {
        // -n/--new: next enumerated note.
        format!("isg{}.md", next_index(&names))
    } else if let Some(n) = args.name {
        // NAME: open an existing note by basename.
        with_md(n.trim())?
    } else {
        last.ok_or_else(|| "isg: no isg notes yet — create one with `plc isg -n`".to_string())?
    };
    // isg notes lead their header with their own index, e.g. `isg20 <date>`.
    let stem = filename.strip_suffix(".md").unwrap_or(&filename);
    let path = note::ensure_note(palace.root(), SUBDIR, &filename, TAG, None, stem)
        .map_err(|e| format!("isg: {e}"))?;
    // The written name, not `filename`: a run's --postfix renames the file.
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        note::write_pointer(&pointer, name).map_err(|e| format!("isg: {e}"))?;
    }
    Ok(path.display().to_string())
}

/// The last note: the one the pointer names while it exists, else the most
/// recently modified.
fn last_note(note_dir: &Path, pointer: &Path, names: &[String]) -> Option<String> {
    note::read_pointer(pointer)
        .filter(|n| note_dir.join(n).is_file())
        .or_else(|| names.first().cloned())
}

/// The next isg index: one past the highest index of any note, or 0 if none.
fn next_index(names: &[String]) -> u64 {
    names
        .iter()
        .filter_map(|n| base_of(n))
        .max()
        .map_or(0, |m| m + 1)
}

/// The index a note belongs to: 52 for `isg52.md`, `isg52b.md` and
/// `isg52+review.md`. `None` for anything else.
fn base_of(name: &str) -> Option<u64> {
    let stem = name.strip_suffix(".md")?.strip_prefix("isg")?;
    let stem = stem.split('+').next().unwrap_or(stem);
    let split = stem
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(stem.len());
    let (digits, letters) = stem.split_at(split);
    if !letters.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    digits.parse().ok()
}

/// The next continuation filename for `base`: `isg<base><letter>.md`, where the
/// letter is one past the highest existing continuation of `base` (or `a`).
fn next_continuation(names: &[String], base: u64) -> String {
    let max = names
        .iter()
        .filter_map(|n| continuation_suffix(n, base))
        .filter_map(|s| letters_to_num(&s))
        .max();
    let next = num_to_letters(max.map_or(1, |m| m + 1));
    format!("isg{base}{next}.md")
}

/// If `name` is a continuation of `base` (`isg<base><letters>.md` with one or
/// more lowercase letters), return its letter suffix.
fn continuation_suffix(name: &str, base: u64) -> Option<String> {
    let stem = name.strip_suffix(".md")?.strip_prefix("isg")?;
    let split = stem.find(|c: char| !c.is_ascii_digit()).unwrap_or(stem.len());
    let (digits, letters) = stem.split_at(split);
    if digits.parse::<u64>().ok()? != base || letters.is_empty() {
        return None;
    }
    letters
        .chars()
        .all(|c| c.is_ascii_lowercase())
        .then(|| letters.to_string())
}

/// Bijective base-26: "a"→1, "z"→26, "aa"→27. `None` if empty or non-lowercase.
fn letters_to_num(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut n = 0u64;
    for c in s.chars() {
        if !c.is_ascii_lowercase() {
            return None;
        }
        n = n * 26 + (c as u64 - 'a' as u64 + 1);
    }
    Some(n)
}

/// Inverse of [`letters_to_num`]: 1→"a", 26→"z", 27→"aa".
fn num_to_letters(mut n: u64) -> String {
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    out.iter().rev().collect()
}

/// Append `.md` to a basename when absent. Errors on an empty name.
fn with_md(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("isg: empty name".to_string());
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

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn next_index_empty_is_zero() {
        assert_eq!(next_index(&[]), 0);
    }

    #[test]
    fn next_index_is_max_plus_one() {
        assert_eq!(next_index(&s(&["isg0.md", "isg1.md", "isg2.md"])), 3);
    }

    #[test]
    fn next_index_respects_gaps() {
        // Highest is 52 even with holes → 53 (never reuse a deleted index).
        assert_eq!(next_index(&s(&["isg3.md", "isg52.md"])), 53);
    }

    #[test]
    fn next_index_ignores_non_isg_and_malformed() {
        let names = s(&["foo.md", "isg.md", "isgX.md", "isg7.md", "notes.txt"]);
        assert_eq!(next_index(&names), 8);
    }

    #[test]
    fn next_index_counts_postfixed_and_follow_ups() {
        assert_eq!(next_index(&s(&["isg1.md", "isg2+review.md"])), 3);
        // isg4 itself deleted, its follow-up kept: 4 stays taken.
        assert_eq!(next_index(&s(&["isg3.md", "isg4b.md"])), 5);
    }

    #[test]
    fn with_md_appends_and_rejects_empty() {
        assert_eq!(with_md("isg5").unwrap(), "isg5.md");
        assert_eq!(with_md("isg5.md").unwrap(), "isg5.md");
        assert!(with_md("").is_err());
    }

    #[test]
    fn bijective_base26_roundtrip() {
        for (n, s) in [(1, "a"), (26, "z"), (27, "aa"), (52, "az"), (703, "aaa")] {
            assert_eq!(num_to_letters(n), s);
            assert_eq!(letters_to_num(s), Some(n));
        }
    }

    #[test]
    fn first_continuation_is_a() {
        // No continuations of 52 yet → isg52a.
        assert_eq!(next_continuation(&s(&["isg52.md", "isg0.md"]), 52), "isg52a.md");
    }

    #[test]
    fn continuation_advances_letter() {
        let names = s(&["isg52.md", "isg52a.md", "isg52b.md"]);
        assert_eq!(next_continuation(&names, 52), "isg52c.md");
    }

    #[test]
    fn continuation_wraps_to_double_letters() {
        let names = s(&["isg52z.md"]);
        assert_eq!(next_continuation(&names, 52), "isg52aa.md");
    }

    #[test]
    fn continuation_is_per_base() {
        // isg5's continuations must not be confused with isg52's.
        let names = s(&["isg5a.md", "isg52a.md", "isg52b.md"]);
        assert_eq!(next_continuation(&names, 5), "isg5b.md");
        assert_eq!(next_continuation(&names, 52), "isg52c.md");
    }

    #[test]
    fn continuation_suffix_parsing() {
        assert_eq!(continuation_suffix("isg52a.md", 52), Some("a".to_string()));
        assert_eq!(continuation_suffix("isg52.md", 52), None); // base, not a continuation
        assert_eq!(continuation_suffix("isg5a.md", 52), None); // wrong base
        assert_eq!(continuation_suffix("isg52A.md", 52), None); // uppercase rejected
    }

    #[test]
    fn base_of_cases() {
        assert_eq!(base_of("isg52.md"), Some(52));
        assert_eq!(base_of("isg52b.md"), Some(52));
        assert_eq!(base_of("isg52aa.md"), Some(52));
        assert_eq!(base_of("isg52+review.md"), Some(52));
        assert_eq!(base_of("isg52b+review.md"), Some(52));
        assert_eq!(base_of("isg.md"), None);
        assert_eq!(base_of("isgfoo.md"), None);
        assert_eq!(base_of("isg52B.md"), None);
        assert_eq!(base_of("foo.md"), None);
    }
}
