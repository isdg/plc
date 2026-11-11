//! Lanes for a DAG drawn the way `git log --graph` draws history.
//!
//! Rows are given newest first and each row lists its parents as row
//! indices, all greater than its own. The output is git's: one `*` line per
//! row, plus the lines git adds around merges (`|\`), octopus merges
//! (`*-.`, `|\ \`) and lanes collapsing leftwards (`|/`, `_`). Each line is
//! padded to the width of the row it belongs to, so text placed after it
//! stays aligned. `plc-core/tests/graph_oracle.rs` holds it to real git
//! (`--no-graph-indent`).
//!
//! One departure: a row without parents is drawn `o`, so a root directly
//! above an unrelated row can't be read as its child.

use std::mem;

/// One line of the drawing: the lane characters and, on a row's own `*`
/// line, that row's index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub graph: String,
    pub node: Option<usize>,
}

/// Lay out `parents` (row → parent rows, oldest last in the drawing).
///
/// # Panics
///
/// If a row names a parent at or above itself.
pub fn layout(parents: &[Vec<usize>]) -> Vec<Line> {
    let mut lanes = Lanes::default();
    let mut out = Vec::new();
    for (row, ps) in parents.iter().enumerate() {
        assert!(ps.iter().all(|&p| p > row), "row {row}: parent above it");
        lanes.start(row, ps);
        loop {
            let (graph, was_commit) = lanes.next_line();
            out.push(Line {
                graph,
                node: was_commit.then_some(row),
            });
            if lanes.state == State::Idle {
                break;
            }
        }
    }
    out
}

const MERGE_CHARS: [char; 3] = ['/', '|', '\\'];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    Expanding,
    Commit,
    Merged,
    Collapsing,
}

/// The drawing state between rows. A lane holds the row it is waiting
/// for; `slots` maps each screen column (two per lane: the lane and the gap
/// after it) of the row being drawn to the lane it heads for, or -1.
#[derive(Default)]
struct Lanes {
    row: usize,
    parents: Vec<usize>,
    lanes: Vec<usize>,
    next: Vec<usize>,
    slots: Vec<isize>,
    prev_slots: Vec<isize>,
    used: usize,
    width: usize,
    at: isize,
    prev_at: isize,
    expansion: usize,
    // 0: the first parent lies left of the row, 1: below or right of it;
    // -1 before the first parent is placed.
    layout: isize,
    added: isize,
    prev_added: isize,
    state: State,
    prev_state: State,
}

impl Lanes {
    fn start(&mut self, row: usize, parents: &[usize]) {
        self.row = row;
        self.parents = parents.to_vec();
        self.prev_at = self.at;
        self.place();
        self.expansion = 0;
        // Not `set`: the previous state's last line is the one that counts.
        self.state = if self.needs_expansion() {
            State::Expanding
        } else {
            State::Commit
        };
    }

    fn set(&mut self, state: State) {
        self.prev_state = self.state;
        self.state = state;
    }

    fn n(&self) -> isize {
        self.parents.len() as isize
    }

    /// Parents of an octopus beyond the two lanes next to it: each costs
    /// two dashes on its line and two rows of room above it.
    fn spread(&self) -> isize {
        self.n() + self.layout - 3
    }

    fn needs_expansion(&self) -> bool {
        let rows = self.spread() * 2;
        self.n() >= 3
            && self.at < self.lanes.len() as isize - 1
            && (self.expansion as isize) < rows
    }

    /// Work out the lanes below this row and where each column heads.
    fn place(&mut self) {
        mem::swap(&mut self.lanes, &mut self.next);
        self.next.clear();
        // The commit line reads where the last collapse left each lane.
        self.prev_slots.clear();
        self.prev_slots.extend_from_slice(&self.slots[..self.used]);
        let size = 2 * (self.lanes.len() + self.parents.len());
        self.slots.clear();
        self.slots.resize(size, -1);
        self.used = size;
        self.width = 0;
        self.prev_added = self.added;
        self.added = 0;

        let mut seen = false;
        for i in 0..=self.lanes.len() {
            let waits_for = if i == self.lanes.len() {
                if seen {
                    break;
                }
                self.row
            } else {
                self.lanes[i]
            };
            if waits_for == self.row {
                seen = true;
                self.at = i as isize;
                self.layout = -1;
                for p in self.parents.clone() {
                    self.push_lane(p, i as isize);
                }
                if self.parents.is_empty() {
                    self.width += 2;
                }
            } else {
                self.push_lane(waits_for, -1);
            }
        }
        while self.used > 1 && self.slots[self.used - 1] < 0 {
            self.used -= 1;
        }
    }

