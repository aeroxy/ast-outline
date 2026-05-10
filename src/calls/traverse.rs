//! BFS traversal of a `CallGraph` for `callers` and `callees`.
//!
//! Mirrors `src/deps/traverse.rs` shape so the rendering code stays
//! symmetric. Both directions traverse over `CallEdge`s.

use crate::calls::graph::{CallEdge, CallGraph, Qn};
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone)]
pub struct CallHit {
    pub depth: usize,
    pub edge: CallEdge,
}

/// Forward — what does `start` call (transitively, deduped by target qn).
pub fn callees(graph: &CallGraph, start: &Qn, max_depth: usize) -> Vec<CallHit> {
    let edges_at = |qn: &Qn| graph.forward.get(qn).cloned().unwrap_or_default();
    bfs(start, max_depth, |qn| {
        edges_at(qn)
            .into_iter()
            .filter_map(|e| {
                if let crate::calls::graph::CallTarget::Resolved(t) = &e.target {
                    let t = t.clone();
                    Some((t, e))
                } else {
                    // Keep external/bare edges in the output but don't recurse
                    // into them (no node to traverse).
                    None
                }
            })
            .collect()
    })
}

/// Reverse — who calls `start` (transitively).
pub fn callers(graph: &CallGraph, start: &Qn, max_depth: usize, limit: usize) -> Vec<CallHit> {
    let edges_at = |qn: &Qn| graph.reverse.get(qn).cloned().unwrap_or_default();
    let mut all = bfs(start, max_depth, |qn| {
        edges_at(qn).into_iter().map(|e| (e.source.clone(), e)).collect()
    });
    if all.len() > limit {
        all.truncate(limit);
    }
    all
}

/// All resolved + external + bare edges originating at `start`. Useful for
/// the `callees` text renderer where we want to surface unresolved targets
/// even when traversal can't recurse into them.
pub fn callees_one_hop(graph: &CallGraph, start: &Qn) -> Vec<CallEdge> {
    graph.forward.get(start).cloned().unwrap_or_default()
}

fn bfs<F: Fn(&Qn) -> Vec<(Qn, CallEdge)>>(
    start: &Qn,
    max_depth: usize,
    edges_at: F,
) -> Vec<CallHit> {
    let mut out = Vec::new();
    let mut seen: HashSet<Qn> = HashSet::new();
    let mut q: VecDeque<(Qn, usize)> = VecDeque::new();
    q.push_back((start.clone(), 0));
    seen.insert(start.clone());
    while let Some((cur, depth)) = q.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for (next, edge) in edges_at(&cur) {
            if seen.insert(next.clone()) {
                out.push(CallHit { depth: depth + 1, edge });
                q.push_back((next, depth + 1));
            }
        }
    }
    out
}
