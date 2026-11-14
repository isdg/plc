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
    for cmd in [&["stat"][..], &["ledger", "stat"]] {
        for scope in ["month", "year"] {
            for layout in ["git", "tab"] {
                let args = ["--type", scope, "--layout", layout];
                let mut run = plc(&home, cmd);
                let out = ok(run.args(args).arg("--color=always"));
                assert!(out.contains("\x1b[90m·"), "{args:?}: {out}");
                assert!(out.contains("\x1b[32m█"), "{args:?}: {out}");
                assert!(!out.contains("38;5;"), "{args:?}");
            }
        }
    }
}

#[test]
fn never_draws_plain_glyphs() {
    let home = with_vault("stat-never");
    let mut cmd = plc(&home, &["stat", "--type", "year", "--color", "never"]);
    let out = ok(cmd.env("CLICOLOR_FORCE", "1"));
    assert!(!out.contains(ESC));
    assert!(out.contains("· empty"));
}

#[test]
fn auto_colours_a_pipe_only_when_forced() {
    let home = with_vault("stat-auto");
    let args = ["stat", "--type", "year"];
    assert!(!ok(&mut plc(&home, &args)).contains(ESC));
    let forced = ok(plc(&home, &args).env("CLICOLOR_FORCE", "1"));
    assert!(forced.contains(ESC));
    let mut cmd = plc(&home, &args);
    let out = ok(cmd.env("CLICOLOR_FORCE", "1").env("NO_COLOR", "1"));
    assert!(!out.contains(ESC));
}

#[test]
fn an_unknown_layout_is_refused() {
    let home = with_vault("stat-layout");
    let out = plc(&home, &["stat", "--layout", "grid"]).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("possible values: git, tab"), "{err}");
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
    let args = ["stat", "-m", "11", "-y", "2026", "--layout", "tab"];
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
    let day = |d: &str| ok(&mut home.plc(&["stat", "--day", d, "11", "26"]));
    let want = "
  Thu 2026-11-05   6 B in 2 notes

        4 B  notes/x.md
        2 B  notes/w.md";
    assert_eq!(day("05"), want);
    assert_eq!(day("06"), "\n  Fri 2026-11-06   nothing written");
    let mut both = home.plc(&["stat", "--day", "05", "11", "26", "-m", "3"]);
    fail(&mut both);
}
