//! End to end: `plc doctor` takes the whitespace out of link targets and
//! the names of the files they reach.

mod common;

use std::fs;

use common::{ok, Home};

fn spaced_vault(tag: &str) -> Home {
    let home = Home::new(tag);
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    home.write("palace/notes/triage/foo bar.md", "see [[Big  Idea]]\n");
    home.write("palace/notes/triage/Big Idea.md", "idea\n");
    home.write("palace/notes/triage/scan 1.pdf", "pdf");
    home.write("palace/.obsidian/a b.json", "{}");
    let text = "[[foo bar|Foo Bar]] [[n/Big Idea#Some Part]] \
                ![[scan 1.pdf]] [[ok_name]] [[ pad ]]\n\
                [[open\n- [[x]]\n";
    home.write("palace/notes/x.md", text);
    home
}

fn read(home: &Home, rel: &str) -> String {
    fs::read_to_string(home.path(rel)).unwrap()
}

#[test]
fn doctor_reports_then_unspaces_names() {
    let home = spaced_vault("links-unspace");
    let report = ok(&mut home.plc(&["doctor"]));
    let want = "  ! 3 file(s) to rename:
      notes/triage/Big Idea.md → notes/triage/Big_Idea.md
      notes/triage/foo bar.md → notes/triage/foo_bar.md
      notes/triage/scan 1.pdf → notes/triage/scan_1.pdf
  ! 4 link(s) to rewrite in 2 note(s):
      [[Big  Idea]] → [[Big_Idea]]
      [[Big Idea]] → [[Big_Idea]]
      [[foo bar]] → [[foo_bar]]
      [[scan 1.pdf]] → [[scan_1.pdf]]

  run `plc doctor --fix` to rename them";
    assert!(report.contains(want), "{report}");
    assert!(home.path("palace/notes/triage/foo bar.md").is_file());

    let fixed = ok(&mut home.plc(&["doctor", "--fix"]));
    assert!(fixed.contains("fixed: renamed the files and links"), "{fixed}");
    let x = read(&home, "palace/notes/x.md");
    let want = "[[foo_bar|Foo Bar]] [[n/Big_Idea#Some Part]] \
                ![[scan_1.pdf]] [[ok_name]] [[ pad ]]\n\
                [[open\n- [[x]]\n";
    assert_eq!(x, want);
    let moved = read(&home, "palace/notes/triage/foo_bar.md");
    assert_eq!(moved, "see [[Big_Idea]]\n");
    assert!(home.path("palace/notes/triage/scan_1.pdf").is_file());
    assert!(home.path("palace/.obsidian/a b.json").is_file());

    let again = ok(&mut home.plc(&["doctor"]));
    assert!(again.contains("Links — all good"), "{again}");
}

#[test]
fn doctor_renames_nothing_over_a_taken_name() {
    let home = spaced_vault("links-taken");
    home.write("palace/notes/triage/foo_bar.md", "kept\n");
    home.write("palace/notes/a  b.md", "");
    home.write("palace/notes/a b.md", "");
    let out = ok(&mut home.plc(&["doctor", "--fix"]));
    assert!(out.contains("notes/triage/foo_bar.md already taken"), "{out}");
    assert!(out.contains("notes/a_b.md already taken"), "{out}");
    assert!(out.contains("nothing renamed"), "{out}");
    assert!(home.path("palace/notes/triage/foo bar.md").is_file());
    assert_eq!(read(&home, "palace/notes/triage/foo_bar.md"), "kept\n");
    assert!(read(&home, "palace/notes/x.md").starts_with("[[foo bar|"));
}
