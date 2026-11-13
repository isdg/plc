//! End to end: `--color` picks between plain and coloured shade glyphs in
//! the `plc stat` and `plc ledger stat` heatmaps.

mod common;

use std::process::Command;

use common::{ok, Home};

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
