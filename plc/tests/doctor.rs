//! End to end: `plc doctor` reports what it finds in `.plcrc`.

mod common;

use common::{ok, Home};

#[test]
fn reports_unknown_keys() {
    let home = Home::new("doctor-keys");
    home.vault("palace");
    let rc = "vault = ~/palace\nfoo = 1\nexport PALACE_DIR=/x\n";
    home.write(".plc/.plcrc", rc);
    let out = ok(&mut home.plc(&["doctor"]));
    let want = "
  ! ~/.plc/.plcrc: unknown key `foo` (ignored)
  ! ~/.plc/.plcrc: unknown key `export PALACE_DIR` (ignored)
      rename its PALACE_DIR key to `vault`
  · vault OK → ";
    assert!(out.contains(want), "{out}");
}

#[test]
fn reports_an_unmoved_legacy_rc() {
    let home = Home::new("doctor-legacy");
    home.write(".plcrc", "PALACE_DIR = /old\n");
    home.write(".plc/.plcrc", "vault = /new\n");
    let out = ok(&mut home.plc(&["doctor"]));
    let want = "
  ! ~/.plcrc not moved: ~/.plc/.plcrc exists
      merge it into ~/.plc/.plcrc, then delete it
";
    assert!(out.contains(want), "{out}");
    assert!(!out.contains("unknown key"), "{out}");
}

#[test]
fn quiet_when_the_rc_is_clean() {
    let home = Home::new("doctor-clean");
    home.vault("palace");
    let rc = "# defaults\nvault = ~/palace\nnotes = inbox\n";
    home.write(".plc/.plcrc", rc);
    let out = ok(&mut home.plc(&["doctor"]));
    assert!(!out.contains("  ! "), "{out}");
}
