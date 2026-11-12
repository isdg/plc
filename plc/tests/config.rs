//! End to end: how the built `plc` finds the vault — `$PALACE_DIR`, else
//! `vault` in `<plc home>/.plcrc`.

mod common;

use std::fs;
use std::os::unix::fs::symlink;

use common::{display, fail, ok, Home, NO_VAULT};

#[test]
fn palace_dir_with_tilde_expanded() {
    let home = Home::new("env");
    let out = ok(home.plc(&["config"]).env("PALACE_DIR", "~/vault"));
    assert_eq!(out, display(&home.path("vault")));
}

#[test]
fn env_beats_rc() {
    let home = Home::new("env-rc");
    home.write(".plc/.plcrc", "vault = /from/rc\n");
    let out = ok(home.plc(&["config"]).env("PALACE_DIR", "/from/env"));
    assert_eq!(out, "/from/env");
}

#[test]
fn rc_when_env_unset_or_empty() {
    let home = Home::new("rc");
    let rc = "# defaults\n\nvault = ~/palace\nfuture = 1\n";
    home.write(".plc/.plcrc", rc);
    let want = display(&home.path("palace"));
    assert_eq!(ok(&mut home.plc(&["config"])), want);
    for empty in ["", "  "] {
        let out = ok(home.plc(&["config"]).env("PALACE_DIR", empty));
        assert_eq!(out, want);
    }
}

#[test]
fn neither_is_an_error() {
    let home = Home::new("neither");
    let want = format!("config: {NO_VAULT} ~/.plc/.plcrc");
    assert_eq!(fail(&mut home.plc(&["config"])), want);
    home.write(".plc/.plcrc", "# no vault here\nvault =\n");
    assert_eq!(fail(&mut home.plc(&["config"])), want);

    let want = format!("palace: {NO_VAULT} ~/.plc/.plcrc");
    assert_eq!(fail(&mut home.plc(&["weekly"])), want);
    let want = format!("init: no DIR given; {NO_VAULT} ~/.plc/.plcrc");
    assert_eq!(fail(&mut home.plc(&["init"])), want);
}

#[test]
fn plc_home_relocates_the_rc() {
    let home = Home::new("plc-home");
    home.write(".plc/.plcrc", "vault = /default\n");
    let elsewhere = home.path("elsewhere");
    let mut cmd = home.plc(&["config"]);
    cmd.env("PLC_HOME", &elsewhere);
    let want = format!("config: {NO_VAULT} ~/elsewhere/.plcrc");
    assert_eq!(fail(&mut cmd), want);

    home.write("elsewhere/.plcrc", "vault = /relocated\n");
    assert_eq!(ok(&mut cmd), "/relocated");
}

const HINT: &str = "(rename its PALACE_DIR key to `vault`)";

#[test]
fn legacy_rc_moves_into_plc_home() {
    let home = Home::new("adopt");
    let legacy = "export PALACE_DIR=\"/v\"\n";
    home.write(".plcrc", legacy);
    let want = format!("config: {NO_VAULT} ~/.plc/.plcrc {HINT}");
    assert_eq!(fail(&mut home.plc(&["config"])), want);
    assert!(!home.path(".plcrc").exists());
    let moved = fs::read_to_string(home.path(".plc/.plcrc")).unwrap();
    assert_eq!(moved, legacy);
}

#[test]
fn legacy_rc_never_replaces_one() {
    let home = Home::new("no-adopt");
    home.write(".plcrc", "PALACE_DIR = /old\n");
    home.write(".plc/.plcrc", "vault = /new\n");
    assert_eq!(ok(&mut home.plc(&["config"])), "/new");
    assert!(home.path(".plcrc").exists());

    let rc = home.path(".plc/.plcrc");
    fs::remove_file(&rc).unwrap();
    symlink(home.path("dotfiles/plcrc"), &rc).unwrap();
    let want = format!("config: {NO_VAULT} ~/.plc/.plcrc");
    assert_eq!(fail(&mut home.plc(&["config"])), want);
    assert!(home.path(".plcrc").exists());
    assert!(fs::symlink_metadata(&rc).unwrap().is_symlink());
}

#[test]
fn legacy_rc_stays_under_plc_home() {
    let home = Home::new("adopt-plc-home");
    home.write(".plcrc", "PALACE_DIR = /old\n");
    let mut cmd = home.plc(&["config"]);
    cmd.env("PLC_HOME", home.path("elsewhere"));
    let want = format!("config: {NO_VAULT} ~/elsewhere/.plcrc");
    assert_eq!(fail(&mut cmd), want);
    assert!(home.path(".plcrc").exists());
    assert!(!home.path("elsewhere").exists());
    assert!(!home.path(".plc").exists());
}

#[test]
fn vault_is_the_default_key() {
    let home = Home::new("key");
    home.write(".plc/.plcrc", "vault = /v\n");
    assert_eq!(ok(&mut home.plc(&["config", "vault"])), "/v");
    fail(&mut home.plc(&["config", "nope"]));
}

#[test]
fn cats_lists_the_dirs_in_notes() {
    let home = Home::new("cats");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let err = fail(&mut home.plc(&["config", "cats"]));
    assert!(err.starts_with("config: read ~/palace/notes: "), "{err}");

    for dir in ["triage", "me", ".git"] {
        fs::create_dir_all(home.path(&format!("palace/notes/{dir}"))).unwrap();
    }
    home.write("palace/notes/loose.md", "x\n");
    let want = [home.path("palace/notes/me"), home.path("palace/notes/triage")]
        .map(|p| display(&p))
        .join("\n");
    assert_eq!(ok(&mut home.plc(&["config", "cats"])), want);
}

#[test]
fn set_and_rc_flags_are_gone() {
    let home = Home::new("set");
    for flag in ["--set", "--rc"] {
        fail(&mut home.plc(&["config", flag, "/elsewhere"]));
    }
    assert!(!home.path(".plcrc").exists());
    assert!(!home.path(".plc").exists());
}
