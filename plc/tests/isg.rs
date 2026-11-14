//! End to end: `plc isg --next` (short `-n`) creates the next enumerated note,
//! and `--continue` (short `-c`) continues one with a letter suffix.

mod common;

use common::{display, fail, ok, Home};

#[test]
fn next_creates_the_next_enumerated_note() {
    let home = Home::new("isg-next");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let dir = home.path("palace/notes/me/writing/isg");

    let first = ok(&mut home.plc(&["isg", "--next"]));
    assert_eq!(first, display(&dir.join("isg0.md")));
    let second = ok(&mut home.plc(&["isg", "-n"]));
    assert_eq!(second, display(&dir.join("isg1.md")));
    assert_eq!(ok(&mut home.plc(&["isg"])), second);
}

#[test]
fn c_continues_the_latest_or_the_given_note() {
    let home = Home::new("isg-continue");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let dir = home.path("palace/notes/me/writing/isg");
    ok(&mut home.plc(&["isg", "-n"]));
    ok(&mut home.plc(&["isg", "-n"]));

    let latest = ok(&mut home.plc(&["isg", "-c"]));
    assert_eq!(latest, display(&dir.join("isg1a.md")));
    let given = ok(&mut home.plc(&["isg", "-c", "0"]));
    assert_eq!(given, display(&dir.join("isg0a.md")));
    let long = ok(&mut home.plc(&["isg", "--continue", "0"]));
    assert_eq!(long, display(&dir.join("isg0b.md")));
}

#[test]
fn create_is_gone() {
    let home = Home::new("isg-create");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    fail(&mut home.plc(&["isg", "--create"]));
}
