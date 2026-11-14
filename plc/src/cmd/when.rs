//! The date the stat commands take: `DD [MM [YY]]` as `plc daily` reads it,
//! a field left off being today's, so no date is today, and `-` for a whole
//! field, which widens what is shown: `- MM YY` is a month, `- - YY` a year.

use chrono::{Datelike, NaiveDate};
use plc_core::calendar;

/// What a date names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Day(NaiveDate),
    Month(i32, u32),
    Year(i32),
}

impl Scope {
    /// The first and last day it covers.
    pub fn span(self) -> (NaiveDate, NaiveDate) {
        let ymd = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).expect("a day");
        match self {
            Scope::Day(day) => (day, day),
            Scope::Month(y, m) => {
                (ymd(y, m, 1), ymd(y, m, calendar::last_day_of_month(y, m)))
            }
            Scope::Year(y) => (ymd(y, 1, 1), ymd(y, 12, 31)),
        }
    }
}

const WHOLE: &str = "-";

/// Read `fields` against `today`.
pub fn parse(fields: &[String], today: NaiveDate) -> Result<Scope, String> {
    let field = |i: usize| fields.get(i).map(|f| f.trim());
    let year = match field(2) {
        Some(WHOLE) => return Err("a year cannot be whole".to_string()),
        Some(y) => parse_year(y)?,
        None => today.year(),
    };
    let month = match field(1) {
        Some(WHOLE) => None,
        Some(m) => Some(parse_month(m)?),
        None => Some(today.month()),
    };
    match (fields.len(), field(0), month) {
        (0, _, _) => Ok(Scope::Day(today)),
        (4.., _, _) => Err("a date is at most DD MM YY".to_string()),
        (_, Some(WHOLE), None) => Ok(Scope::Year(year)),
        (_, Some(WHOLE), Some(m)) => Ok(Scope::Month(year, m)),
        (_, Some(_), None) => {
            Err("a whole month needs a whole day: - - YY".to_string())
        }
        (_, Some(d), Some(m)) => {
            let d: u32 = d.parse().map_err(|_| format!("invalid day: {d}"))?;
            NaiveDate::from_ymd_opt(year, m, d)
                .map(Scope::Day)
                .ok_or_else(|| format!("no such day: {d:02} {m:02} {year}"))
        }
        (_, None, _) => unreachable!("fields is not empty"),
    }
}

fn parse_month(s: &str) -> Result<u32, String> {
    match s.parse() {
        Ok(m @ 1..=12) => Ok(m),
        _ => Err(format!("invalid month: {s}")),
    }
}

/// A year as written, or `20YY` for two digits.
fn parse_year(s: &str) -> Result<i32, String> {
    let n: i32 = s.parse().map_err(|_| format!("invalid year: {s}"))?;
    let two_digit = s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());
    Ok(if two_digit { 2000 + n } else { n })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 14).unwrap()
    }

    fn scope(fields: &[&str]) -> Result<Scope, String> {
        let fields: Vec<String> =
            fields.iter().map(|f| f.to_string()).collect();
        parse(&fields, today())
    }

    fn day(y: i32, m: u32, d: u32) -> Scope {
        Scope::Day(NaiveDate::from_ymd_opt(y, m, d).unwrap())
    }

    #[test]
    fn no_date_is_today() {
        assert_eq!(scope(&[]), Ok(day(2026, 11, 14)));
    }

    #[test]
    fn a_day_fills_what_is_left_off_from_today() {
        assert_eq!(scope(&["27"]), Ok(day(2026, 11, 27)));
        assert_eq!(scope(&["27", "09"]), Ok(day(2026, 9, 27)));
        assert_eq!(scope(&["27", "09", "25"]), Ok(day(2025, 9, 27)));
        assert_eq!(scope(&["7", "9", "2025"]), Ok(day(2025, 9, 7)));
    }

    #[test]
    fn whole_fields_widen_to_a_month_or_a_year() {
        assert_eq!(scope(&["-"]), Ok(Scope::Month(2026, 11)));
        assert_eq!(scope(&["-", "09"]), Ok(Scope::Month(2026, 9)));
        assert_eq!(scope(&["-", "09", "25"]), Ok(Scope::Month(2025, 9)));
        assert_eq!(scope(&["-", "-"]), Ok(Scope::Year(2026)));
        assert_eq!(scope(&["-", "-", "25"]), Ok(Scope::Year(2025)));
    }

    #[test]
    fn a_scope_spans_its_days() {
        let at = |m, d| NaiveDate::from_ymd_opt(2024, m, d).unwrap();
        assert_eq!(Scope::Day(at(2, 29)).span(), (at(2, 29), at(2, 29)));
        assert_eq!(Scope::Month(2024, 2).span(), (at(2, 1), at(2, 29)));
        assert_eq!(Scope::Year(2024).span(), (at(1, 1), at(12, 31)));
    }

    #[test]
    fn bad_dates_say_why() {
        let err = |f: &[&str]| scope(f).unwrap_err();
        assert_eq!(err(&["30", "02", "26"]), "no such day: 30 02 2026");
        let whole = "a whole month needs a whole day: - - YY";
        assert_eq!(err(&["27", "-", "25"]), whole);
        assert_eq!(err(&["-", "-", "-"]), "a year cannot be whole");
        assert_eq!(err(&["1", "2", "3", "4"]), "a date is at most DD MM YY");
        assert_eq!(err(&["-", "13"]), "invalid month: 13");
        assert_eq!(err(&["x"]), "invalid day: x");
    }
}
