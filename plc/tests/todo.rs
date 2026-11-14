//! End to end: `plc doctor` renames do-notes to todo notes, and `plc todo`
//! waits for it.

mod common;

use std::fs;

use common::{fail, ok, Home};

const OLD: &str = "palace/notes/management/do";
const NEW: &str = "palace/notes/management/todo";

fn old_vault(tag: &str) -> Home {
    let home = Home::new(tag);
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let head = "isg 2026-11-07 09:00:00 +0100\n\n[[do]]\n";
    home.write(&format!("{OLD}/do-2026-W46.md"), &format!("{head}a\n"));
    home.write(&format!("{OLD}/do-2026-W47.md"), &format!("{head}b\n"));
    home.write(&format!("{OLD}/pic.png"), "png");
    home.write("palace/.plc/last-do", "do-2026-W47.md\n");
    let links = "see [[do-2026-W46|last week]], [[Do-2026-W47#plan]] \
                 and [[dog]]\n";
    home.write("palace/notes/x.md", links);
    home
}

fn read(home: &Home, rel: &str) -> String {
    fs::read_to_string(home.path(rel)).unwrap()
}

#[test]
fn doctor_reports_then_renames_the_do_notes() {
    let home = old_vault("todo-migrate");
    let err = fail(&mut home.plc(&["todo"]));
    assert!(err.contains("run `plc doctor --fix`"), "{err}");

    let report = ok(&mut home.plc(&["doctor"]));
    let want = "notes/management/do/do-2026-W46.md → \
                notes/management/todo/todo-2026-W46.md";
    assert!(report.contains(want), "{report}");
    assert!(report.contains(".plc/last-do → .plc/last-todo"), "{report}");
    assert!(report.contains("4 link(s) to rename in 3 note(s)"), "{report}");
    assert!(home.path(OLD).is_dir(), "a report changes nothing");

    let fixed = ok(&mut home.plc(&["doctor", "--fix"]));
    assert!(fixed.contains("fixed: renamed"), "{fixed}");
    assert!(!home.path(OLD).exists());
    assert!(home.path(&format!("{NEW}/pic.png")).is_file());
    let note = read(&home, &format!("{NEW}/todo-2026-W47.md"));
    assert_eq!(note, "isg 2026-11-07 09:00:00 +0100\n\n[[todo]]\nb\n");
    let links = read(&home, "palace/notes/x.md");
    let want = "see [[todo-2026-W46|last week]], [[todo-2026-W47#plan]] \
                and [[dog]]\n";
    assert_eq!(links, want);
    assert_eq!(read(&home, "palace/.plc/last-todo"), "todo-2026-W47.md\n");
    assert!(!home.path("palace/.plc/last-do").exists());

    let again = ok(&mut home.plc(&["doctor"]));
    assert!(again.contains("Todo — all good"), "{again}");
    let last = ok(&mut home.plc(&["todo"]));
    assert!(last.ends_with("notes/management/todo/todo-2026-W47.md"));
}

#[test]
fn doctor_renames_nothing_over_an_existing_note() {
    let home = old_vault("todo-conflict");
    home.write(&format!("{NEW}/todo-2026-W46.md"), "kept\n");
    let out = ok(&mut home.plc(&["doctor", "--fix"]));
    let exists = "notes/management/todo/todo-2026-W46.md already exists";
    assert!(out.contains(exists), "{out}");
    assert!(out.contains("nothing renamed"), "{out}");
    assert!(home.path(&format!("{OLD}/do-2026-W47.md")).is_file());
    assert_eq!(read(&home, &format!("{NEW}/todo-2026-W46.md")), "kept\n");
}
