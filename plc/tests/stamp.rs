//! End to end: every note command joins the run's `--postfix` to its seeded
//! stamp's signature (`isg+review <date>`), as it does to the filename.

mod common;

use std::fs;

use plc_core::notes;

use common::{ok, Home};

fn first_line(home: &Home, args: &[&str]) -> String {
    let path = ok(&mut home.plc(args));
    let text = fs::read_to_string(path).unwrap();
    text.lines().next().unwrap().to_string()
}

#[test]
fn every_note_command_signs_its_stamp_with_the_postfix() {
    let home = Home::new("stamp-postfix");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let cases: [(&[&str], &str); 9] = [
        (&["daily", "-x", "on  call"], "isg+on+call"),
        (&["weekly", "-x", "plan"], "isg+plan"),
        (&["shot", "-x", "walk"], "isg+walk"),
        (&["shot", "-i", "hi", "-x", "talk"], "isg+talk"),
        (&["todo", "-n", "-x", "w"], "isg+w"),
        (&["top", "-x", "t"], "isg+t"),
        (&["add", "idea", "-x", "v2"], "isg+v2"),
        (&["murmur", "wind", "-x", "v2"], "isg+v2"),
        (&["isg", "-n", "-x", "review"], "isg0+review"),
    ];
    for (args, sign) in cases {
        let line = first_line(&home, args);
        assert!(line.starts_with(&format!("{sign} 20")), "{args:?}: {line}");
        assert!(notes::stamp(&line).is_some(), "{args:?}: {line}");
    }
}

#[test]
fn the_postfix_precedes_the_back_dated_mark() {
    let home = Home::new("stamp-mark");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    let line = first_line(&home, &["daily", "-1", "-x", "late"]);
    assert!(line.starts_with("isg+late 20"), "{line}");
    assert!(line.ends_with(" *"), "{line}");
}

#[test]
fn no_postfix_leaves_the_signature() {
    let home = Home::new("stamp-plain");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    assert!(first_line(&home, &["daily"]).starts_with("isg 20"));
    assert!(first_line(&home, &["isg", "-n"]).starts_with("isg0 20"));
}
