//! Disk persistence for the unified `UnifiedGraph` at
//! `.ast-outline/deps/graph.bin`. Replaces the previous `src/deps/cache.rs`
//! which serialized only `DepGraph`. Schema bump from `deps-index.v1` to
//! `graph-index.v1` forces a one-time rebuild for upgrading users.
//!
//! Mirrors the search-index pattern in `src/search/cache.rs`: mtime/size/
//! xxhash3 delta detection, advisory `fs2` lock, atomic `.tmp` + rename.

use crate::core::JSON_SCHEMA_GRAPH_INDEX;
use crate::deps::build_graph;
use crate::graph_cache::UnifiedGraph;
use crate::search::cache::{compute_delta, hash_file, FileRecord};
use bincode::serde::{decode_from_slice, encode_to_vec};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const CACHE_SCHEMA: &str = JSON_SCHEMA_GRAPH_INDEX;

/// On-disk wrapper combining the unified graph + the file fingerprints used
/// for freshness detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheFile {
    pub schema: String,
    pub graph: UnifiedGraph,
    /// Mtime/size/hash records for delta detection. Reuses the search
    /// index's `FileRecord` to avoid a parallel implementation.
    pub files: Vec<FileRecord>,
}

pub fn cache_dir(root: &Path) -> PathBuf {
    root.join(".ast-outline").join("deps")
}

pub fn cache_path(root: &Path) -> PathBuf {
    cache_dir(root).join("graph.bin")
}

pub fn lock_path(root: &Path) -> PathBuf {
    cache_dir(root).join("lock")
}

/// Try to read a fresh cache for `root`. Returns `None` if the cache
/// doesn't exist, has the wrong schema, or is stale per `compute_delta`.
pub fn load_if_fresh(root: &Path) -> Option<UnifiedGraph> {
    let path = cache_path(root);
    let bytes = fs::read(&path).ok()?;
    let (cf, _): (CacheFile, _) = decode_from_slice(&bytes, bincode::config::standard()).ok()?;
    if cf.schema != CACHE_SCHEMA {
        return None;
    }
    let delta = compute_delta(root, root, &cf.files);
    if delta.requires_rebuild() {
        return None;
    }
    Some(cf.graph)
}

/// Build a fresh `UnifiedGraph` (deps half only — call graph is materialised
/// later via `promote_calls`) and persist it.
pub fn build_and_save(root: &Path) -> std::io::Result<UnifiedGraph> {
    let deps = build_graph(root).map_err(std::io::Error::other)?;
    let graph = UnifiedGraph::from_deps(deps);
    let records = collect_file_records(root)?;
    let _ = save(root, &graph, &records);
    Ok(graph)
}

/// Load from cache when fresh; otherwise build + persist.
pub fn load_or_build(root: &Path, force_rebuild: bool) -> std::io::Result<UnifiedGraph> {
    if !force_rebuild {
        if let Some(g) = load_if_fresh(root) {
            return Ok(g);
        }
    }
    build_and_save(root)
}

/// Persist a graph + file fingerprints atomically. Writes via `.tmp` +
/// rename, holds an advisory exclusive lock during the write.
pub fn save(
    root: &Path,
    graph: &UnifiedGraph,
    files: &[FileRecord],
) -> std::io::Result<()> {
    let dir = cache_dir(root);
    fs::create_dir_all(&dir)?;
    write_gitignore(&dir)?;

    let lock = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_path(root))?;
    lock.lock_exclusive()?;

    let cf = CacheFile {
        schema: CACHE_SCHEMA.to_string(),
        graph: graph.clone(),
        files: files.to_vec(),
    };
    let bytes = encode_to_vec(&cf, bincode::config::standard())
        .map_err(std::io::Error::other)?;

    let final_path = cache_path(root);
    let tmp = final_path.with_extension("bin.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &final_path)?;

    fs2::FileExt::unlock(&lock).ok();
    Ok(())
}

fn write_gitignore(dir: &Path) -> std::io::Result<()> {
    let p = dir.parent().map(|d| d.join(".gitignore"));
    if let Some(p) = p {
        if !p.exists() {
            fs::write(&p, "*\n")?;
        }
    }
    Ok(())
}

/// Build a `Vec<FileRecord>` describing the current state of every
/// indexable file in `root`. Reuses search's `compute_delta` against an
/// empty cache to populate hashes.
pub fn collect_file_records(root: &Path) -> std::io::Result<Vec<FileRecord>> {
    let delta = compute_delta(root, root, &[]);
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