    /// Route `waits_for` into the lanes below; `from` is this row's lane
    /// when it is one of the row's parents, else -1.
    fn push_lane(&mut self, waits_for: usize, from: isize) {
        let lane = match self.next.iter().position(|&r| r == waits_for) {
            Some(lane) => lane,
            None => {
                self.next.push(waits_for);
                self.next.len() - 1
            }
        } as isize;
        let w = self.width as isize;
        let slot = if self.n() > 1 && from > -1 && self.layout == -1 {
            let dist = from - lane;
            let shift = if dist > 1 { 2 * dist - 3 } else { 1 };
            self.layout = if dist > 0 { 0 } else { 1 };
            self.added = self.n() + self.layout - 2;
            self.width += 2 * self.layout as usize;
            w + (self.layout - 1) * shift
        } else if self.added > 0 && w >= 2 && lane == self.slots[w as usize - 2]
        {
            // A lane a merge opened would run right beside the lane it
            // joins: join them at once instead.
            self.added = -1;
            w - 2
        } else {
            self.width += 2;
            w
        };
        self.slots[slot as usize] = lane;
    }

    fn settled(&self) -> bool {
        self.slots[..self.used]
            .iter()
            .enumerate()
            .all(|(i, &t)| t < 0 || t == (i / 2) as isize)
    }

    fn next_line(&mut self) -> (String, bool) {
        let mut line = String::new();
        let was_commit = self.state == State::Commit;
        match self.state {
            State::Idle => {
                for _ in &self.next {
                    line.push_str("| ");
                }
            }
            State::Expanding => self.expanding_line(&mut line),
            State::Commit => self.commit_line(&mut line),
            State::Merged => self.merged_line(&mut line),
            State::Collapsing => self.collapsing_line(&mut line),
        }
        let len = line.chars().count();
        if len < self.width {
            line.extend(std::iter::repeat_n(' ', self.width - len));
        }
        (line, was_commit)
    }

    /// Lanes right of an octopus merge step aside to make room for it.
    fn expanding_line(&mut self, line: &mut String) {
        let mut seen = false;
        for (i, &waits_for) in self.lanes.iter().enumerate() {
            if waits_for == self.row {
                seen = true;
                line.push('|');
                line.extend(std::iter::repeat_n(' ', self.expansion));
            } else if seen && self.expansion == 0 {
                let leaning = self.prev_state == State::Merged
                    && self.prev_at < i as isize;
                line.push(if leaning { '\\' } else { '|' });
            } else if seen {
                line.push('\\');
            } else {
                line.push('|');
            }
            line.push(' ');
        }
        self.expansion += 1;
        if !self.needs_expansion() {
            self.set(State::Commit);
        }
    }

    fn commit_line(&mut self, line: &mut String) {
        let mut seen = false;
        for i in 0..=self.lanes.len() {
            let waits_for = if i == self.lanes.len() {
                if seen {
                    break;
                }
                self.row
            } else {
                self.lanes[i]
            };
            let ii = i as isize;
            if waits_for == self.row {
                seen = true;
                line.push(if self.parents.is_empty() { 'o' } else { '*' });
                if self.n() > 2 {
                    let spread = self.spread();
                    for k in 0..spread {
                        line.push('-');
                        line.push(if k == spread - 1 { '.' } else { '-' });
                    }
                }
            } else if seen && self.added > 1 {
                line.push('\\');
            } else if seen && self.added == 1 {
                let leaning = self.prev_state == State::Merged
                    && self.prev_added > 0
                    && self.prev_at < ii;
                line.push(if leaning { '\\' } else { '|' });
            } else if self.prev_state == State::Collapsing
                && slot(&self.prev_slots, 2 * i + 1) == ii
                && (0..ii).contains(&slot(&self.slots, 2 * i))
            {
                line.push('/');
            } else {
                line.push('|');
            }
            line.push(' ');
        }
        if self.n() > 1 {
            self.set(State::Merged);
        } else if self.settled() {
            self.set(State::Idle);
        } else {
            self.set(State::Collapsing);
        }
    }

