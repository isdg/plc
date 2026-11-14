//! `plc daily` — create/resolve a daily note and print its path.
//!
//! Ports `daily()` (palace.zsh): takes a date as `DD [MM [YY]]`, read as the
//! stat commands read it (see [`when`]), defaults each missing field to
//! today, and marks any explicitly dated note with a `*` in its header.

use chrono::{Datelike, Local, NaiveDate};
use clap::Args;

use crate::cmd::when::{self, Scope};
use crate::config::Palace;
use crate::note;

#[derive(Args)]
pub struct DailyArgs {
    /// Date as `DD [MM [YY|YYYY]]`, today's for any field left off.
    #[arg(value_name = "DD MM YY")]
    positional: Vec<String>,
    /// Resolve the note and report `new|old<TAB><path>` without creating it.
    /// Lets the shell prompt before a new note is actually written.
    #[arg(long = "check")]
    check: bool,
    #[command(flatten)]
    pub note: note::NoteArgs,
}

pub fn run(palace: &Palace, args: DailyArgs) -> Result<String, String> {
    let date = resolve(&args.positional, Local::now().date_naive())?;
    let explicit = !args.positional.is_empty();

    let subdir = format!(
        "notes/management/daily/{:04}/{:02}",
        date.year(),
        date.month()
    );
    let filename = format!(
        "{:04}-{:02}-{:02}.md",
        date.year(),
        date.month(),
        date.day()
    );
    let marker = if explicit { Some("*") } else { None };

    if args.check {
        let (new, path) = note::would_create(palace.root(), &subdir, &filename);
        let status = if new { "new" } else { "old" };
        return Ok(format!("{status}\t{}", path.display()));
    }

    note::ensure_note(palace.root(), &subdir, &filename, "daily", marker, note::SIGNATURE)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("daily: {e}"))
}

/// The day `positional` names against `today`: one day, never a whole
/// month or year.
fn resolve(
    positional: &[String],
    today: NaiveDate,
) -> Result<NaiveDate, String> {
    match when::parse(positional, today) {
        Ok(Scope::Day(day)) => Ok(day),
        Ok(_) => Err("daily: a daily note is one day: DD MM YY".to_string()),
        Err(e) => Err(format!("daily: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 20).unwrap()
    }

    fn day(fields: &[&str]) -> Result<NaiveDate, String> {
        let fields: Vec<String> =
            fields.iter().map(|f| f.to_string()).collect();
        resolve(&fields, today())
    }

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn defaults_to_today() {
        assert_eq!(day(&[]), Ok(today()));
    }

    #[test]
    fn fields_left_off_are_today() {
        assert_eq!(day(&["05"]), Ok(ymd(2026, 6, 5)));
        assert_eq!(day(&["05", "05"]), Ok(ymd(2026, 5, 5)));
        assert_eq!(day(&["05", "05", "25"]), Ok(ymd(2025, 5, 5)));
        assert_eq!(day(&["12", "3", "2019"]), Ok(ymd(2019, 3, 12)));
    }

    #[test]
    fn bad_dates_are_refused() {
        let err = day(&["31", "2"]).unwrap_err();
        assert_eq!(err, "daily: no such day: 31 02 2026");
        let err = day(&["1", "2", "3", "4"]).unwrap_err();
        assert_eq!(err, "daily: a date is at most DD MM YY");
        let err = day(&["-", "05"]).unwrap_err();
        assert_eq!(err, "daily: a daily note is one day: DD MM YY");
    }
}
