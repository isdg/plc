//! End to end: `plc murmur NAME` opens a note of that name anywhere in the
//! vault, else creates it in the murmur dir; no name is an error.

mod common;

use std::fs;

use common::{display, fail, ok, Home};

const DIR: &str = "palace/notes/me/writing/murmur";

fn vault(tag: &str) -> Home {
    let home = Home::new(tag);
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    home
}

#[test]
fn a_name_is_required() {
    let home = vault("murmur-noname");
    let err = fail(&mut home.plc(&["murmur"]));
    assert_eq!(err, "murmur: a note name is required (or pass -l to list)");
}

#[test]
fn a_new_name_lands_in_the_murmur_dir() {
    let home = vault("murmur-new");
    let out = ok(&mut home.plc(&["murmur", "wind"]));
    assert_eq!(out, display(&home.path(&format!("{DIR}/wind.md"))));
    let text = fs::read_to_string(&out).unwrap();
    assert!(text.ends_with("\n\n[[murmur]]\n\nwind\n"), "{text:?}");
    assert_eq!(ok(&mut home.plc(&["murmur", "-n", "wind"])), out);
}

#[test]
fn a_note_elsewhere_in_the_vault_is_opened() {
    let home = vault("murmur-elsewhere");
    let note = home.write("palace/notes/triage/Wind.md", "mine\n");
    assert_eq!(ok(&mut home.plc(&["murmur", "wind"])), display(&note));
    assert_eq!(fs::read_to_string(&note).unwrap(), "mine\n");
    assert!(!home.path(&format!("{DIR}/wind.md")).exists());

    home.write("palace/notes/me/wind.md", "twin\n");
    let err = fail(&mut home.plc(&["murmur", "wind"]));
    let want = "murmur: 2 notes share the name: \
                notes/me/wind.md, notes/triage/Wind.md";
    assert_eq!(err, want);
}
