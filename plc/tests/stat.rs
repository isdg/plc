//! End to end: `--color` picks between plain and coloured shade glyphs in
//! the `plc stat` and `plc ledger stat` heatmaps.

mod common;

use std::process::Command;

use common::{fail, ok, Home};

const ESC: char = '\x1b';

fn with_vault(tag: &str) -> Home {
    let home = Home::new(tag);
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    home
}

/// `plc args…` with no colour preference inherited from the test's env.
fn plc(home: &Home, args: &[&str]) -> Command {
    let mut cmd = home.plc(args);
    for var in ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "CI", "TERM"] {
        cmd.env_remove(var);
    }
    cmd
}

#[test]
fn always_colours_every_heatmap_from_the_ansi_ramp() {
    let home = with_vault("stat-always");
    let mut runs = Vec::new();
    for cmd in [&["stat"][..], &["ledger", "stat"]] {
        for date in [&[][..], &["-", "-"]] {
            for format in ["grid", "rows"] {
                runs.push([cmd, date, &["-f", format]].concat());
            }
        }
    }
    for args in runs {
        let out = ok(plc(&home, &args).arg("--color=always"));
        assert!(out.contains("\x1b[90m·"), "{args:?}: {out}");
        assert!(out.contains("\x1b[32m█"), "{args:?}: {out}");
        assert!(!out.contains("38;5;"), "{args:?}");
    }
}

#[test]
fn never_draws_plain_glyphs() {
    let home = with_vault("stat-never");
    let mut cmd = plc(&home, &["stat", "-", "-", "--color", "never"]);
    let out = ok(cmd.env("CLICOLOR_FORCE", "1"));
    assert!(!out.contains(ESC));
    assert!(out.contains("· empty"));
}

#[test]
fn auto_colours_a_pipe_only_when_forced() {
    let home = with_vault("stat-auto");
    let args = ["stat", "-", "-"];
    assert!(!ok(&mut plc(&home, &args)).contains(ESC));
    let forced = ok(plc(&home, &args).env("CLICOLOR_FORCE", "1"));
    assert!(forced.contains(ESC));
    let mut cmd = plc(&home, &args);
    let out = ok(cmd.env("CLICOLOR_FORCE", "1").env("NO_COLOR", "1"));
    assert!(!out.contains(ESC));
}

#[test]
fn a_format_must_fit_the_date() {
    let home = with_vault("stat-format");
    let err = |args: &[&str]| fail(&mut plc(&home, args));
    let names = "possible values: grid, rows, plot, list, tsv";
    assert!(err(&["stat", "-f", "git"]).contains(names));
    let list = "plc stat: -f list shows a day: DD MM YY";
    assert!(err(&["stat", "-", "-", "-f", "list"]).contains(list));
    let grid = "plc stat: -f grid shows a month or a year: - MM YY";
    assert!(err(&["stat", "05", "11", "26", "-f", "grid"]).contains(grid));
    let whole = "plc stat: a whole month needs a whole day: - - YY";
    assert!(err(&["stat", "05", "-", "26"]).contains(whole));
}

#[test]
fn days_count_the_bytes_under_each_stamp_across_the_vault() {
    let home = with_vault("stat-stamps");
    let a = "a".repeat(100);
    let b = "b".repeat(300);
    let weekly = format!(
        "isg 2026-11-02 08:00:00 +0100\n\n[[weekly]]\n{a}\n\
         isg 2026-11-05 08:00:00 +0100\n{b}\n"
    );
    home.write("palace/notes/w.md", &weekly);
    let daily = "notes/management/daily/2026/11/2026-11-10.md";
    home.write(&format!("palace/{daily}"), "isg 2026-11-10 09:00:00 +0100\n");
    let args = ["stat", "-", "11", "26", "-f", "rows"];
    let out = ok(plc(&home, &args).arg("--color=never"));
    let legend =
        "· empty   ░ ≤100 B   ▒ ≤100 B   ▓ ≤300 B   █ >300 B";
    assert!(out.contains(legend), "{out}");
    assert!(out.contains("Days written : 2 / 30"), "{out}");
    assert!(out.contains("Total        : 400 B"), "{out}");
}

#[test]
fn day_lists_the_notes_written_on_it() {
    let home = with_vault("stat-day");
    let weekly = "isg 2026-11-02 08:00:00 +0100\n\n[[weekly]]\nabc\n\
                  isg 2026-11-05 08:00:00 +0100\nde\n";
    home.write("palace/notes/w.md", weekly);
    home.write("palace/notes/x.md", "isg 2026-11-05 09:00:00 +0100\nxyzw\n");
    let day = |d: &str| ok(&mut home.plc(&["stat", d, "11", "26"]));
    let want = "
  Thu 2026-11-05   6 B in 2 notes

        4 B  notes/x.md
        2 B  notes/w.md";
    assert_eq!(day("05"), want);
    assert_eq!(day("06"), "\n  Fri 2026-11-06   nothing written");
    let tsv = ["stat", "05", "11", "26", "-f", "tsv"];
    assert_eq!(ok(&mut home.plc(&tsv)), "notes/x.md\t4\nnotes/w.md\t2");
    let month = ok(&mut home.plc(&["stat", "-", "11", "26", "-f", "tsv"]));
    let lines: Vec<&str> = month.lines().collect();
    assert_eq!(lines.len(), 30);
    assert_eq!(lines[0], "2026-11-01\t0");
    assert_eq!(lines[4], "2026-11-05\t6");
}

#[test]
fn ledger_stat_takes_the_same_dates() {
    let home = with_vault("stat-ledger");
    for (amount, memo, account) in
        [("4.50", "coffee", "cash"), ("12", "lunch", "bank")]
    {
        let args = ["ledger", "add", amount, memo, "-a", account, "-n"];
        ok(home.plc(&args).args(["-c", "food", "-d", "2026-11-05"]));
    }
    let day = ok(&mut home.plc(&["ledger", "stat", "05", "11", "26"]));
    let head = "\n  Thu 2026-11-05   16.50 EUR spend in 2 transactions\n\n";
    assert!(day.starts_with(head), "{day}");
    let lines: Vec<&str> = day.lines().skip(3).collect();
    assert!(lines[0].starts_with("       12.00  $ "), "{day}");
    assert!(lines[1].contains("coffee"), "{day}");

    let grep = ["ledger", "stat", "05", "11", "26", "-g", "coffee"];
    let one = ok(&mut home.plc(&grep));
    assert!(one.contains("4.50 EUR spend in 1 transaction"), "{one}");

    let month = ["ledger", "stat", "-", "11", "26", "-f", "tsv"];
    let tsv = ok(&mut home.plc(&month));
    let lines: Vec<&str> = tsv.lines().collect();
    assert_eq!((lines.len(), lines[4]), (30, "2026-11-05\t16.50"));

    let none = ok(&mut home.plc(&["ledger", "stat", "06", "11", "26"]));
    assert_eq!(none, "\n  Fri 2026-11-06   no spend");
}

#[test]
fn note_flags_belong_to_note_commands() {
    let home = with_vault("stat-note-flags");
    let err = fail(&mut home.plc(&["stat", "-x", "v2"]));
    assert!(err.contains("unexpected argument '-x'"), "{err}");
    let add = ["ledger", "add", "1", "x", "-a", "cash", "-x", "v"];
    assert!(fail(&mut home.plc(&add)).contains("unexpected argument '-x'"));
}
