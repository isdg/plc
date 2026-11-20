//! End to end: a daily note for any day but today's is marked with `*`.

mod common;

use std::fs;
use std::path::PathBuf;

use chrono::Local;

use common::{ok, Home};

fn with_rc(tag: &str) -> Home {
    let home = Home::new(tag);
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    home
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
