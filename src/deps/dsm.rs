//! Design Structure Matrix — file × file binary matrix sorted by
//! topological level. Cycle members are placed adjacent and form
//! visible above-diagonal blocks (architectural inversions).
//!
//! v1: just the binary matrix. Future ideas: cluster detection
//! and propagation-cost score per cluster.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use crate::deps::graph::DepGraph;

#[derive(Debug, Clone)]
pub struct Dsm {
    /// Files in matrix order (rows + columns). Sorted by Lakos level
    /// (longest path from leaves), then lexicographically.
    pub files: Vec<PathBuf>,
    /// Square matrix: `cells[i][j]` is true iff `files[i]` imports
    /// `files[j]`.
    pub cells: Vec<Vec<bool>>,
    /// Per-file Lakos level (longest dependency-chain length).
    pub levels: Vec<u32>,
}

pub fn build(graph: &DepGraph) -> Dsm {
    let levels_map = compute_levels(graph);
    let mut files: Vec<PathBuf> = graph.forward.keys().cloned().collect();
    files.sort_by(|a, b| {
        let la = levels_map.get(a).copied().unwrap_or(0);
        let lb = levels_map.get(b).copied().unwrap_or(0);
        la.cmp(&lb).then_with(|| a.cmp(b))
    });
    let n = files.len();
    let pos: HashMap<&PathBuf, usize> = files.iter().enumerate().map(|(i, p)| (p, i)).collect();
    let mut cells = vec![vec![false; n]; n];
    for (src, edges) in &graph.forward {
        let Some(&i) = pos.get(src) else { continue };
        for e in edges {
            if let Some(&j) = pos.get(&e.target) {
                cells[i][j] = true;
            }
        }
    }
    let levels: Vec<u32> = files
        .iter()
        .map(|f| levels_map.get(f).copied().unwrap_or(0))
        .collect();
    Dsm {
        files,
        cells,
        levels,
    }
}

/// Lakos level: longest path from a leaf (no outgoing edges) to the
/// node, computed in cycle-safe topological order.
fn compute_levels(graph: &DepGraph) -> HashMap<PathBuf, u32> {
    // For each file, count the *outgoing* edges (forward count).
    // Repeatedly peel off zero-out-degree nodes; they get level 0.
    // Subsequent layers get level = max(neighbour level) + 1.
    // Cycle members all stay at the same residual level (their highest
    // assigned predecessor) — close enough for visualisation purposes.

    let mut out_count: HashMap<PathBuf, usize> = HashMap::new();
    let mut rev_adj: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    let mut all: HashSet<PathBuf> = HashSet::new();

    for (src, edges) in &graph.forward {
        all.insert(src.clone());
        out_count.insert(src.clone(), edges.len());
        for e in edges {
            all.insert(e.target.clone());
            rev_adj.entry(e.target.clone()).or_default().push(src.clone());
        }
    }
    for f in &all {
        out_count.entry(f.clone()).or_insert(0);
    }

    let mut levels: HashMap<PathBuf, u32> = HashMap::new();
    let mut q: VecDeque<PathBuf> = VecDeque::new();
    for (f, &c) in &out_count {
        if c == 0 {
            levels.insert(f.clone(), 0);
            q.push_back(f.clone());
        }
    }
    while let Some(node) = q.pop_front() {
        let cur_level = *levels.get(&node).unwrap_or(&0);
        if let Some(parents) = rev_adj.get(&node) {
            for p in parents {
                let entry = levels.entry(p.clone()).or_insert(0);
                if *entry < cur_level + 1 {
                    *entry = cur_level + 1;
                }
                let cnt = out_count.get_mut(p).unwrap();
                *cnt = cnt.saturating_sub(1);
                if *cnt == 0 {
                    q.push_back(p.clone());
                }
            }
        }
    }
    // Cycle nodes never get out_count to zero — assign them the max
    // level assigned so far + 1 to keep them visible at the top.
    let max_assigned = levels.values().copied().max().unwrap_or(0);
    for f in &all {
        levels.entry(f.clone()).or_insert(max_assigned + 1);
    }
    levels
}
