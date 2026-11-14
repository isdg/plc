//! End to end: `plc isg --new` (short `-n`) creates the next enumerated note,
//! and `--follow` (short `-f`) follows one up with a letter suffix.

mod common;

use common::{display, fail, ok, Home};

#[test]
fn new_creates_the_next_enumerated_note() {
    let home = Home::new("isg-new");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let dir = home.path("palace/notes/me/writing/isg");

    let first = ok(&mut home.plc(&["isg", "--new"]));
    assert_eq!(first, display(&dir.join("isg0.md")));
    let second = ok(&mut home.plc(&["isg", "-n"]));
    assert_eq!(second, display(&dir.join("isg1.md")));
    assert_eq!(ok(&mut home.plc(&["isg"])), second);
}

#[test]
fn follow_adds_a_letter_to_the_latest_or_given() {
    let home = Home::new("isg-follow");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let dir = home.path("palace/notes/me/writing/isg");
    ok(&mut home.plc(&["isg", "-n"]));
    ok(&mut home.plc(&["isg", "-n"]));

    let latest = ok(&mut home.plc(&["isg", "-f"]));
    assert_eq!(latest, display(&dir.join("isg1a.md")));
    let given = ok(&mut home.plc(&["isg", "-f", "0"]));
    assert_eq!(given, display(&dir.join("isg0a.md")));
    let long = ok(&mut home.plc(&["isg", "--follow", "0"]));
    assert_eq!(long, display(&dir.join("isg0b.md")));
}

#[test]
fn old_flags_are_gone() {
    let home = Home::new("isg-old");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    ok(&mut home.plc(&["isg", "-n"]));
    for flag in ["-c", "--create", "--next", "--continue"] {
        fail(&mut home.plc(&["isg", flag]));
    }
}
