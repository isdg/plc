//! End to end: a fresh vault takes todo notes.

mod common;

use common::{ok, Home};

#[test]
fn a_fresh_init_takes_todo_notes() {
    let home = Home::new("init-todo");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    ok(&mut home.plc(&["init"]));
    let note = ok(&mut home.plc(&["todo", "-n"]));
    assert!(note.contains("/notes/management/todo/"), "{note}");
}