    /// The line under a merge, where its parents' lanes fan out.
    fn merged_line(&mut self, line: &mut String) {
        let first = self.parents[0];
        let mut first_seen = false;
        let mut seen = false;
        for i in 0..=self.lanes.len() {
            let waits_for = if i == self.lanes.len() {
                if seen {
                    break;
                }
                self.row
            } else {
                self.lanes[i]
            };
            if waits_for == self.row {
                seen = true;
                let mut kind = self.layout as usize;
                for j in 0..self.parents.len() {
                    line.push(MERGE_CHARS[kind]);
                    if kind == 2 {
                        if self.added > 0 || j + 1 < self.parents.len() {
                            line.push(' ');
                        }
                    } else {
                        kind += 1;
                    }
                }
                if self.added == 0 {
                    line.push(' ');
                }
            } else if seen {
                line.push(if self.added > 0 { '\\' } else { '|' });
                line.push(' ');
            } else {
                line.push('|');
                if self.layout != 0 || i as isize != self.at - 1 {
                    line.push(if first_seen { '_' } else { ' ' });
                }
            }
            if waits_for == first {
                first_seen = true;
            }
        }
        if self.settled() {
            self.set(State::Idle);
        } else {
            self.set(State::Collapsing);
        }
    }

    /// Move every lane one step towards its column, `_` carrying the one
    /// lane allowed to travel further on this line.
    fn collapsing_line(&mut self, line: &mut String) {
        mem::swap(&mut self.slots, &mut self.prev_slots);
        let size = self.used;
        self.slots.resize(self.prev_slots.len().max(size), -1);
        for s in &mut self.slots[..size] {
            *s = -1;
        }
        let mut edge: isize = -1;
        let mut edge_target: isize = -1;
        for i in 0..size {
            let target = self.prev_slots[i];
            if target < 0 {
                continue;
            }
            let ii = i as isize;
            if target * 2 == ii {
                self.slots[i] = target;
            } else if self.slots[i - 1] < 0 {
                self.slots[i - 1] = target;
                if edge == -1 {
                    edge = ii;
                    edge_target = target;
                    self.span(target, ii);
                }
            } else if self.slots[i - 1] == target {
                // Joins the lane on its left, which heads the same way.
            } else {
                self.slots[i - 2] = target;
                if edge == -1 {
                    edge = ii - 1;
                    edge_target = target;
                    self.span(target, ii);
                }
            }
        }
        if self.slots[size - 1] < 0 {
            self.used -= 1;
        }
        let mut horizontal = false;
        for i in 0..self.used {
            let target = self.slots[i];
            let ii = i as isize;
            if target < 0 {
                line.push(' ');
            } else if target * 2 == ii {
                line.push('|');
            } else if target == edge_target && ii != edge - 1 {
                if ii != target * 2 + 3 {
                    self.slots[i] = -1;
                }
                horizontal = true;
                line.push('_');
            } else {
                if horizontal && ii < edge {
                    self.slots[i] = -1;
                }
                line.push('/');
            }
        }
        if self.settled() {
            self.set(State::Idle);
        }
    }

    /// Mark the gaps a horizontal edge to `target` crosses from `from`.
    fn span(&mut self, target: isize, from: isize) {
        let mut j = target * 2 + 3;
        while j < from - 2 {
            self.slots[j as usize] = target;
            j += 2;
        }
    }
}

fn slot(slots: &[isize], i: usize) -> isize {
    slots.get(i).copied().unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(parents: &[Vec<usize>]) -> Vec<String> {
        layout(parents)
            .into_iter()
            .map(|l| match l.node {
                Some(n) => format!("{}{n}", l.graph),
                None => l.graph.trim_end().to_string(),
            })
            .collect()
    }

    #[test]
    fn a_chain_is_one_lane() {
        assert_eq!(draw(&[vec![1], vec![2], vec![]]), ["* 0", "* 1", "o 2"]);
    }

    #[test]
    fn a_merge_fans_out_and_closes() {
        let dag = [vec![1, 2], vec![3], vec![3], vec![]];
        assert_eq!(
            draw(&dag),
            ["*   0", "|\\", "* | 1", "| * 2", "|/", "o 3"]
        );
    }

    #[test]
    fn rows_name_their_own_line() {
        let lines = layout(&[vec![1, 2], vec![2], vec![]]);
        let nodes: Vec<_> = lines.iter().filter_map(|l| l.node).collect();
        assert_eq!(nodes, [0, 1, 2]);
    }

    #[test]
    #[should_panic(expected = "parent above it")]
    fn a_parent_above_its_child_is_refused() {
        layout(&[vec![], vec![0]]);
    }
}
