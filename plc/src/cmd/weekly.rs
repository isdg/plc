//! `plc weekly` — create/resolve a week's note and print its path.
//!
//! Ports `weekly()` (palace.zsh): target
//! `notes/management/weekly/<YYYY-Www>.md`, tag `weekly`, the week starting
//! on `week_start` (see [`week::label`]), this week or `+N`/`-N` weeks off,
//! marking a note for any other week with a `*`.

use chrono::{Local, NaiveDate};
use clap::Args;
use plc_core::{rc, week};

use crate::cmd::when;
use crate::config::Palace;
use crate::note;

pub(crate) const SUBDIR: &str = "notes/management/weekly";

#[derive(Args)]
pub struct WeeklyArgs {
    /// `+N`/`-N` weeks from this week.
    #[arg(
        value_name = "+N",
        allow_negative_numbers = true,
        value_parser = when::offset
    )]
    offset: Option<i64>,
    #[command(flatten)]
    pub note: note::NoteArgs,
}

pub fn run(palace: &Palace, args: WeeklyArgs) -> Result<String, String> {
    let start = rc::week_start().map_err(|e| format!("weekly: {e}"))?;
    let today = Local::now().date_naive();
    let day = resolve(args.offset.unwrap_or(0), today)
        .map_err(|e| format!("weekly: {e}"))?;
    let label = week::label(day, start);
    let marker = (label != week::label(today, start)).then_some("*");
    note::ensure_note(
        palace.root(),
        SUBDIR,
        &format!("{label}.md"),
        "weekly",
        marker,
        note::SIGNATURE,
    )
    .map(|p| p.display().to_string())
    .map_err(|e| format!("weekly: {e}"))
}

/// A day of the week `weeks` after the one holding `today`.
fn resolve(weeks: i64, today: NaiveDate) -> Result<NaiveDate, String> {
    let days = weeks
        .checked_mul(7)
        .ok_or_else(|| format!("offset out of range: {weeks:+}"))?;
    when::shift(today, days)
}

#[cfg(test)]
mod tests {
    use chrono::Weekday;

    use super::*;

    fn label(weeks: i64, today: NaiveDate, start: Weekday) -> String {
        week::label(resolve(weeks, today).unwrap(), start)
    }

    #[test]
    fn an_offset_counts_weeks_from_this_one() {
        let fri = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        assert_eq!(label(0, fri, Weekday::Mon), "2026-W47");
        assert_eq!(label(1, fri, Weekday::Mon), "2026-W48");
        assert_eq!(label(-1, fri, Weekday::Mon), "2026-W46");
        assert_eq!(label(6, fri, Weekday::Mon), "2026-W53");
        assert_eq!(label(7, fri, Weekday::Mon), "2027-W01");
        let sat = NaiveDate::from_ymd_opt(2026, 12, 26).unwrap();
        assert_eq!(label(0, sat, Weekday::Sat), "2026-W53");
        assert_eq!(label(1, sat, Weekday::Sat), "2027-W01");
        assert_eq!(label(-1, sat, Weekday::Sat), "2026-W52");
    }

    #[test]
    fn a_far_offset_is_refused() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let err = resolve(i64::MAX, today).unwrap_err();
        assert_eq!(err, format!("offset out of range: +{}", i64::MAX));
        let err = resolve(1 << 50, today).unwrap_err();
        assert_eq!(err, format!("offset out of range: +{}", 7_i64 << 50));
    }
}
