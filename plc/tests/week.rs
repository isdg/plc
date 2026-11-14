//! End to end: `week_start` in `.plcrc` names weekly and do-notes, and a
//! bad one fails them and shows in `plc doctor`.

mod common;

use std::path::PathBuf;

use chrono::{Local, Weekday};
use plc_core::week;

use common::{fail, ok, Home};

fn with_rc(tag: &str, rc: &str) -> Home {
    let home = Home::new(tag);
    home.vault("palace");
    home.write(".plc/.plcrc", &format!("vault = ~/palace\n{rc}"));
    home
}

/// The Saturday-start note `args` print, checked against this week's name
/// on both sides of the run, so a run across midnight still passes.
fn assert_week(home: &Home, args: &[&str], dir: &str, prefix: &str) {
    let path = || {
        let today = Local::now().date_naive();
        let name = week::label(today, Weekday::Sat);
        home.path(&format!("palace/{dir}/{prefix}{name}.md"))
    };
    let before = path();
    let got = PathBuf::from(ok(&mut home.plc(args)));
    assert!(got == before || got == path(), "{got:?}");
    assert!(got.is_file());
}

#[test]
fn weekly_and_do_notes_follow_week_start() {
    let home = with_rc("week-sat", "week_start = Saturday\n");
    let weekly = "notes/management/weekly";
    assert_week(&home, &["weekly"], weekly, "");
    assert_week(&home, &["todo", "-n"], "notes/management/do", "do-");
}

#[test]
fn a_bad_week_start_fails_and_is_reported() {
    let home = with_rc("week-bad", "week_start = sa\n");
    let want = "week_start: `sa` is not a weekday (monday … sunday)";
    assert!(fail(&mut home.plc(&["weekly"])).contains(want));
    assert!(fail(&mut home.plc(&["todo", "-n"])).contains(want));
    let out = ok(&mut home.plc(&["doctor"]));
    assert!(out.contains(&format!("  ! ~/.plc/.plcrc: {want}")), "{out}");
}
