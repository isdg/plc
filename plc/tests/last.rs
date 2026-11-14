//! End to end: `plc todo` and `plc isg` list with `-l`, mark the last note
//! with `-L NAME`, and open it with `--last`, as with no flag.

mod common;

use common::{fail, ok, Home};

fn with_vault(tag: &str) -> Home {
    let home = Home::new(tag);
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    home
}

#[test]
fn do_lists_marks_and_opens_the_last() {
    let home = with_vault("last-do");
    let dir = "palace/notes/management/todo";
    home.write(&format!("{dir}/todo-2026-W01.md"), "a\n");
    let new = ok(&mut home.plc(&["todo", "-n"]));
    let mark = ok(&mut home.plc(&["todo", "-L", "todo-2026-W01"]));
    assert_eq!(mark, "Marked as last: todo-2026-W01.md");
    let list = ok(&mut home.plc(&["todo", "-l"]));
    assert!(list.contains("* todo-2026-W01.md"), "{list}");
    let last = ok(&mut home.plc(&["todo", "--last"]));
    assert_eq!(last, ok(&mut home.plc(&["todo"])));
    assert!(last.ends_with("todo-2026-W01.md") && last != new, "{last}");
    let err = fail(&mut home.plc(&["todo", "-L", "nope"]));
    assert!(err.contains("no such note"), "{err}");
    let old = fail(&mut home.plc(&["do", "-n"]));
    assert!(old.contains("unrecognized subcommand 'do'"), "{old}");
}

#[test]
fn isg_marks_and_opens_the_last() {
    let home = with_vault("last-isg");
    let first = ok(&mut home.plc(&["isg", "-n"]));
    let second = ok(&mut home.plc(&["isg", "-n"]));
    assert_eq!(ok(&mut home.plc(&["isg", "--last"])), second);
    let mark = ok(&mut home.plc(&["isg", "-L", "isg0"]));
    assert_eq!(mark, "Marked as last: isg0.md");
    assert_eq!(ok(&mut home.plc(&["isg", "--last"])), first);
    assert_eq!(ok(&mut home.plc(&["isg"])), first);
    let list = ok(&mut home.plc(&["isg", "-l"]));
    assert!(!list.contains('*'), "{list}");
    assert!(fail(&mut home.plc(&["isg", "-L", "isg9"])).contains("no such"));
    assert!(fail(&mut home.plc(&["isg", "--last", "-n"])).contains("cannot"));
}
