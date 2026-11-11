//! End to end: how the built `plc` finds the vault — `$PALACE_DIR`, else
//! `vault` in `<plc home>/.plcrc`.

mod common;

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

#[test]
fn set_and_rc_flags_are_gone() {
    let home = Home::new("set");
    for flag in ["--set", "--rc"] {
        fail(&mut home.plc(&["config", flag, "/elsewhere"]));
    }
    assert!(!home.path(".plcrc").exists());
    assert!(!home.path(".plc").exists());
}
