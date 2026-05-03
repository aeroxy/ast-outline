//! `ast-outline deps|reverse-deps|cycles|graph` — file-level
//! dependency-graph analysis.
//!
//! Two-stage pipeline:
//! 1. `extract`: per-language tree-sitter pass produces normalised
//!    `RawImport` records.
//! 2. `resolver`: a single suffix-index resolver maps each `RawImport`
//!    to a concrete file path inside the project.
//!
//! The resulting `DepGraph` powers `deps`, `reverse-deps`, `cycles`,
//! `graph`, and the dep-aware ranking boost in `find-related`.

pub mod cache;
pub mod cli;
pub mod dsm;
pub mod extract;
pub mod graph;
pub mod manifest;
pub mod options;
pub mod render;
pub mod resolver;
pub mod scc;
pub mod traverse;

pub use graph::{DepEdge, DepGraph};
pub use options::DepError;

use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::deps::extract::extract;
use crate::deps::manifest::detect_aliases;
use crate::deps::resolver::{build_suffix_index, resolve, ResolveCtx};

/// Build a `DepGraph` from scratch by walking `root`, parsing every
/// supported file, and resolving each import.
///
/// The build is parallel across files via rayon. Suffix-index build
/// happens first (single pass), then per-file extraction + resolution
/// runs in parallel.
pub fn build_graph(root: &Path) -> Result<DepGraph, DepError> {
    let start = Instant::now();
    let root_canon = root
        .canonicalize()
        .map_err(|e| DepError::Io {
            path: root.to_path_buf(),
            source: e,
        })?;

    let aliases = detect_aliases(&root_canon);
    let idx = build_suffix_index(&root_canon);

    // Collect a list of files to process from the index (already filtered).
    let files: Vec<_> = idx.by_file.keys().cloned().collect();

    // Per-file extract + resolve in parallel.
    let resolved: Vec<(PathBuf, Vec<DepEdge>, Vec<String>)> = files
        .par_iter()
        .map(|file| {
            let info = match idx.by_file.get(file) {
                Some(i) => i,
                None => return (file.clone(), Vec::new(), Vec::new()),
            };
            let raw_imports = extract(file, info.language);

            let mut edges = Vec::new();
            let mut external = Vec::new();
            let ctx = ResolveCtx {
                from_file: file,
                lang: info.language,
                alias_prefix: aliases.go_module.as_deref(),
                path_aliases: &aliases.ts_path_aliases,
            };
            for ri in raw_imports {
                match resolve(&ri.spec, &ctx, &idx) {
                    Some(target) => {
                        if target == *file {
                            // self-edge from path collisions — skip.
                            continue;
                        }
                        edges.push(DepEdge {
                            target,
                            kind: ri.kind,
                            line: ri.line,
                            local_name: ri.local_name,
                            raw_path: ri.raw_path,
                        });
                    }
                    None => external.push(ri.raw_path.unwrap_or(ri.spec)),
                }
            }
            (file.clone(), edges, external)
        })
        .collect();

    let mut g = DepGraph::empty(root_canon.clone());
    for (file, edges, external) in resolved {
        if !edges.is_empty() {
            g.forward.insert(file.clone(), edges);
        } else {
            // Insert empty list so the file is known to the graph.
            g.forward.insert(file.clone(), Vec::new());
        }
        if !external.is_empty() {
            g.external.insert(file, external);
        }
    }
    graph::dedup_edges(&mut g);

    let elapsed = start.elapsed();
    let edge_count: usize = g.forward.values().map(|v| v.len()).sum();
    let external_count: usize = g.external.values().map(|v| v.len()).sum();
    g.stats = graph::GraphStats {
        file_count: g.forward.len(),
        edge_count,
        external_count,
        build_ms: elapsed.as_millis() as u64,
    };
    Ok(g)
}

/// Load from cache when fresh; otherwise build + persist.
pub fn load_or_build(root: &Path, force_rebuild: bool) -> Result<DepGraph, DepError> {
    if !force_rebuild {
        if let Some(g) = cache::load_if_fresh(root) {
            return Ok(g);
        }
    }
    let g = build_graph(root)?;
    // Persist (best-effort — failure is non-fatal).
    if let Ok(records) = collect_file_records(root) {
        let _ = cache::save(root, &g, &records);
    }
    Ok(g)
}

/// Build a `Vec<FileRecord>` describing the current state of every
/// indexable file in `root`. Reuses search's `compute_delta` against an
/// empty cache to populate hashes.
fn collect_file_records(root: &Path) -> std::io::Result<Vec<crate::search::cache::FileRecord>> {
    use crate::search::cache::{compute_delta, FileRecord, hash_file};

    let delta = compute_delta(root, &[]);
    let mut out = Vec::with_capacity(delta.added.len());
    for path in delta.added {
        let meta = std::fs::metadata(&path)?;
        let mtime = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let mtime_ns = match mtime.duration_since(std::time::SystemTime::UNIX_EPOCH) {
            Ok(d) => d.as_nanos() as i128,
            Err(e) => -(e.duration().as_nanos() as i128),
        };
        let rel = path
            .strip_prefix(root)
            .map(|r| {
                r.components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_else(|_| path.display().to_string());
        let hash = hash_file(&path).unwrap_or(0);
        out.push(FileRecord {
            path: rel,
            mtime_ns,
            size: meta.len(),
            content_hash: hash,
            chunk_start: 0,
            chunk_end: 0,
        });
    }
    Ok(out)
}
