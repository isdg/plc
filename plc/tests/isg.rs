//! End to end: `plc isg` opens the last note, `--new` (`-n`) creates the next
//! one, `-n --follow` (`-nf`) follows one up, and `--list` (`-l`) lists them.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{display, fail, ok, Home};

fn isg_home(tag: &str) -> (Home, PathBuf) {
    let home = Home::new(tag);
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let dir = home.path("palace/notes/me/writing/isg");
    (home, dir)
}

#[test]
fn new_creates_the_next_note_and_marks_it_last() {
    let (home, dir) = isg_home("isg-new");

    let first = ok(&mut home.plc(&["isg", "--new"]));
    assert_eq!(first, display(&dir.join("isg0.md")));
    let second = ok(&mut home.plc(&["isg", "-n"]));
    assert_eq!(second, display(&dir.join("isg1.md")));

    let pointer = home.path("palace/.plc/last-isg");
    assert_eq!(fs::read_to_string(pointer).unwrap(), "isg1.md\n");
    assert_eq!(ok(&mut home.plc(&["isg"])), second);
}

#[test]
fn opening_a_note_by_name_marks_it_last() {
    let (home, dir) = isg_home("isg-open");
    ok(&mut home.plc(&["isg", "-n"]));
    ok(&mut home.plc(&["isg", "-n"]));

    ok(&mut home.plc(&["isg", "isg0"]));
    assert_eq!(ok(&mut home.plc(&["isg"])), display(&dir.join("isg0.md")));
}

#[test]
fn follow_takes_the_last_note_or_the_given_one() {
    let (home, dir) = isg_home("isg-follow");
    ok(&mut home.plc(&["isg", "-n"]));
    ok(&mut home.plc(&["isg", "-n"]));
    ok(&mut home.plc(&["isg", "isg0"]));

    let last = ok(&mut home.plc(&["isg", "-nf"]));
    assert_eq!(last, display(&dir.join("isg0a.md")));
    let again = ok(&mut home.plc(&["isg", "-nf"]));
    assert_eq!(again, display(&dir.join("isg0b.md")));
    let given = ok(&mut home.plc(&["isg", "-nf", "1"]));
    assert_eq!(given, display(&dir.join("isg1a.md")));
    let long = ok(&mut home.plc(&["isg", "--new", "--follow", "1"]));
    assert_eq!(long, display(&dir.join("isg1b.md")));
}

#[test]
fn follow_needs_new() {
    let (home, _) = isg_home("isg-follow-alone");
    ok(&mut home.plc(&["isg", "-n"]));
    fail(&mut home.plc(&["isg", "-f"]));
    fail(&mut home.plc(&["isg", "--follow", "0"]));
}

#[test]
fn a_postfixed_note_is_marked_and_followed() {
    let (home, dir) = isg_home("isg-postfix");
    let made = ok(&mut home.plc(&["isg", "-n", "-x", "review"]));
    assert_eq!(made, display(&dir.join("isg0+review.md")));
    assert_eq!(ok(&mut home.plc(&["isg"])), made);
    let follow = ok(&mut home.plc(&["isg", "-nf"]));
    assert_eq!(follow, display(&dir.join("isg0a.md")));
    let next = ok(&mut home.plc(&["isg", "-n"]));
    assert_eq!(next, display(&dir.join("isg1.md")));
}

#[test]
fn a_stale_pointer_falls_back_to_the_newest_file() {
    let (home, dir) = isg_home("isg-stale");
    ok(&mut home.plc(&["isg", "-n"]));
    home.write("palace/.plc/last-isg", "isg9.md\n");
    assert_eq!(ok(&mut home.plc(&["isg"])), display(&dir.join("isg0.md")));
}

#[test]
fn l_lists_the_notes() {
    let (home, _) = isg_home("isg-list");
    ok(&mut home.plc(&["isg", "-n"]));
    assert_eq!(ok(&mut home.plc(&["isg", "-l"])), "isg0.md");
    assert_eq!(ok(&mut home.plc(&["isg", "--list"])), "isg0.md");
}

#[test]
fn old_flags_are_gone() {
    let (home, _) = isg_home("isg-old");
    ok(&mut home.plc(&["isg", "-n"]));
    for flag in ["-c", "--create", "--next", "--continue"] {
        fail(&mut home.plc(&["isg", flag]));
    }
}
