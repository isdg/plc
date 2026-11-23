//! The date the stat commands take: `DD [MM [YY]]` as `plc daily` reads it,
//! a field left off being today's, so no date is today, and `-` for a whole
//! field, which widens what is shown: `- MM YY` is a month, `- - YY` a year.
//! A lone `+N` or `-N` is the day N days after or before today.
//!
//! `plc stat` also takes a range, `FROM .. TO` (see [`parse_span`]).

use chrono::{Datelike, Days, Months, NaiveDate};
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

/// What `plc stat` shows: what a date names, or the days of a range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Scope(Scope),
    /// Every day from the first to the last, both included.
    Days(NaiveDate, NaiveDate),
}

const WHOLE: &str = "-";
const TO: &str = "..";

/// Read `fields` against `today`.
pub fn parse(fields: &[String], today: NaiveDate) -> Result<Scope, String> {
    match fields {
        [f] if is_offset(f) => shift(today, offset(f)?).map(Scope::Day),
        _ if fields.iter().any(|f| is_offset(f)) => {
            Err("an offset stands alone: +N or -N".to_string())
        }
        _ => parse_date(fields, today),
    }
}

/// Read `fields` against `today`: a date, as [`parse`] reads it, or a
/// range `FROM .. TO`. An end is a date, standing for its first day on the
/// left and its last on the right, or an [`amount`] from today; a left end
/// left off is January 1 of the right end's year, a right one today.
pub fn parse_span(
    fields: &[String],
    today: NaiveDate,
) -> Result<Span, String> {
    let words = split_to(fields);
    let Some(at) = words.iter().position(|w| w == TO) else {
        return parse(fields, today).map(Span::Scope);
    };
    let (from, to) = (&words[..at], &words[at + 1..]);
    if to.iter().any(|w| w == TO) {
        return Err("a range has one ..: FROM .. TO".to_string());
    }
    let last = match to {
        [] => today,
        _ => end(to, today)?.1,
    };
    let first = match from {
        [] => NaiveDate::from_ymd_opt(last.year(), 1, 1).expect("January 1"),
        _ => end(from, today)?.0,
    };
    if first > last {
        return Err(format!("a range runs forward: {first} is after {last}"));
    }
    Ok(narrow(first, last))
}

/// `fields` with each `..` its own word, so `3m..` reads as `3m ..`.
fn split_to(fields: &[String]) -> Vec<String> {
    let mut words = Vec::new();
    for field in fields {
        let mut parts = field.trim().split(TO).peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                words.push(part.to_string());
            }
            if parts.peek().is_some() {
                words.push(TO.to_string());
            }
        }
    }
    words
}

/// The first and last day a range end names.
fn end(
    fields: &[String],
    today: NaiveDate,
) -> Result<(NaiveDate, NaiveDate), String> {
    match fields {
        [f] if is_amount(f) => amount(f, today).map(|day| (day, day)),
        _ => parse(fields, today).map(Scope::span),
    }
}

/// Whether `field` is written as an [`amount`]: an offset, or a count with
/// a unit.
fn is_amount(field: &str) -> bool {
    let field = field.trim();
    is_offset(field)
        || (field.starts_with(|c: char| c.is_ascii_digit())
            && field.ends_with(['d', 'w', 'm']))
}

/// The day an amount names: `+N` or `-N` days from today as for a date, or
/// a count of days, weeks or months, `30d`, `2w`, `3m`, back from today, or
/// ahead with a `+`. A month back from May 31 is April 30.
fn amount(s: &str, today: NaiveDate) -> Result<NaiveDate, String> {
    let s = s.trim();
    let bad = || format!("invalid amount: {s} (+N, -N, or N with d, w or m)");
    let (count, unit) = match s.strip_suffix(['d', 'w', 'm']) {
        Some(count) => (count, &s[count.len()..]),
        None => (s, ""),
    };
    let signed = if count.starts_with(['+', '-']) {
        count.to_string()
    } else if unit.is_empty() {
        return Err(bad());
    } else {
        format!("-{count}")
    };
    let n = offset(&signed).map_err(|_| bad())?;
    let far = || format!("amount out of range: {s}");
    match unit {
        "w" => shift(today, n.checked_mul(7).ok_or_else(far)?),
        "m" => {
            let months = u32::try_from(n.unsigned_abs()).map_err(|_| far())?;
            let months = Months::new(months);
            let to = if n < 0 {
                today.checked_sub_months(months)
            } else {
                today.checked_add_months(months)
            };
            to.ok_or_else(far)
        }
        _ => shift(today, n),
    }
}

/// The range as the scope it is, when it covers exactly one.
fn narrow(first: NaiveDate, last: NaiveDate) -> Span {
    let month = Scope::Month(first.year(), first.month());
    let year = Scope::Year(first.year());
    if first == last {
        Span::Scope(Scope::Day(first))
    } else if month.span() == (first, last) {
        Span::Scope(month)
    } else if year.span() == (first, last) {
        Span::Scope(year)
    } else {
        Span::Days(first, last)
    }
}

/// Whether `field` is written as an offset rather than a date field.
fn is_offset(field: &str) -> bool {
    let field = field.trim();
    field.starts_with('+') || (field.starts_with('-') && field != WHOLE)
}

/// A signed count, written `+N` or `-N`.
pub fn offset(s: &str) -> Result<i64, String> {
    let s = s.trim();
    let digits = s.strip_prefix(['+', '-']).unwrap_or_default();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("invalid offset: {s} (+N or -N)"));
    }
    s.parse().map_err(|_| format!("offset out of range: {s}"))
}

