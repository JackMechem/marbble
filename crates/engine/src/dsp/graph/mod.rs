//! Which strips must finish before which, for running them at the same time (see `pool/` and
//! `parallel/`).
//!
//! The session works a graph out from the project, off the audio thread, and sends it in a
//! `Cmd::Graph`. Tasks are the engine's tracks in processing order, so every edge points forward
//! in the list: a graph can't hold a cycle, and `TaskGraph::new` refuses an edge that would make
//! one. A strip's dependencies are kept in that same order, which is the order its inputs are
//! summed in, so the sums come out bit for bit as the serial loop's do.
//!
//! The graph is only a claim about what the engine does. `Engine` checks it against the routes it
//! resolved itself (`parallel::covers`) before trusting it, and plays serially whenever it
//! doesn't hold, as in the blocks between an edit's commands and its new graph.

use crate::model::{Id, Project};

/// The most tasks a graph can have; a larger project just plays serially.
pub const MAX_TASKS: usize = 1024;

/// Why a graph can't be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphError {
    TooBig(usize),
    /// An edge names a track that isn't in the list, or the list names one twice.
    Unknown(Id),
    /// An edge points backwards in the list (or at its own track).
    Backwards {
        from: Id,
        to: Id,
    },
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::TooBig(n) => write!(f, "{n} tracks are more than the {MAX_TASKS} a graph can hold"),
            GraphError::Unknown(id) => write!(f, "track {id} isn't in the processing order, or is in it twice"),
            GraphError::Backwards { from, to } => write!(f, "track {from} plays into {to}, which plays before it"),
        }
    }
}

/// Which strips wait for which, in the order the serial loop plays them. Built off the audio thread
/// and handed over whole; running it allocates nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskGraph {
    ids: Vec<Id>,
    /// Each task's dependencies, ascending: `deps[dep_at[i]..dep_at[i + 1]]`.
    dep_at: Vec<u32>,
    deps: Vec<u16>,
    /// And what waits for it, likewise.
    succ_at: Vec<u32>,
    succs: Vec<u16>,
    roots: Vec<u16>,
    /// The most tasks with plug-ins that can run at once, by depth in the graph.
    heavy_width: usize,
}

impl TaskGraph {
    /// A graph over `ids` (the processing order), where each `(from, to)` makes `to` wait for
    /// `from`. `heavy` says which tracks run plug-ins, the only work worth a second thread.
    pub fn new(ids: Vec<Id>, edges: &[(Id, Id)], heavy: impl Fn(Id) -> bool) -> Result<TaskGraph, GraphError> {
        let n = ids.len();
        if n > MAX_TASKS {
            return Err(GraphError::TooBig(n));
        }
        let at: std::collections::HashMap<Id, usize> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        if at.len() != n {
            return Err(GraphError::Unknown(*ids.iter().find(|id| ids.iter().filter(|x| x == id).count() > 1).unwrap_or(&0)));
        }
        let mut dep: Vec<Vec<u16>> = vec![Vec::new(); n];
        let mut succ: Vec<Vec<u16>> = vec![Vec::new(); n];
        for &(from, to) in edges {
            let a = *at.get(&from).ok_or(GraphError::Unknown(from))?;
            let b = *at.get(&to).ok_or(GraphError::Unknown(to))?;
            if a >= b {
                return Err(GraphError::Backwards { from, to });
            }
            dep[b].push(a as u16);
            succ[a].push(b as u16);
        }
        for l in dep.iter_mut().chain(succ.iter_mut()) {
            l.sort_unstable();
            l.dedup();
        }
        let flatten = |lists: &[Vec<u16>]| {
            let mut starts = Vec::with_capacity(n + 1);
            let mut all = Vec::with_capacity(lists.iter().map(Vec::len).sum());
            for l in lists {
                starts.push(all.len() as u32);
                all.extend_from_slice(l);
            }
            starts.push(all.len() as u32);
            (starts, all)
        };
        // Depth in the graph: a task's level is one past its deepest dependency, so tasks of one
        // level can overlap whatever their cost.
        let mut level = vec![0usize; n];
        for i in 0..n {
            level[i] = dep[i].iter().map(|&d| level[d as usize] + 1).max().unwrap_or(0);
        }
        let mut per_level = vec![0usize; n + 1];
        for i in 0..n {
            if heavy(ids[i]) {
                per_level[level[i]] += 1;
            }
        }
        let (dep_at, deps) = flatten(&dep);
        let (succ_at, succs) = flatten(&succ);
        let roots = (0..n).filter(|&i| dep[i].is_empty()).map(|i| i as u16).collect();
        Ok(TaskGraph { ids, dep_at, deps, succ_at, succs, roots, heavy_width: per_level.into_iter().max().unwrap_or(0) })
    }

    /// The graph of a project: what `Project::process_order` puts each track after, except a
    /// folder's subtracks, which only read its automation (the engine does that before the strips
    /// start).
    pub fn of_project(p: &Project) -> Result<TaskGraph, GraphError> {
        let ids = p.process_order();
        let mut edges = Vec::new();
        for &id in &ids {
            for to in p.feeds(id).into_iter().chain(p.keyed_by(id)) {
                edges.push((id, to));
            }
        }
        let heavy =
            |id: Id| p.track(id).is_some_and(|t| t.frozen.is_none() && !p.is_off(id) && (t.instrument.is_some() || t.inserts.iter().any(Option::is_some)));
        TaskGraph::new(ids, &edges, heavy)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn id(&self, task: usize) -> Id {
        self.ids[task]
    }

    /// What `task` waits for, in processing order.
    pub fn deps(&self, task: usize) -> &[u16] {
        &self.deps[self.dep_at[task] as usize..self.dep_at[task + 1] as usize]
    }

    /// What waits for `task`.
    pub fn succs(&self, task: usize) -> &[u16] {
        &self.succs[self.succ_at[task] as usize..self.succ_at[task + 1] as usize]
    }

    /// The tasks that wait for nothing.
    pub fn roots(&self) -> &[u16] {
        &self.roots
    }

    /// Whether `task` waits for `dep` directly.
    pub fn has_dep(&self, task: usize, dep: usize) -> bool {
        self.deps(task).binary_search(&(dep as u16)).is_ok()
    }

    /// How many strips with plug-ins can run at once. Under 2, a second thread has nothing to
    /// gain, whatever the project's load.
    pub fn heavy_width(&self) -> usize {
        self.heavy_width
    }
}

#[cfg(test)]
mod tests;
