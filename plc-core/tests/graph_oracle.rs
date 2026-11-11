//! `graph::layout` against real `git log --graph`: each DAG becomes a
//! throwaway repository, one empty commit per row, and both drawings must
//! match byte for byte. Skipped when `git` is not installed.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use plc_core::graph::layout;

fn git(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    cmd
}

fn have_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

struct Repo(PathBuf);

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What git draws for `parents` (row → parent rows, row 0 newest).
fn git_graph(name: &str, parents: &[Vec<usize>]) -> String {
    let dir = std::env::temp_dir().join(format!(
        "plc-graph-oracle-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let repo = Repo(dir);
    assert!(git(&repo.0).args(["init", "-q"]).status().unwrap().success());

    let n = parents.len();
    let mut stream = String::new();
    for row in (0..n).rev() {
        let msg = format!("c{row}\n");
        let when = 1_700_000_000 + (n - row) as u64 * 60;
        let _ = write!(
            stream,
            "commit refs/heads/c{row}\nmark :{}\n\
             committer t <t@t> {when} +0000\ndata {}\n{msg}",
            row + 1,
            msg.len()
        );
        for (k, p) in parents[row].iter().enumerate() {
            let verb = if k == 0 { "from" } else { "merge" };
            let _ = writeln!(stream, "{verb} :{}", p + 1);
        }
        stream.push('\n');
    }
    let mut import = git(&repo.0)
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    import
        .stdin
        .take()
        .unwrap()
        .write_all(stream.as_bytes())
        .unwrap();
    assert!(import.wait().unwrap().success());

    let out = git(&repo.0)
        .args([
            "log",
            "--graph",
            "--date-order",
            "--no-color",
            "--no-graph-indent",
            "--format=%s",
            "--branches",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    strip_ansi(&String::from_utf8(out.stdout).unwrap())
}

/// Git colours an octopus merge's dashes even under `--no-color`.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            chars.by_ref().find(|&c| c == 'm');
        } else {
            out.push(c);
        }
    }
    out
}

/// Our drawing in git's terms: roots are `o` here, `*` in git.
fn ours(parents: &[Vec<usize>]) -> String {
    let mut s = String::new();
    for line in layout(parents) {
        match line.node {
            Some(row) => {
                let root = parents[row].is_empty();
                assert_eq!(line.graph.contains('o'), root, "row {row}");
                s.push_str(&line.graph.replace('o', "*"));
                let _ = write!(s, "c{row}");
            }
            None => s.push_str(&line.graph),
        }
        s.push('\n');
    }
    s
}

fn check(name: &str, parents: &[Vec<usize>]) {
    let want = git_graph(name, parents);
    let got = ours(parents);
    if want != got {
        let at = want
            .lines()
            .zip(got.lines())
            .position(|(w, g)| w != g)
            .unwrap_or(want.lines().count().min(got.lines().count()));
        panic!(
            "{name}: first difference at line {}\n\
             --- git\n{want}--- ours\n{got}parents: {parents:?}",
            at + 1
        );
    }
}

/// xorshift64*, so the random DAGs are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// `n` rows; each draws 0–`max_parents` parents, mostly near, some far.
fn random_dag(seed: u64, n: usize, max_parents: usize) -> Vec<Vec<usize>> {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    (0..n)
        .map(|row| {
            let older = n - row - 1;
            if older == 0 {
                return vec![];
            }
            let want = match rng.below(20) {
                0..=2 => 0,
                3..=13 => 1,
                14..=17 => 2,
                _ => 3 + rng.below(max_parents.saturating_sub(2).max(1)),
            }
            .min(older)
            .min(max_parents);
            let mut ps = Vec::new();
            while ps.len() < want {
                let reach = if rng.below(4) == 0 { older } else { 6 };
                let p = row + 1 + rng.below(reach.min(older));
                if !ps.contains(&p) {
                    ps.push(p);
                }
            }
            ps
        })
        .collect()
}

#[test]
fn hand_picked_shapes_match_git() {
    if !have_git() {
        eprintln!("git not found: skipped");
        return;
    }
    let shapes: &[(&str, Vec<Vec<usize>>)] = &[
        ("chain", vec![vec![1], vec![2], vec![]]),
        ("merge", vec![vec![1, 2], vec![3], vec![3], vec![]]),
        ("two-roots", vec![vec![2], vec![], vec![]]),
        ("lone-rows", vec![vec![], vec![], vec![3], vec![]]),
        ("fork", vec![vec![2], vec![2], vec![]]),
        ("octopus", vec![vec![1, 2, 3], vec![4], vec![4], vec![4], vec![]]),
        (
            "octopus-right",
            vec![vec![5], vec![2, 3, 4], vec![5], vec![5], vec![5], vec![]],
        ),
        (
            "left-merge",
            vec![vec![2], vec![2, 3], vec![4], vec![4], vec![]],
        ),
        ("hub", vec![vec![5], vec![5], vec![5], vec![5], vec![5], vec![]]),
        (
            "crossing",
            vec![vec![3], vec![2], vec![4], vec![5], vec![5], vec![]],
        ),
    ];
    for (name, dag) in shapes {
        check(name, dag);
    }
}

#[test]
fn random_dags_match_git() {
    if !have_git() {
        eprintln!("git not found: skipped");
        return;
    }
    // PLC_ORACLE_SEEDS widens the sweep when the layout changes.
    let seeds = std::env::var("PLC_ORACLE_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60u64);
    for seed in 0..seeds {
        let n = 10 + (seed as usize * 37) % 190;
        let max_parents = 2 + (seed as usize % 6);
        check(&format!("seed{seed}"), &random_dag(seed, n, max_parents));
    }
}
