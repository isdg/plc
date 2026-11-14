//! End to end: machine state lives in `<plc home>/<vault name>`, while the
//! vault's own `.plc` keeps what travels with it.

mod common;

use common::{fail, ok, Home};

const LOG: &str = "last-transactions";

fn with_vault(tag: &str) -> Home {
    let home = Home::new(tag);
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    home
}

#[test]
fn ledger_log_goes_to_the_state_dir() {
    let home = with_vault("state-log");
    ok(&mut home.plc(&["ledger", "last"]));
    assert!(home.path(".plc/palace").join(LOG).is_file());
    assert!(!home.path("palace/.plc").join(LOG).exists());
}

#[test]
fn legacy_log_moves_to_the_state_dir() {
    let home = with_vault("state-legacy");
    let legacy = home.write("palace/.plc/last-transactions", "stale\n");
    ok(&mut home.plc(&["ledger", "last"]));
    assert!(!legacy.exists());
    assert!(home.path(".plc/palace").join(LOG).is_file());
}

#[test]
fn legacy_log_is_dropped_beside_a_current_one() {
    let home = with_vault("state-both");
    home.write(".plc/palace/last-transactions", "current\n");
    let legacy = home.write("palace/.plc/last-transactions", "stale\n");
    ok(&mut home.plc(&["ledger", "last"]));
    assert!(!legacy.exists());
    assert!(home.path(".plc/palace").join(LOG).is_file());
}

#[test]
fn plc_home_relocates_the_state() {
    let home = Home::new("state-plc-home");
    home.vault("palace");
    home.write("elsewhere/.plcrc", "vault = ~/palace\n");
    let mut cmd = home.plc(&["ledger", "last"]);
    ok(cmd.env("PLC_HOME", home.path("elsewhere")));
    assert!(home.path("elsewhere/palace").join(LOG).is_file());
    assert!(!home.path(".plc").exists());
}

#[test]
fn do_pointer_stays_in_the_vault() {
    let home = with_vault("state-do");
    ok(&mut home.plc(&["todo", "-n"]));
    assert!(home.path("palace/.plc/last-do").is_file());
    assert!(!home.path(".plc/palace/last-do").exists());
}

#[test]
fn a_dot_name_is_no_state_dir() {
    let home = Home::new("state-dot");
    let vault = home.vault(".plcrc");
    let mut cmd = home.plc(&["ledger", "last"]);
    let err = fail(cmd.env("PALACE_DIR", &vault));
    let want = format!(
        "palace: vault '{}' can't name a state dir in ~/.plc: \
         its name must not be empty or start with '.'",
        vault.display()
    );
    assert_eq!(err, want);
    assert!(!home.path(".plc").exists());
}
