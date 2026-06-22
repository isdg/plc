//! `plc daily` — create/resolve a daily note and print its path.
//!
//! Ports `daily()` (palace.zsh): accepts a date as positional `DD [MM [YY]]`
//! or via `-d/-m/-y` flags, defaults each missing field to today, expands a
//! 2-digit year to `20YY`, rejects impossible dates, and marks any explicitly
//! dated note with a `*` in its header.

use chrono::{Datelike, Local, NaiveDate};
use clap::Args;

use crate::config::Palace;
use crate::note;

#[derive(Args)]
pub struct DailyArgs {
    /// Date as DD [MM [YY|YYYY]]; fills any fields not given via flags.
    #[arg(value_name = "DD MM YY")]
    positional: Vec<String>,
    /// Day of month (1-31).
    #[arg(short = 'd', long = "day")]
    day: Option<String>,
    /// Month (1-12).
    #[arg(short = 'm', long = "month")]
    month: Option<String>,
    /// Year; 2-digit (25 → 2025) or 4-digit.
    #[arg(short = 'y', long = "year")]
    year: Option<String>,
}

pub fn run(palace: &Palace, args: DailyArgs) -> Result<String, String> {
    let (date, explicit) =
        resolve(args.day, args.month, args.year, &args.positional, Local::now().date_naive())?;

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

    note::ensure_note(palace.root(), &subdir, &filename, "daily", marker, note::SIGNATURE)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("daily: {e}"))
}

/// Resolve the target date from flag + positional inputs against `today`.
///
/// Positional tokens fill the first still-unset field in day → month → year
/// order (matching the zsh while-loop). Returns the date plus whether any
/// field was given explicitly (→ back-date `*` marker).
fn resolve(
    day: Option<String>,
    month: Option<String>,
    year: Option<String>,
    positional: &[String],
    today: NaiveDate,
) -> Result<(NaiveDate, bool), String> {
    let mut day = day;
    let mut month = month;
    let mut year = year;
    let explicit =
        day.is_some() || month.is_some() || year.is_some() || !positional.is_empty();

    for tok in positional {
        if day.is_none() {
            day = Some(tok.clone());
        } else if month.is_none() {
            month = Some(tok.clone());
        } else if year.is_none() {
            year = Some(tok.clone());
        } else {
            return Err(format!("daily: extra arg: {tok}"));
        }
    }

    let dd = match day {
        Some(s) => parse_field(&s, "day")?,
        None => today.day(),
    };
    let mm = match month {
        Some(s) => parse_field(&s, "month")?,
        None => today.month(),
    };
    let yy = match year {
        Some(s) => parse_year(&s)?,
        None => today.year(),
    };

    let date = NaiveDate::from_ymd_opt(yy, mm, dd)
        .ok_or_else(|| format!("daily: invalid date {yy:04}-{mm:02}-{dd:02}"))?;
    Ok((date, explicit))
}

/// Parse a base-10 day/month field (tolerates leading zeros, e.g. "05").
fn parse_field(s: &str, what: &str) -> Result<u32, String> {
    s.trim()
        .parse::<u32>()
        .map_err(|_| format!("daily: invalid {what}: {s}"))
}

/// Parse a year, expanding an exactly-2-digit value to `20YY`.
fn parse_year(s: &str) -> Result<i32, String> {
    let t = s.trim();
    let n: i32 = t.parse().map_err(|_| format!("daily: invalid year: {s}"))?;
    let is_two_digit = t.len() == 2 && t.bytes().all(|b| b.is_ascii_digit());
    Ok(if is_two_digit { 2000 + n } else { n })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 20).unwrap()
    }

    fn none() -> Option<String> {
        None
    }

    #[test]
    fn defaults_to_today() {
        let (d, explicit) = resolve(none(), none(), none(), &[], today()).unwrap();
        assert_eq!(d, today());
        assert!(!explicit);
    }

    #[test]
    fn positional_day_month_year() {
        let pos = vec!["05".into(), "05".into(), "25".into()];
        let (d, explicit) = resolve(none(), none(), none(), &pos, today()).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2025, 5, 5).unwrap());
        assert!(explicit);
    }

    #[test]
    fn two_digit_year_expands() {
        let (d, _) = resolve(none(), none(), Some("24".into()), &[], today()).unwrap();
        assert_eq!(d.year(), 2024);
    }

    #[test]
    fn four_digit_year_kept() {
        let (d, _) = resolve(none(), none(), Some("2019".into()), &[], today()).unwrap();
        assert_eq!(d.year(), 2019);
    }

    #[test]
    fn flags_then_positional_fill_remaining() {
        // -d 12 sets day; positionals fill month then year.
        let pos = vec!["3".into(), "2024".into()];
        let (d, _) =
            resolve(Some("12".into()), none(), none(), &pos, today()).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2024, 3, 12).unwrap());
    }

    #[test]
    fn missing_field_uses_today() {
        // Only year given → today's day + month.
        let (d, explicit) =
            resolve(none(), none(), Some("2025".into()), &[], today()).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2025, 6, 20).unwrap());
        assert!(explicit);
    }

    #[test]
    fn invalid_date_rejected() {
        let err = resolve(Some("31".into()), Some("2".into()), none(), &[], today())
            .unwrap_err();
        assert!(err.contains("invalid date"), "{err}");
    }

    #[test]
    fn extra_positional_rejected() {
        let pos = vec!["1".into(), "2".into(), "3".into(), "4".into()];
        let err = resolve(none(), none(), none(), &pos, today()).unwrap_err();
        assert!(err.contains("extra arg"), "{err}");
    }
}
