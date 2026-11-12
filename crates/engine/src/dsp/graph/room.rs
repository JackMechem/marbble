//! A graph the engine builds itself, on the audio thread, when the session's no longer holds for
//! its strips (`Parallel::wanted`): in the blocks between an edit's commands and the graph the
//! session works out for it, which would otherwise play every strip on the callback alone (#475).
//! Its room is allocated with the workers, off the audio thread, for [`MAX_TASKS`] strips with every
//! route each can have, and refilling it allocates nothing.

use super::*;
use crate::model::{INSERT_SLOTS, SEND_SLOTS};

/// The most edges one strip can make: its output, its sends, the instrument output it is fed
/// from, and a side-chain key per insert.
const EDGES_PER_TASK: usize = 2 + SEND_SLOTS + INSERT_SLOTS;

/// A [`TaskGraph`] with the room to be rebuilt in place, and what building it needs.
pub struct GraphRoom {
    pub(super) graph: TaskGraph,
    /// Where each task's next dependency or successor goes, while they're laid out.
    cursor: Vec<u32>,
    level: Vec<u32>,
    per_level: Vec<u32>,
}

impl Default for GraphRoom {
    fn default() -> Self {
        let edges = MAX_TASKS * EDGES_PER_TASK;
        GraphRoom {
            graph: TaskGraph {
                ids: Vec::with_capacity(MAX_TASKS),
                dep_at: Vec::with_capacity(MAX_TASKS + 1),
                deps: Vec::with_capacity(edges),
                succ_at: Vec::with_capacity(MAX_TASKS + 1),
                succs: Vec::with_capacity(edges),
                roots: Vec::with_capacity(MAX_TASKS),
                heavy_width: 0,
            },
            cursor: Vec::with_capacity(MAX_TASKS + 1),
            level: Vec::with_capacity(MAX_TASKS),
            per_level: Vec::with_capacity(MAX_TASKS + 1),
        }
    }
}

impl GraphRoom {
    pub fn graph(&self) -> &TaskGraph {
        &self.graph
    }

    /// Rebuilds the graph over `n` tasks, as `TaskGraph::new` would: `id` names each, `edges(i, add)`
    /// calls `add(from, to)` for each edge strip `i` makes (at most `EDGES_PER_TASK`), and `heavy`
    /// says which run plug-ins. False, leaving no graph to trust, if there are too many tasks or an
    /// edge doesn't point forwards. Allocates nothing.
    pub fn refill(&mut self, n: usize, id: impl Fn(usize) -> Id, edges: impl Fn(usize, &mut dyn FnMut(usize, usize)), heavy: impl Fn(usize) -> bool) -> bool {
        let g = &mut self.graph;
        g.ids.clear();
        if n > MAX_TASKS {
            return false;
        }
        // How many dependencies each task has, then where they start.
        let count = &mut self.cursor;
        count.clear();
        count.resize(n, 0);
        let (mut total, mut forwards) = (0usize, true);
        for i in 0..n {
            edges(i, &mut |a, b| {
                if a < b && b < n {
                    count[b] += 1;
                    total += 1;
                } else {
                    forwards = false;
                }
            });
        }
        if !forwards || total > g.deps.capacity() {
            return false;
        }
        g.dep_at.clear();
        g.dep_at.push(0);
        for c in count.iter_mut() {
            let start = *g.dep_at.last().expect("starts at 0");
            g.dep_at.push(start + *c);
            *c = start;
        }
        g.deps.clear();
        g.deps.resize(total, 0);
        for i in 0..n {
            edges(i, &mut |a, b| {
                g.deps[count[b] as usize] = a as u16;
                count[b] += 1;
            });
        }
        // Each task's dependencies ascending and once each, closed up.
        let mut w = 0;
        for i in 0..n {
            let (s, e) = (g.dep_at[i] as usize, g.dep_at[i + 1] as usize);
            g.deps[s..e].sort_unstable();
            g.dep_at[i] = w as u32;
            for k in s..e {
                if k == s || g.deps[k] != g.deps[k - 1] {
                    g.deps[w] = g.deps[k];
                    w += 1;
                }
            }
        }
        g.dep_at[n] = w as u32;
        g.deps.truncate(w);
        // What waits for each task: laid out by its dependents in order, so ascending too.
        let count = &mut self.cursor;
        count.clear();
        count.resize(n, 0);
        for &d in &g.deps {
            count[d as usize] += 1;
        }
        g.succ_at.clear();
        g.succ_at.push(0);
        for c in count.iter_mut() {
            let start = *g.succ_at.last().expect("starts at 0");
            g.succ_at.push(start + *c);
            *c = start;
        }
        g.succs.clear();
        g.succs.resize(w, 0);
        for b in 0..n {
            for k in g.dep_at[b] as usize..g.dep_at[b + 1] as usize {
                let a = g.deps[k] as usize;
                g.succs[count[a] as usize] = b as u16;
                count[a] += 1;
            }
        }
        // Width at each depth, as in `TaskGraph::new`.
        self.level.clear();
        self.per_level.clear();
        self.per_level.resize(n + 1, 0);
        for i in 0..n {
            let deps = &g.deps[g.dep_at[i] as usize..g.dep_at[i + 1] as usize];
            let level = deps.iter().map(|&d| self.level[d as usize] + 1).max().unwrap_or(0);
            self.level.push(level);
            if heavy(i) {
                self.per_level[level as usize] += 1;
            }
        }
        g.heavy_width = self.per_level.iter().copied().max().unwrap_or(0) as usize;
        g.roots.clear();
        g.roots.extend((0..n).filter(|&i| g.dep_at[i] == g.dep_at[i + 1]).map(|i| i as u16));
        g.ids.extend((0..n).map(id));
        true
    }
}
