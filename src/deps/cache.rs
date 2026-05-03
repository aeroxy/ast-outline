//! Disk cache for `DepGraph` at `.ast-outline/deps/graph.bin`.
//!
//! Mirrors the search-index pattern in `src/search/cache.rs` —
//! mtime-based delta detection, advisory `fs2` lock, atomic write.
//! Any non-empty delta triggers a full rebuild (same simplification
//! the search index uses today).

use crate::core::JSON_SCHEMA_DEPS_INDEX;
use crate::search::cache::{compute_delta, FileRecord, Delta};
use crate::deps::graph::DepGraph;
use bincode::serde::{decode_from_slice, encode_to_vec};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const CACHE_SCHEMA: &str = JSON_SCHEMA_DEPS_INDEX;

/// On-disk wrapper combining the graph + the file fingerprints used for
/// freshness detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheFile {
    pub schema: String,
    pub graph: DepGraph,
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
pub fn load_if_fresh(root: &Path) -> Option<DepGraph> {
    let path = cache_path(root);
    let bytes = fs::read(&path).ok()?;
    let (cf, _): (CacheFile, _) = decode_from_slice(&bytes, bincode::config::standard()).ok()?;
    if cf.schema != CACHE_SCHEMA {
        return None;
    }
    let delta = compute_delta(root, &cf.files);
    if delta.requires_rebuild() {
        return None;
    }
    Some(cf.graph)
}

/// Persist a graph + file fingerprints atomically. Writes via `.tmp` +
/// rename, holds an advisory exclusive lock during the write.
pub fn save(root: &Path, graph: &DepGraph, files: &[FileRecord]) -> std::io::Result<()> {
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

/// Convenience: take a delta computed elsewhere and decide whether to use
/// the cache or rebuild. Used by `find-related`.
#[allow(dead_code)]
pub fn delta_requires_rebuild(d: &Delta) -> bool {
    d.requires_rebuild()
}
