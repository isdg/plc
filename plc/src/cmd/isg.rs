//! `plc isg` — enumerated "isg" notes under `notes/me/writing/isg/`.
//!
//! Like murmur notes, but auto-named with a monotonically increasing index
//! assigned at creation: `isg0.md`, `isg1.md`, … `isg52.md`. The next index is
//! one past the highest existing `isg<N>.md` (so deletions never cause reuse).
//!
//! A note can be *continued* with letter suffixes: `isg52a`, `isg52b`, … are
//! continuations of `isg52`. Letters use a bijective base-26 sequence
//! (`a`…`z`, `aa`, `ab`, …) so continuations never run out.
//!
//!   `plc isg`         create the next enumerated note → prints path
//!   `plc isg -l`      list notes newest-first → zsh pipes to fzf
//!   `plc isg NAME`    resolve/open an existing note by basename (fzf-pick reopen)
//!   `plc isg -c [N]`  continue note N (or the latest if N omitted) → isg<N><letter>

use std::fs;

use clap::Args;

use crate::config::Palace;
use crate::note;

const SUBDIR: &str = "notes/me/writing/isg";
const TAG: &str = "isg";

#[derive(Args)]
pub struct IsgArgs {
    /// List isg notes newest-first (zsh pipes this through fzf).
    #[arg(short = 'l', long = "list", conflicts_with_all = ["cont", "name"])]
    list: bool,
    /// Continue note INDEX with the next letter suffix (isg<INDEX><letter>).
    /// Omit INDEX to continue the latest note.
    #[arg(short = 'c', long = "continue", value_name = "INDEX", num_args = 0..=1, conflicts_with = "name")]
    cont: Option<Option<u64>>,
    /// Open an existing note by basename (".md" appended if missing). Without
    /// it, the next enumerated note is created.
    #[arg(value_name = "NAME")]
    name: Option<String>,
}

pub fn run(palace: &Palace, args: IsgArgs) -> Result<String, String> {
    let note_dir = palace.root().join(SUBDIR);
    fs::create_dir_all(&note_dir).map_err(|e| format!("isg: {e}"))?;

    if args.list {
        return note::list_md_by_recency(&note_dir)
            .map(|v| v.join("\n"))
            .map_err(|e| format!("isg: {e}"));
    }

    let filename = match (args.cont, args.name) {
        // -c [INDEX]: continue note INDEX (or the latest) with the next letter.
        (Some(which), _) => {
            let names = note::list_md_by_recency(&note_dir).map_err(|e| format!("isg: {e}"))?;
            let base = match which {
                Some(n) => n,
                None => max_base(&names)
                    .ok_or_else(|| "isg: no notes to continue".to_string())?,
            };
            next_continuation(&names, base)
        }
        // NAME: open an existing note by basename.
        (None, Some(n)) => with_md(n.trim())?,
        // (default): next enumerated note.
        (None, None) => {
            let names = note::list_md_by_recency(&note_dir).map_err(|e| format!("isg: {e}"))?;
            format!("isg{}.md", next_index(&names))
        }
    };
    note::ensure_note(palace.root(), SUBDIR, &filename, TAG, None)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("isg: {e}"))
}

/// The next isg index: one past the highest existing `isg<N>.md`, or 0 if none.
fn next_index(names: &[String]) -> u64 {
    names
        .iter()
        .filter_map(|n| parse_index(n))
        .max()
        .map_or(0, |m| m + 1)
}

/// Parse the index out of an `isg<digits>.md` basename. `None` for anything
/// else (`isg.md`, `isgfoo.md`, continuations like `isg5a.md`, non-isg names).
fn parse_index(name: &str) -> Option<u64> {
    let digits = name.strip_suffix(".md")?.strip_prefix("isg")?;
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok()
}

/// The highest base index present (ignoring continuations), or `None` if empty.
fn max_base(names: &[String]) -> Option<u64> {
    names.iter().filter_map(|n| parse_index(n)).max()
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
    fn parse_index_cases() {
        assert_eq!(parse_index("isg0.md"), Some(0));
        assert_eq!(parse_index("isg52.md"), Some(52));
        assert_eq!(parse_index("isg.md"), None);
        assert_eq!(parse_index("isgfoo.md"), None);
        assert_eq!(parse_index("foo.md"), None);
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
    fn max_base_ignores_continuations() {
        let names = s(&["isg3.md", "isg7.md", "isg7a.md", "isg7b.md"]);
        assert_eq!(max_base(&names), Some(7));
    }
}
