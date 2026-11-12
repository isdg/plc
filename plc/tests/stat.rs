//! End to end: `--color` picks between shade glyphs and coloured blocks in
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
fn always_draws_coloured_blocks_in_every_heatmap() {
    let home = with_vault("stat-always");
    for layout in ["git", "tab"] {
        let args = ["stat", "--type", "year", "--layout", layout];
        let out = ok(plc(&home, &args).arg("--color=always"));
        assert!(out.contains("\x1b[38;5;237m▇"), "{layout}: {out}");
        assert!(!out.contains('░'), "{layout}");
    }
    let out = ok(&mut plc(&home, &["stat", "--color", "always"]));
    assert!(out.contains('▇'));
    let args = ["ledger", "stat", "--type", "year", "--color", "always"];
    assert!(ok(&mut plc(&home, &args)).contains('▇'));
}

#[test]
fn never_draws_shade_glyphs() {
    let home = with_vault("stat-never");
    let mut cmd = plc(&home, &["stat", "--type", "year", "--color", "never"]);
    let out = ok(cmd.env("CLICOLOR_FORCE", "1"));
    assert!(!out.contains(ESC) && !out.contains('▇'));
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
