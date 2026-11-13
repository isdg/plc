//! The bytes written each day across a vault, as `plc stat` scores them.
//! Text belongs to the stamp above it, the note's header or one written
//! inline, so a note kept for weeks counts on each day it grew.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, FixedOffset, Local, NaiveDate};

use crate::ledger::TIMESTAMP_FMT;
use crate::notes;

/// Bytes written per day in the notes of the vault at `root`, leaving out
/// `+ledger` notes: their lines are transactions, which `plc ledger stat`
/// counts. Text above a note's first stamp is dated by its file name, else
/// its mtime.
pub fn by_day(root: &Path) -> HashMap<NaiveDate, u64> {
    let mut days = HashMap::new();
    let ledger =
        |p: &Path| notes::stem(p).is_some_and(|s| s.ends_with("+ledger"));
    for path in notes::walk(root).filter(|p| !ledger(p)) {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let undated = notes::stem(&path)
            .and_then(notes::name_date)
            .or_else(|| modified(&path));
        for (day, bytes) in note_bytes(&text, undated) {
            *days.entry(day).or_default() += bytes;
        }
    }
    days
}

fn modified(path: &Path) -> Option<NaiveDate> {
    let at: DateTime<Local> = fs::metadata(path).ok()?.modified().ok()?.into();
    Some(at.date_naive())
}

/// A note's bytes by the day of the stamp above them, in note order. The
/// stamps and the header plc seeds (a stamp line, then blank lines and lone
/// `[[tag]]` lines) count for nothing; text above every stamp is `undated`.
pub fn note_bytes(
    text: &str,
    undated: Option<NaiveDate>,
) -> Vec<(NaiveDate, u64)> {
    let mut lines = text.lines().peekable();
    let mut day = undated;
    if let Some(at) = lines.peek().and_then(|l| notes::stamp(l)) {
        day = Some(at.date_naive());
        lines.next();
        while lines.next_if(|l| seeded(l)).is_some() {}
    }
    let mut out: Vec<(NaiveDate, u64)> = Vec::new();
    let mut add = |day: Option<NaiveDate>, text: &str| {
        let (Some(day), bytes @ 1..) = (day, text.len() as u64) else {
            return;
        };
        match out.last_mut() {
            Some((last, total)) if *last == day => *total += bytes,
            _ => out.push((day, bytes)),
        }
    };
    for line in lines {
        let mut from = 0;
        for (start, end, at) in inline_stamps(line) {
            add(day, &line[from..start]);
            day = Some(at.date_naive());
            from = end;
        }
        add(day, &line[from..]);
    }
    out
}

fn seeded(line: &str) -> bool {
    let line = line.trim();
    let link = line.starts_with("[[") && line.ends_with("]]");
    line.is_empty() || (link && line.matches("[[").count() == 1)
}

/// The `<date> <time> <zone>` stamps in `line`, as byte spans and times.
/// One second on its line spans the signature before it, as a stamp line's
/// does.
fn inline_stamps(line: &str) -> Vec<(usize, usize, DateTime<FixedOffset>)> {
    let tokens = tokens(line);
    let mut found = Vec::new();
    let mut i = 0;
    while let [(start, date), (_, time), (zone_at, zone), ..] = tokens[i..] {
        let at = (date.len() == 10 && date.as_bytes()[4] == b'-')
            .then(|| format!("{date} {time} {zone}"))
            .and_then(|s| DateTime::parse_from_str(&s, TIMESTAMP_FMT).ok());
        let Some(at) = at else {
            i += 1;
            continue;
        };
        let start = if i == 1 { tokens[0].0 } else { start };
        found.push((start, zone_at + zone.len(), at));
        i += 3;
    }
    found
}

/// The whitespace-separated tokens of `line` with their byte offsets.
fn tokens(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, ch) in line.char_indices() {
        match (ch.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                out.push((s, &line[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, &line[s..]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    const HEAD: &str = "isg 2026-11-10 09:00:00 +0100\n\n[[daily]]\n";

    #[test]
    fn a_seeded_note_is_empty() {
        assert!(note_bytes(HEAD, Some(day(1))).is_empty());
        let shot = "isg 2026-11-10 09:00:00 +0100\n";
        assert!(note_bytes(shot, None).is_empty());
        let tagged = format!("{HEAD}[[review]]\n\n");
        assert!(note_bytes(&tagged, None).is_empty());
    }

    #[test]
    fn text_belongs_to_the_stamp_above_it() {
        let text = format!(
            "{HEAD}abc\n\nisg 2026-11-12 20:00:00 +0100\nдар\n\
             x y 2026-11-13 08:00:00 +0100 yz"
        );
        let got = note_bytes(&text, Some(day(1)));
        // "дар" is 6 bytes; "x y " stays with the day above it.
        assert_eq!(got, [(day(10), 3), (day(12), 6 + 4), (day(13), 3)]);
    }

    #[test]
    fn unstamped_text_is_undated() {
        assert_eq!(note_bytes("plain\n", Some(day(5))), [(day(5), 5)]);
        assert!(note_bytes("plain\n", None).is_empty());
        let late = "a\n2026-11-07 10:00:00 +0100 b";
        assert_eq!(note_bytes(late, Some(day(5))), [(day(5), 1), (day(7), 2)]);
    }

    #[test]
    fn only_a_full_stamp_dates() {
        let text = "2026-11-07 10:00:00 Z\n2026-11-07 x +0100\n";
        assert_eq!(note_bytes(text, None), []);
        assert_eq!(note_bytes(text, Some(day(1))), [(day(1), 21 + 18)]);
    }

    #[test]
    fn a_vault_adds_its_notes_by_day() {
        let root = std::env::temp_dir()
            .join(format!("plc-activity-{}", std::process::id()));
        let daily = root.join("notes/management/daily/2026/11");
        fs::create_dir_all(&daily).unwrap();
        fs::create_dir_all(root.join(".plc")).unwrap();
        fs::write(daily.join("2026-11-10.md"), format!("{HEAD}abc\n")).unwrap();
        fs::write(daily.join("2026-11-10T21.00.md"), "de").unwrap();
        let weekly = "isg 2026-11-09 08:00:00 +0100\n\n[[weekly]]\nx\n\
                      isg 2026-11-10 08:00:00 +0100\nyy\n";
        fs::write(root.join("w.md"), weekly).unwrap();
        fs::write(root.join(".plc/hidden.md"), "zzzz").unwrap();
        let spend = "$ 2026-11-11 12:00:00 +0100 * 4.50 coffee -a cash\n";
        fs::write(daily.join("2026-11+ledger.md"), spend).unwrap();

        let days = by_day(&root);
        assert_eq!(days.get(&day(9)), Some(&1));
        assert_eq!(days.get(&day(10)), Some(&(3 + 2 + 2)));
        assert_eq!(days.len(), 2);
        fs::remove_dir_all(&root).ok();
    }
}
