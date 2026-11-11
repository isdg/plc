//! `plc graph` — the vault drawn the way `git log --graph` draws history:
//! every note a row, newest first; a link to an older note is a parent
//! edge, so a note linking several is a merge. Roots are drawn `o`.
//!
//! Text rows read `<lanes> <date>  <name> (<refs>) -><n>`: refs are link
//! names that are no note, `-><n>` counts links to newer notes, which draw
//! no line. `--json` prints one object per graph line for an editor.

use std::fmt::Write as _;

use clap::Args;
use plc_core::graph::{Dag, Note};
use serde_json::json;

use crate::config::Palace;

#[derive(Args)]
pub struct GraphArgs {
    /// One JSON object per line: `graph`, plus `note` on a note's own row
    /// (its parents and children as row numbers).
    #[arg(long)]
    json: bool,
    /// Leave out notes no link joins to another note.
    #[arg(long)]
    linked: bool,
}

pub fn run(palace: &Palace, args: GraphArgs) -> Result<String, String> {
    let dag = Dag::scan(palace.root());
    let dag = if args.linked { dag.linked() } else { dag };
    Ok(if args.json { json_lines(&dag) } else { text(&dag) })
}

fn text(dag: &Dag) -> String {
    let mut out = String::new();
    for line in dag.lines() {
        match line.node {
            Some(row) => {
                let n = &dag.notes[row];
                let _ = write!(out, "{}{}  {}", line.graph, n.date, n.name);
                if !n.refs.is_empty() {
                    let _ = write!(out, " ({})", n.refs.join(", "));
                }
                if n.forward > 0 {
                    let _ = write!(out, " ->{}", n.forward);
                }
            }
            None => out.push_str(line.graph.trim_end()),
        }
        out.push('\n');
    }
    out.pop();
    out
}

fn json_lines(dag: &Dag) -> String {
    let lines: Vec<String> = dag
        .lines()
        .into_iter()
        .map(|line| {
            let value = match line.node {
                Some(row) => json!({
                    "graph": line.graph,
                    "note": note(row, &dag.notes[row]),
                }),
                None => json!({ "graph": line.graph }),
            };
            value.to_string()
        })
        .collect();
    lines.join("\n")
}

fn note(row: usize, n: &Note) -> serde_json::Value {
    json!({
        "row": row,
        "path": n.path.to_string_lossy(),
        "name": n.name,
        "date": n.date,
        "parents": n.parents,
        "children": n.children,
        "refs": n.refs,
        "forward": n.forward,
    })
}
