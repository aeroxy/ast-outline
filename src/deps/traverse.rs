//! BFS traversal of a `DepGraph` for `deps` and `reverse-deps`.
//!
//! Both directions share one BFS routine parameterised by an
//! adjacency lookup closure.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use crate::deps::graph::{DepEdge, DepGraph, ImportKind};

#[derive(Debug, Clone)]
pub struct DepHit {
    pub depth: usize,
    pub file: PathBuf,
    pub kind: ImportKind,
    pub line: u32,
    pub local_name: Option<String>,
}

/// Forward BFS — what does `start` import (transitively).
pub fn forward(graph: &DepGraph, start: &PathBuf, max_depth: usize) -> Vec<DepHit> {
    let edges_at = |p: &PathBuf| graph.forward.get(p).cloned().unwrap_or_default();
    bfs(start, max_depth, edges_at)
}

/// Reverse BFS — who imports `start` (transitively).
pub fn reverse(graph: &DepGraph, start: &PathBuf, max_depth: usize, limit: usize) -> Vec<DepHit> {
    let rev = graph.reverse_adjacency();
    let edges_at = |p: &PathBuf| {
        rev.get(p)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|src| DepEdge {
                target: src,
                kind: ImportKind::Bare,
                line: 0,
                local_name: None,
                raw_path: None,
            })
            .collect::<Vec<_>>()
    };
    let mut all = bfs(start, max_depth, edges_at);
    if all.len() > limit {
        all.truncate(limit);
    }
    all
}

fn bfs<F: Fn(&PathBuf) -> Vec<DepEdge>>(
    start: &PathBuf,
    max_depth: usize,
    edges_at: F,
) -> Vec<DepHit> {
    let mut out = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut q: VecDeque<(PathBuf, usize)> = VecDeque::new();
    q.push_back((start.clone(), 0));
    seen.insert(start.clone());
    while let Some((cur, depth)) = q.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for e in edges_at(&cur) {
            if seen.insert(e.target.clone()) {
                out.push(DepHit {
                    depth: depth + 1,
                    file: e.target.clone(),
                    kind: e.kind,
                    line: e.line,
                    local_name: e.local_name,
                });
                q.push_back((e.target, depth + 1));
            }
        }
    }
    out
}

/// Per-file depth from `source` in the *combined* (forward + reverse)
/// graph — used by `find-related` for dep-aware boosting.
pub fn neighbourhood_depths(
    graph: &DepGraph,
    source: &PathBuf,
    max_depth: usize,
) -> HashMap<PathBuf, usize> {
    let mut depths: HashMap<PathBuf, usize> = HashMap::new();
    depths.insert(source.clone(), 0);
    let rev = graph.reverse_adjacency();
    let mut q: VecDeque<(PathBuf, usize)> = VecDeque::new();
    q.push_back((source.clone(), 0));
    while let Some((cur, d)) = q.pop_front() {
        if d >= max_depth {
            continue;
        }
        let mut neighbours: Vec<PathBuf> = graph
            .forward
            .get(&cur)
            .map(|edges| edges.iter().map(|e| e.target.clone()).collect())
            .unwrap_or_default();
        if let Some(reversers) = rev.get(&cur) {
            neighbours.extend(reversers.clone());
        }
        for n in neighbours {
            let new_d = d + 1;
            let prev = depths.get(&n).copied();
            if prev.is_none_or(|p| p > new_d) {
                depths.insert(n.clone(), new_d);
                q.push_back((n, new_d));
            }
        }
    }
    depths
}