/// The day `days` after `day`, or before it when negative.
pub fn shift(day: NaiveDate, days: i64) -> Result<NaiveDate, String> {
    let n = Days::new(days.unsigned_abs());
    let to = if days < 0 {
        day.checked_sub_days(n)
    } else {
        day.checked_add_days(n)
    };
    to.ok_or_else(|| format!("offset out of range: {days:+}"))
}

fn parse_date(fields: &[String], today: NaiveDate) -> Result<Scope, String> {
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

    #[test]
    fn an_offset_counts_days_from_today() {
        assert_eq!(scope(&["+1"]), Ok(day(2026, 11, 15)));
        assert_eq!(scope(&["-1"]), Ok(day(2026, 11, 13)));
        assert_eq!(scope(&["+0"]), Ok(day(2026, 11, 14)));
        assert_eq!(scope(&["-0"]), Ok(day(2026, 11, 14)));
        assert_eq!(scope(&[" +17 "]), Ok(day(2026, 12, 1)));
        assert_eq!(scope(&["-366"]), Ok(day(2025, 11, 13)));
    }

    fn span(fields: &[&str]) -> Result<Span, String> {
        let fields: Vec<String> =
            fields.iter().map(|f| f.to_string()).collect();
        parse_span(&fields, today())
    }

    fn at(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn a_range_runs_from_its_left_ends_first_day_to_its_rights_last() {
        let days = |a, b| Ok(Span::Days(a, b));
        let sep_nov = days(at(2026, 9, 1), at(2026, 11, 30));
        assert_eq!(span(&["-", "09", "..", "-", "11"]), sep_nov);
        let aug_nov = days(at(2026, 8, 15), at(2026, 11, 10));
        assert_eq!(span(&["15", "08", "..", "10", "11"]), aug_nov);
        let across = days(at(2025, 11, 1), at(2026, 2, 28));
        assert_eq!(span(&["-", "11", "25", "..", "-", "02"]), across);
        assert_eq!(span(&["-", "09", ".."]), days(at(2026, 9, 1), today()));
        let to_march = days(at(2026, 1, 1), at(2026, 3, 31));
        assert_eq!(span(&["..", "-", "03"]), to_march);
        let in_2025 = days(at(2025, 1, 1), at(2025, 3, 31));
        assert_eq!(span(&["..", "-", "03", "25"]), in_2025);
    }

    #[test]
    fn a_range_ends_at_amounts_from_today() {
        let back = |a| Ok(Span::Days(a, today()));
        assert_eq!(span(&["3m", ".."]), back(at(2026, 8, 14)));
        assert_eq!(span(&["2w", ".."]), back(at(2026, 10, 31)));
        assert_eq!(span(&["30d", ".."]), back(at(2026, 10, 15)));
        assert_eq!(span(&["-30", ".."]), back(at(2026, 10, 15)));
        assert_eq!(span(&["-3m", ".."]), back(at(2026, 8, 14)));
        let ahead = Ok(Span::Days(today(), at(2027, 1, 14)));
        assert_eq!(span(&["-0", "..", "+2m"]), ahead);
        let into_2027 = Ok(Span::Days(at(2027, 1, 1), at(2027, 1, 14)));
        assert_eq!(span(&["..", "+2m"]), into_2027);
        assert_eq!(span(&["3m.."]), back(at(2026, 8, 14)));
        let end_of_month = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        assert_eq!(amount("1m", end_of_month), Ok(at(2026, 4, 30)));
    }

    #[test]
    fn a_range_that_is_one_scope_is_drawn_as_it() {
        assert_eq!(span(&["5", "..", "5"]), Ok(Span::Scope(day(2026, 11, 5))));
        let month = Ok(Span::Scope(Scope::Month(2026, 9)));
        assert_eq!(span(&["-", "09", "..", "-", "09"]), month);
        let year = Ok(Span::Scope(Scope::Year(2025)));
        assert_eq!(span(&["-", "01", "25", "..", "-", "12", "25"]), year);
        assert_eq!(span(&["-", "-"]), Ok(Span::Scope(Scope::Year(2026))));
    }

    #[test]
    fn a_bad_range_says_why() {
        let err = |f: &[&str]| span(f).unwrap_err();
        let back = "a range runs forward: 2026-11-01 is after 2026-09-30";
        assert_eq!(err(&["-", "11", "..", "-", "09"]), back);
        let one = "a range has one ..: FROM .. TO";
        assert_eq!(err(&["1", "..", "2", "..", "3"]), one);
        assert_eq!(err(&["3x", ".."]), "invalid day: 3x");
        let bad = "invalid amount: 3x (+N, -N, or N with d, w or m)";
        assert_eq!(amount("3x", today()), Err(bad.to_string()));
        let far = "amount out of range: 99999999999m";
        assert_eq!(err(&["99999999999m", ".."]), far);
    }

    #[test]
    fn an_offset_stands_alone_and_says_why_not() {
        let err = |f: &[&str]| scope(f).unwrap_err();
        let alone = "an offset stands alone: +N or -N";
        assert_eq!(err(&["+1", "12"]), alone);
        assert_eq!(err(&["05", "-1"]), alone);
        assert_eq!(err(&["-", "+1"]), alone);
        assert_eq!(err(&["+"]), "invalid offset: + (+N or -N)");
        assert_eq!(err(&["-x"]), "invalid offset: -x (+N or -N)");
        assert_eq!(err(&["+-1"]), "invalid offset: +-1 (+N or -N)");
        let big = "+99999999999999999999";
        assert_eq!(err(&[big]), format!("offset out of range: {big}"));
        let far = "offset out of range: +9223372036854775807";
        assert_eq!(err(&["+9223372036854775807"]), far);
        let far = "offset out of range: -9223372036854775808";
        assert_eq!(err(&["-9223372036854775808"]), far);
    }
}
