//! End to end: `plc graph` on a small vault, as text and as JSON.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

struct Vault(PathBuf);

impl Vault {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir()
            .join(format!("plc-graph-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("notes")).unwrap();
        Vault(dir)
    }

    fn note(&self, rel: &str, day: u32, body: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let stamp = format!("isg 2026-01-{day:02} 10:00:00 +0000");
        fs::write(path, format!("{stamp}\n\n{body}\n")).unwrap();
    }

    fn graph(&self, args: &[&str]) -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_plc"))
            .arg("graph")
            .args(args)
            .env("PALACE_DIR", &self.0)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{err}");
        String::from_utf8(out.stdout).unwrap()
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample(tag: &str) -> Vault {
    let v = Vault::new(tag);
    v.note("notes/daily.md", 1, "[[later]]");
    v.note("notes/later.md", 2, "");
    v.note("notes/d1.md", 3, "[[daily]] [[revolut]]");
    v.note("notes/lone.md", 4, "[[tag]]");
    v.note("notes/d2.md", 5, "[[daily]] [[d1]]");
    v.note(".git/hidden.md", 6, "[[daily]]");
    v
}

#[test]
fn text_draws_notes_as_git_log() {
    let v = sample("text");
    assert_eq!(
        v.graph(&[]),
        "*   2026-01-05  d2\n\
         |\\\n\
         | | o 2026-01-04  lone (tag)\n\
         | * 2026-01-03  d1 (revolut)\n\
         |/\n\
         | o 2026-01-02  later\n\
         o 2026-01-01  daily ->1\n"
    );
}

#[test]
fn linked_leaves_out_notes_without_links() {
    let v = sample("linked");
    let text = v.graph(&["--linked"]);
    assert!(!text.contains("lone"), "{text}");
    assert!(text.contains("later"), "{text}");
}

fn rows(json: &str) -> Vec<Value> {
    json.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn json_maps_lines_to_notes() {
    let v = sample("json");
    let lines = rows(&v.graph(&["--json"]));
    let notes: Vec<&Value> =
        lines.iter().filter_map(|l| l.get("note")).collect();
    let names: Vec<&str> =
        notes.iter().map(|n| n["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["d2", "lone", "d1", "later", "daily"]);
    for (row, n) in notes.iter().enumerate() {
        assert_eq!(n["row"], row);
    }
    assert_eq!(notes[0]["parents"], serde_json::json!([4, 2]));
    assert_eq!(notes[4]["children"], serde_json::json!([0, 2]));
    assert_eq!(notes[4]["forward"], 1);
    assert_eq!(notes[2]["refs"], serde_json::json!(["revolut"]));
    let path = Path::new(notes[2]["path"].as_str().unwrap());
    assert_eq!(path, v.0.join("notes/d1.md"));
    assert!(lines.iter().all(|l| l["graph"].is_string()));
}
