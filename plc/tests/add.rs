//! End to end: `plc add NAME` (aliases `note`, `no`) seeds a stamped note in
//! the notes dir, which `plc config notes` prints.

mod common;

use std::fs;

use common::{display, fail, ok, Home, NO_VAULT};

/// Just the stamp line, e.g. `isg 2026-11-12 19:42:00 +0100`.
fn stamped(text: &str) -> bool {
    text.starts_with("isg 20") && text.lines().count() == 1
}

#[test]
fn defaults_to_vault_notes() {
    let home = Home::new("add-default");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let dir = home.path("palace/notes");
    assert_eq!(ok(&mut home.plc(&["config", "notes"])), display(&dir));

    let out = ok(&mut home.plc(&["add", "idea"]));
    assert_eq!(out, display(&dir.join("idea.md")));
    let text = fs::read_to_string(&out).unwrap();
    assert!(stamped(&text), "{text:?}");
}

#[test]
fn relative_notes_key_is_under_the_vault() {
    let home = Home::new("add-rel");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\nnotes = notes/inbox\n");
    let dir = home.path("palace/notes/inbox");
    assert_eq!(ok(&mut home.plc(&["config", "notes"])), display(&dir));
    let out = ok(&mut home.plc(&["add", "idea.md"]));
    assert_eq!(out, display(&dir.join("idea.md")));
}

#[test]
fn relative_notes_follow_palace_dir() {
    let home = Home::new("add-env");
    let vault = home.vault("other");
    home.write(".plc/.plcrc", "notes = inbox\n");
    let mut cmd = home.plc(&["config", "notes"]);
    cmd.env("PALACE_DIR", &vault);
    assert_eq!(ok(&mut cmd), display(&vault.join("inbox")));
}

#[test]
fn absolute_notes_need_no_vault() {
    let home = Home::new("add-abs");
    home.write(".plc/.plcrc", "notes = ~/loose\n");
    let dir = home.path("loose");
    assert_eq!(ok(&mut home.plc(&["config", "notes"])), display(&dir));
    let out = ok(&mut home.plc(&["no", "idea"]));
    assert_eq!(out, display(&dir.join("idea.md")));
}

#[test]
fn relative_notes_need_a_valid_vault() {
    let home = Home::new("add-novault");
    let want = format!("config: {NO_VAULT} ~/.plc/.plcrc");
    assert_eq!(fail(&mut home.plc(&["config", "notes"])), want);
    let want = format!("palace: {NO_VAULT} ~/.plc/.plcrc");
    assert_eq!(fail(&mut home.plc(&["add", "idea"])), want);

    home.write(".plc/.plcrc", "vault = ~/palace\n");
    fs::create_dir_all(home.path("palace")).unwrap();
    let err = fail(&mut home.plc(&["add", "idea"]));
    assert!(err.contains("has no 'notes/'"), "{err}");
    assert!(!home.path("palace/notes").exists());
}

#[test]
fn an_existing_note_is_kept() {
    let home = Home::new("add-keep");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let note = home.write("palace/notes/idea.md", "mine\n");
    assert_eq!(ok(&mut home.plc(&["note", "idea"])), display(&note));
    assert_eq!(fs::read_to_string(&note).unwrap(), "mine\n");
}

#[test]
fn global_tag_and_postfix_apply() {
    let home = Home::new("add-tag");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let out = ok(&mut home.plc(&["add", "idea", "-t", "review", "-x", "v2"]));
    assert!(out.ends_with("/notes/idea+v2.md"), "{out}");
    let text = fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("isg 20"), "{text:?}");
    assert!(text.ends_with("\n\n[[review]]\n"), "{text:?}");
}

#[test]
fn bad_names_are_refused() {
    let home = Home::new("add-bad");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    fail(&mut home.plc(&["add"]));
    assert_eq!(fail(&mut home.plc(&["add", " "])), "add: empty name");
    let err = fail(&mut home.plc(&["add", "../escape"]));
    let want = "add: name must not contain a path separator: ../escape";
    assert_eq!(err, want);
    assert!(!home.path("palace/escape.md").exists());
}
