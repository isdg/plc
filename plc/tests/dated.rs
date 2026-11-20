//! End to end: a daily or weekly note for any day or week but today's is
//! marked with `*`, and `±N` names one N days or weeks off.

mod common;

use std::fs;
use std::path::PathBuf;

use chrono::{Days, Local, NaiveDate, Weekday};
use plc_core::week;

use common::{fail, ok, Home};

fn with_rc(tag: &str) -> Home {
    let home = Home::new(tag);
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    home
}

/// Run `args`, check the note is the one `name` gives for today on either
/// side of the run (so a run across midnight passes), and return its stamp.
fn stamp(
    home: &Home,
    args: &[&str],
    name: impl Fn(NaiveDate) -> String,
) -> String {
    let path = || home.path(&name(Local::now().date_naive()));
    let before = path();
    let got = PathBuf::from(ok(&mut home.plc(args)));
    assert!(got == before || got == path(), "{got:?}");
    let body = fs::read_to_string(&got).unwrap();
    body.lines().next().unwrap().to_string()
}

fn daily(days: i64) -> impl Fn(NaiveDate) -> String {
    move |today| {
        let n = Days::new(days.unsigned_abs());
        let day = if days < 0 { today - n } else { today + n };
        let ym = day.format("%Y/%m");
        format!("palace/notes/management/daily/{ym}/{day}.md")
    }
}

fn weekly(weeks: i64) -> impl Fn(NaiveDate) -> String {
    move |today| {
        let n = Days::new(7 * weeks.unsigned_abs());
        let day = if weeks < 0 { today - n } else { today + n };
        let label = week::label(day, Weekday::Mon);
        format!("palace/notes/management/weekly/{label}.md")
    }
}

#[test]
fn daily_marks_by_day_not_by_arguments() {
    let home = with_rc("dated-daily");
    let run = |date: &str| {
        let mut args = vec!["daily"];
        args.extend(date.split(' '));
        let path = PathBuf::from(ok(&mut home.plc(&args)));
        let body = fs::read_to_string(path).unwrap();
        body.lines().next().unwrap().to_string()
    };
    let today = Local::now().date_naive();
    let line = run(&today.format("%d %m %Y").to_string());
    if Local::now().date_naive() == today {
        assert!(!line.ends_with('*'), "{line}");
    }
    assert!(run("29 02 2020").ends_with(" *"));
}

#[test]
fn daily_offsets_mark_any_other_day() {
    let home = with_rc("offset-daily");
    assert!(!stamp(&home, &["daily"], daily(0)).ends_with('*'));
    assert!(stamp(&home, &["daily", "+1"], daily(1)).ends_with(" *"));
    assert!(stamp(&home, &["daily", "-1"], daily(-1)).ends_with(" *"));
    let want = "daily: an offset stands alone: +N or -N";
    assert_eq!(fail(&mut home.plc(&["daily", "+1", "12"])), want);
}

#[test]
fn weekly_offsets_mark_any_other_week() {
    let home = with_rc("offset-weekly");
    assert!(!stamp(&home, &["weekly"], weekly(0)).ends_with('*'));
    assert!(stamp(&home, &["weekly", "+1"], weekly(1)).ends_with(" *"));
    assert!(stamp(&home, &["weekly", "-2"], weekly(-2)).ends_with(" *"));
    let err = fail(&mut home.plc(&["weekly", "1"]));
    assert!(err.contains("invalid offset: 1 (+N or -N)"), "{err}");
}

#[test]
fn stat_takes_a_day_offset() {
    let home = with_rc("offset-stat");
    ok(&mut home.plc(&["stat", "-1"]));
    ok(&mut home.plc(&["stat", "+1"]));
}
