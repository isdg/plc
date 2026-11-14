//! End to end: `plc isg --next` (short `-n`) creates the next enumerated note.

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
fn create_is_gone() {
    let home = Home::new("isg-create");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    fail(&mut home.plc(&["isg", "-c"]));
    fail(&mut home.plc(&["isg", "--create"]));
}
