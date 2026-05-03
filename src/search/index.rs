//! Per-repo persistent search index.
//!
//! `Index::open(repo_root)` either loads the cached index from
//! `.ast-outline/index/` (and refreshes it if files have changed) or builds
//! one from scratch on first use. After that, `index.search(...)` and
//! `index.find_related(...)` run the full pipeline:
//!
//! ```text
//! search:        tokenize → BM25 + dense top-k → RRF → ranking → top-k
//! find-related:  resolve chunk → semantic top-k (lang-filtered) → exclude self → top-k
//! ```
//!
//! Phase-7 simplification: any non-empty delta (added / modified / removed)
//! triggers a full rebuild. The on-disk format reserves the fields needed
//! for a v2 partial-rebuild path (per-file `chunk_range` + a tombstones
//! vector in `meta.json`) so swapping in incremental updates later doesn't
//! invalidate caches.

use crate::file_filter::{add_filters, should_skip_path};
use crate::search::bm25::Bm25Index;
use crate::search::cache::{compute_delta, hash_file, FileRecord};
use crate::search::chunker::{chunk_file, is_indexable, Chunk};
use crate::search::download::{ensure_model, ModelInfo};
use crate::search::embed::{cosine_topk, Embedder, DIM};
use crate::search::fusion::{combine, resolve_alpha, rrf_scores};
use crate::search::ranking::{apply_query_boost, boost_multi_chunk_files, rerank_topk};
use crate::search::tokens::tokenize;

use fs2::FileExt;
use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

const SCHEMA: &str = "ast-outline.search-index.v1";

/// On-disk paths under a repo's `.ast-outline/index/` directory.
#[derive(Debug, Clone)]
pub struct IndexPaths {
    pub root: PathBuf,
    pub index_dir: PathBuf,
    pub meta_json: PathBuf,
    pub chunks_bin: PathBuf,
    pub embeddings_f32: PathBuf,
    pub bm25_bin: PathBuf,
    pub files_bin: PathBuf,
    pub lock: PathBuf,
    pub gitignore: PathBuf,
}

impl IndexPaths {
    pub fn from_repo(repo_root: &Path) -> Self {
        let index_dir = repo_root.join(".ast-outline").join("index");
        Self {
            root: repo_root.to_path_buf(),
            meta_json: index_dir.join("meta.json"),
            chunks_bin: index_dir.join("chunks.bin"),
            embeddings_f32: index_dir.join("embeddings.f32"),
            bm25_bin: index_dir.join("bm25.bin"),
            files_bin: index_dir.join("files.bin"),
            lock: index_dir.join("lock"),
            gitignore: repo_root.join(".ast-outline").join(".gitignore"),
            index_dir,
        }
    }
}

/// Top-level metadata persisted as JSON for human readability + version checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub schema: String,
    pub ast_outline_version: String,
    pub model: ModelMeta,
    pub created_unix: u64,
    pub chunk_count: u32,
    /// Always `"f32_le"` for v1. Reserved so a v2 can switch to f16/quantized.
    pub embedding_dtype: String,
    /// Reserved for incremental updates — empty in v1.
    #[serde(default)]
    pub tombstones: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMeta {
    pub id: String,
    pub dim: u32,
}

/// One search hit — a chunk with its final score.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub chunk: Chunk,
    pub score: f32,
}

/// Options for `search`. `find-related` doesn't need any (just `top_k`).
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub top_k: usize,
    /// Override the auto-resolved alpha. `None` = auto-detect from query type.
    pub alpha: Option<f32>,
    /// If set, restrict to chunks whose `language` field is in this set.
    pub languages: Option<Vec<String>>,
}

impl SearchOptions {
    #[allow(dead_code)] // used by network-gated tests; CLI/MCP build via struct literal
    pub fn with_top_k(top_k: usize) -> Self {
        Self {
            top_k,
            ..Default::default()
        }
    }
}

pub struct Index {
    pub paths: IndexPaths,
    pub meta: Meta,
    chunks: Vec<Chunk>,
    /// `chunk_count × DIM` row-major. Held in memory for v1; mmap is a v2 swap.
    embeddings: Vec<f32>,
    bm25: Bm25Index,
    files: Vec<FileRecord>,
    embedder: Arc<Embedder>,
    /// Memoised dep graph for `find-related` boost. `None` until the
    /// first call; then either Some(graph) when `.ast-outline/deps/`
    /// has a fresh cache, or stays None to mean "no boost available".
    /// Mutated via `RwLock` so the borrow remains shared.
    dep_graph: std::sync::RwLock<Option<Option<crate::deps::DepGraph>>>,
}

impl Index {
    /// Open the index at `repo_root`, building if missing or refreshing on
    /// detected file changes.
    pub fn open(repo_root: &Path) -> io::Result<Self> {
        let paths = IndexPaths::from_repo(repo_root);

        // Try to load. If anything fails (missing files, schema mismatch,
        // corruption) fall back to a fresh build.
        if paths.meta_json.exists() {
            match Self::load_unlocked(&paths) {
                Ok(loaded) => {
                    let delta = compute_delta(&paths.root, &loaded.files);
                    if !delta.requires_rebuild() {
                        return Ok(loaded);
                    }
                    eprintln!(
                        "ast-outline: index stale ({} added, {} modified, {} removed) — rebuilding",
                        delta.added.len(),
                        delta.modified.len(),
                        delta.removed.len(),
                    );
                }
                Err(e) => {
                    eprintln!("ast-outline: index unreadable ({e}); rebuilding");
                }
            }
        }

        Self::build(repo_root)
    }

    /// Force a full rebuild from scratch. Leaves any existing cache replaced.
    pub fn build(repo_root: &Path) -> io::Result<Self> {
        let paths = IndexPaths::from_repo(repo_root);
        fs::create_dir_all(&paths.index_dir)?;
        // Always ensure the .gitignore is present so users don't accidentally
        // commit the cache.
        ensure_gitignore(&paths)?;

        let lock_file = acquire_lock(&paths)?;

        let started = std::time::Instant::now();
        eprintln!("ast-outline: building index for {}", paths.root.display());

        // 1. Walk + chunk every indexable file.
        let (file_paths, chunks_per_file): (Vec<PathBuf>, Vec<Vec<Chunk>>) =
            walk_and_chunk(&paths.root);

        // 2. Build flat chunks vec + per-file chunk_range.
        let mut chunks = Vec::new();
        let mut files: Vec<FileRecord> = Vec::with_capacity(file_paths.len());
        for (path, file_chunks) in file_paths.iter().zip(chunks_per_file.into_iter()) {
            let rel = match path.strip_prefix(&paths.root) {
                Ok(r) => normalise_path(r),
                Err(_) => continue,
            };
            let meta_io = match fs::metadata(path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime_ns = mtime_nanos(&meta_io);
            let size = meta_io.len();
            let content_hash = hash_file(path).unwrap_or(0);
            let chunk_start = chunks.len() as u32;
            chunks.extend(file_chunks);
            let chunk_end = chunks.len() as u32;
            files.push(FileRecord {
                path: rel,
                mtime_ns,
                size,
                content_hash,
                chunk_start,
                chunk_end,
            });
        }
        let chunk_count = chunks.len() as u32;
        eprintln!(
            "ast-outline: chunked {} files → {} chunks in {:.1}s",
            file_paths.len(),
            chunk_count,
            started.elapsed().as_secs_f64()
        );

        // 3. Load model + embed all chunks (parallel via rayon).
        let model_dir = ensure_model(&ModelInfo::potion_code_16m())?;
        let embedder = Arc::new(Embedder::open(&model_dir)?);
        let started_embed = std::time::Instant::now();
        let embeddings: Vec<f32> = chunks
            .par_iter()
            .flat_map(|c| {
                let v = embedder.encode_one(&c.content);
                v.to_vec()
            })
            .collect();
        eprintln!(
            "ast-outline: embedded in {:.1}s",
            started_embed.elapsed().as_secs_f64()
        );

        // 4. Build BM25.
        let started_bm25 = std::time::Instant::now();
        let bm25_docs: Vec<Vec<String>> = chunks
            .par_iter()
            .map(|c| tokenize(&enrich_for_bm25(c)))
            .collect();
        let bm25 = Bm25Index::build(bm25_docs);
        eprintln!(
            "ast-outline: bm25 built in {:.1}s",
            started_bm25.elapsed().as_secs_f64()
        );

        // 5. Persist everything atomically — write to temp paths then rename.
        let meta = Meta {
            schema: SCHEMA.to_string(),
            ast_outline_version: env!("CARGO_PKG_VERSION").to_string(),
            model: ModelMeta {
                id: ModelInfo::potion_code_16m().id,
                dim: DIM as u32,
            },
            created_unix: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            chunk_count,
            embedding_dtype: "f32_le".to_string(),
            tombstones: Vec::new(),
        };
        write_meta(&paths.meta_json, &meta)?;
        write_bincode(&paths.chunks_bin, &chunks)?;
        write_bincode(&paths.files_bin, &files)?;
        write_bincode(&paths.bm25_bin, &bm25)?;
        write_embeddings(&paths.embeddings_f32, &embeddings)?;

        eprintln!(
            "ast-outline: index built in {:.1}s total",
            started.elapsed().as_secs_f64()
        );

        // Lock auto-released on drop.
        drop(lock_file);

        Ok(Self {
            paths,
            meta,
            chunks,
            embeddings,
            bm25,
            files,
            embedder,
            dep_graph: std::sync::RwLock::new(None),
        })
    }

    /// Load from disk without delta-checking. Used by `open` and tests.
    fn load_unlocked(paths: &IndexPaths) -> io::Result<Self> {
        let meta: Meta = read_meta(&paths.meta_json)?;
        if meta.schema != SCHEMA {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("schema {} != {}", meta.schema, SCHEMA),
            ));
        }
        if meta.model.dim as usize != DIM {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("model dim {} != {DIM}", meta.model.dim),
            ));
        }

        let chunks: Vec<Chunk> = read_bincode(&paths.chunks_bin)?;
        let files: Vec<FileRecord> = read_bincode(&paths.files_bin)?;
        let bm25: Bm25Index = read_bincode(&paths.bm25_bin)?;
        let embeddings = read_embeddings(&paths.embeddings_f32)?;

        if embeddings.len() != chunks.len() * DIM {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "embeddings.f32 length {} != chunks ({}) × DIM ({DIM})",
                    embeddings.len(),
                    chunks.len()
                ),
            ));
        }

        let model_dir = ensure_model(&ModelInfo::potion_code_16m())?;
        let embedder = Arc::new(Embedder::open(&model_dir)?);

        Ok(Self {
            paths: paths.clone(),
            meta,
            chunks,
            embeddings,
            bm25,
            files,
            embedder,
            dep_graph: std::sync::RwLock::new(None),
        })
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Hybrid BM25 + dense search with full ranking pipeline.
    pub fn search(&self, query: &str, opts: &SearchOptions) -> Vec<SearchHit> {
        if self.chunks.is_empty() || opts.top_k == 0 {
            return Vec::new();
        }
        let alpha = resolve_alpha(query, opts.alpha);
        let candidate_count = opts.top_k * 5;

        // Build language mask once if filtering.
        let mask = build_language_mask(&self.chunks, opts.languages.as_deref());

        // Semantic top-N.
        let q_embed = self.embedder.encode_one(query);
        let semantic_scored = cosine_topk(
            &q_embed,
            &self.embeddings,
            mask.as_deref(),
            candidate_count,
        );

        // BM25 top-N.
        let query_tokens = tokenize(query);
        let bm25_scores = if query_tokens.is_empty() {
            Vec::new()
        } else {
            let raw = self.bm25.get_scores(&query_tokens, mask.as_deref());
            top_k_indices(&raw, candidate_count)
        };

        // RRF + alpha combine.
        let sem_rrf = rrf_scores(&semantic_scored);
        let bm25_rrf = rrf_scores(&bm25_scores);
        let combined = combine(&sem_rrf, &bm25_rrf, alpha);

        // File coherence + query-aware boosts.
        let mut scored = combined;
        boost_multi_chunk_files(&mut scored, &self.chunks);
        let scored = apply_query_boost(scored, query, &self.chunks);

        // Final top-k with path penalties + saturation decay.
        let ranked = rerank_topk(&scored, &self.chunks, opts.top_k, /* penalise_paths */ true);
        ranked
            .into_iter()
            .map(|(id, score)| SearchHit {
                chunk: self.chunks[id as usize].clone(),
                score,
            })
            .collect()
    }

    /// Lazily load the dep graph cache (if any). Returns None when no
    /// fresh cache exists — `find_related` then skips the boost.
    fn dep_graph_cached(&self) -> Option<crate::deps::DepGraph> {
        {
            let guard = self.dep_graph.read().ok()?;
            if let Some(slot) = guard.as_ref() {
                return slot.clone();
            }
        }
        let loaded = crate::deps::cache::load_if_fresh(&self.paths.root);
        if let Ok(mut w) = self.dep_graph.write() {
            *w = Some(loaded.clone());
        }
        loaded
    }

    /// Semantic-only similarity from a chunk identified by its file + line.
    /// Filters to chunks of the same language and excludes the source itself.
    /// When a fresh dep-graph cache exists, also applies a multiplicative
    /// boost to chunks in the importer/importee neighbourhood.
    pub fn find_related(
        &self,
        file_path: &str,
        line: u32,
        top_k: usize,
    ) -> Option<Vec<SearchHit>> {
        self.find_related_opts(file_path, line, top_k, /* dep_boost */ true, /* dep_depth */ 2)
    }

    pub fn find_related_opts(
        &self,
        file_path: &str,
        line: u32,
        top_k: usize,
        dep_boost: bool,
        dep_depth: usize,
    ) -> Option<Vec<SearchHit>> {
        let source_id = resolve_chunk(&self.chunks, file_path, line)?;
        let source = &self.chunks[source_id as usize];

        // Build language-restricted + self-excluding mask.
        let mut mask = vec![false; self.chunks.len()];
        for (i, c) in self.chunks.iter().enumerate() {
            mask[i] = i as u32 != source_id && c.language == source.language;
        }

        // Pull a wider candidate window when boosting so the boost can
        // promote items that wouldn't be in the top-k by raw similarity.
        let candidate_k = if dep_boost { top_k * 5 } else { top_k };
        let q_embed = self.embedder.encode_one(&source.content);
        let mut scored = cosine_topk(&q_embed, &self.embeddings, Some(&mask), candidate_k);

        if dep_boost {
            if let Some(graph) = self.dep_graph_cached() {
                let abs_source = self.paths.root.join(&source.file_path);
                let abs_source = abs_source.canonicalize().unwrap_or(abs_source);
                let depths = crate::deps::traverse::neighbourhood_depths(&graph, &abs_source, dep_depth);
                if !depths.is_empty() {
                    for (id, score) in scored.iter_mut() {
                        let chunk = &self.chunks[*id as usize];
                        let abs = self.paths.root.join(&chunk.file_path);
                        let abs = abs.canonicalize().unwrap_or(abs);
                        if let Some(d) = depths.get(&abs) {
                            *score *= match *d {
                                0 => 1.0, // self — masked already
                                1 => 1.40,
                                2 => 1.20,
                                _ => 1.0,
                            };
                        }
                    }
                    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    scored.truncate(top_k);
                }
            }
        }

        // Truncate (no-op if dep_boost was off).
        scored.truncate(top_k);

        Some(
            scored
                .into_iter()
                .map(|(id, score)| SearchHit {
                    chunk: self.chunks[id as usize].clone(),
                    score,
                })
                .collect(),
        )
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

fn build_language_mask(chunks: &[Chunk], languages: Option<&[String]>) -> Option<Vec<bool>> {
    let langs = languages?;
    if langs.is_empty() {
        return None;
    }
    Some(
        chunks
            .iter()
            .map(|c| langs.iter().any(|l| l == &c.language))
            .collect(),
    )
}

/// Convert a dense scores vector into the top-k `(id, score)` pairs (descending).
/// Used for BM25 (which returns one score per chunk).
fn top_k_indices(scores: &[f32], k: usize) -> Vec<(u32, f32)> {
    if scores.is_empty() || k == 0 {
        return Vec::new();
    }
    let take = k.min(scores.len());
    let mut idx: Vec<u32> = (0..scores.len() as u32).collect();
    idx.select_nth_unstable_by(take - 1, |&a, &b| {
        scores[b as usize]
            .partial_cmp(&scores[a as usize])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut top: Vec<u32> = idx.into_iter().take(take).collect();
    top.sort_by(|&a, &b| {
        scores[b as usize]
            .partial_cmp(&scores[a as usize])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Drop zero-score entries — BM25 zeros mean "no query token matched".
    top.into_iter()
        .map(|i| (i, scores[i as usize]))
        .filter(|(_, s)| *s > 0.0)
        .collect()
}

/// Find the chunk that best contains `file_path:line`.
fn resolve_chunk(chunks: &[Chunk], file_path: &str, line: u32) -> Option<u32> {
    let normalised = file_path.replace('\\', "/");
    let mut fallback: Option<u32> = None;
    for (i, c) in chunks.iter().enumerate() {
        if c.file_path != normalised {
            continue;
        }
        if c.start_line <= line && line < c.end_line {
            return Some(i as u32);
        }
        if line == c.end_line {
            fallback = Some(i as u32);
        }
    }
    fallback
}

/// Append file path components to chunk content to boost path-based queries.
fn enrich_for_bm25(chunk: &Chunk) -> String {
    let path = Path::new(&chunk.file_path);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let dir_parts: Vec<&str> = path
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| c.as_os_str().to_str())
                .filter(|s| *s != "." && *s != "/")
                .collect()
        })
        .unwrap_or_default();
    let dir_text: String = dir_parts
        .iter()
        .rev()
        .take(3)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" ");
    format!("{} {} {} {}", chunk.content, stem, stem, dir_text)
}

fn walk_and_chunk(repo_root: &Path) -> (Vec<PathBuf>, Vec<Vec<Chunk>>) {
    // Collect indexable paths first so chunking can run in parallel.
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut builder = WalkBuilder::new(repo_root);
    add_filters(&mut builder);
    let walker = builder.build();
    for entry in walker.flatten() {
        let p = entry.path();
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        if is_indexable(p).is_none() {
            continue;
        }
        if should_skip_path(p, repo_root) {
            continue;
        }
        paths.push(p.to_path_buf());
    }
    paths.sort(); // deterministic order

    let chunks_per_file: Vec<Vec<Chunk>> = paths
        .par_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(repo_root)
                .map(normalise_path)
                .unwrap_or_else(|_| p.display().to_string());
            chunk_file(p, &rel)
        })
        .collect();

    (paths, chunks_per_file)
}

fn ensure_gitignore(paths: &IndexPaths) -> io::Result<()> {
    if paths.gitignore.exists() {
        return Ok(());
    }
    if let Some(parent) = paths.gitignore.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&paths.gitignore, "*\n")?;
    Ok(())
}

fn acquire_lock(paths: &IndexPaths) -> io::Result<fs::File> {
    let lock_file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&paths.lock)?;
    lock_file.lock_exclusive().map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("could not acquire index lock: {e}"),
        )
    })?;
    Ok(lock_file)
}

fn write_meta(path: &Path, meta: &Meta) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(meta)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    write_atomic(path, &json)
}

fn read_meta(path: &Path) -> io::Result<Meta> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn write_bincode<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = bincode::serde::encode_to_vec(value, bincode::config::standard())
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    write_atomic(path, &bytes)
}

fn read_bincode<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    let bytes = fs::read(path)?;
    let (value, _): (T, _) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(value)
}

fn write_embeddings(path: &Path, values: &[f32]) -> io::Result<()> {
    // Header-less, contiguous little-endian f32. Length is known from
    // chunk_count × DIM in meta.json.
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for v in values {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    write_atomic(path, &bytes)
}

fn read_embeddings(path: &Path) -> io::Result<Vec<f32>> {
    let bytes = fs::read(path)?;
    if bytes.len() % 4 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "embeddings.f32 length not a multiple of 4",
        ));
    }
    let n = bytes.len() / 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let arr: [u8; 4] = bytes[i * 4..i * 4 + 4].try_into().unwrap();
        out.push(f32::from_le_bytes(arr));
    }
    Ok(out)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    Ok(())
}

fn mtime_nanos(meta: &fs::Metadata) -> i128 {
    let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    match mtime.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    }
}

fn normalise_path(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    fn tmp_repo() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write_file(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    #[test]
    fn index_paths_layout() {
        let p = IndexPaths::from_repo(Path::new("/r"));
        assert!(p.index_dir.ends_with(".ast-outline/index"));
        assert!(p.gitignore.ends_with(".ast-outline/.gitignore"));
        assert!(p.meta_json.ends_with("meta.json"));
        assert!(p.embeddings_f32.ends_with("embeddings.f32"));
    }

    #[test]
    fn enrich_for_bm25_includes_stem_twice_and_dirs() {
        let chunk = Chunk {
            content: "fn x() {}".to_string(),
            file_path: "src/auth/login.rs".to_string(),
            start_line: 1,
            end_line: 1,
            start_byte: 0,
            end_byte: 9,
            language: "rust".to_string(),
        };
        let enriched = enrich_for_bm25(&chunk);
        // Stem appears twice; "src" and "auth" appear once each in dir text.
        let count = |s: &str, n: &str| s.matches(n).count();
        assert_eq!(count(&enriched, "login"), 2);
        assert!(enriched.contains("src"));
        assert!(enriched.contains("auth"));
    }

    #[test]
    fn resolve_chunk_finds_overlapping() {
        let mk = |sl, el| Chunk {
            content: String::new(),
            file_path: "f.rs".to_string(),
            start_line: sl,
            end_line: el,
            start_byte: 0,
            end_byte: 0,
            language: "rust".to_string(),
        };
        let chunks = vec![mk(1, 10), mk(20, 30), mk(40, 50)];
        assert_eq!(resolve_chunk(&chunks, "f.rs", 5), Some(0));
        assert_eq!(resolve_chunk(&chunks, "f.rs", 25), Some(1));
        assert_eq!(resolve_chunk(&chunks, "f.rs", 9), Some(0));
        // line == end_line: fallback path.
        assert_eq!(resolve_chunk(&chunks, "f.rs", 50), Some(2));
        // No matching file.
        assert_eq!(resolve_chunk(&chunks, "other.rs", 5), None);
        // Out-of-range line.
        assert_eq!(resolve_chunk(&chunks, "f.rs", 60), None);
    }

    #[test]
    fn top_k_indices_orders_and_drops_zeros() {
        let scores = vec![0.0, 0.5, 0.0, 0.9, 0.1];
        let top = top_k_indices(&scores, 5);
        assert_eq!(top.len(), 3); // zeros dropped
        assert_eq!(top[0].0, 3);
        assert_eq!(top[1].0, 1);
        assert_eq!(top[2].0, 4);
    }

    #[test]
    fn build_language_mask_filters() {
        let mk = |lang: &str| Chunk {
            content: String::new(),
            file_path: String::new(),
            start_line: 0,
            end_line: 0,
            start_byte: 0,
            end_byte: 0,
            language: lang.to_string(),
        };
        let chunks = vec![mk("rust"), mk("python"), mk("rust"), mk("go")];
        let mask = build_language_mask(&chunks, Some(&["rust".to_string()]));
        assert_eq!(mask, Some(vec![true, false, true, false]));
        let none = build_language_mask(&chunks, None);
        assert!(none.is_none());
    }

    /// Smoke test of the persistence round-trip without touching the embedder.
    /// Builds tiny structures by hand, writes, reads back, asserts equality.
    #[test]
    fn persistence_roundtrip() {
        let dir = tmp_repo();
        let paths = IndexPaths::from_repo(dir.path());
        fs::create_dir_all(&paths.index_dir).unwrap();

        let chunks = vec![Chunk {
            content: "hello".to_string(),
            file_path: "a.rs".to_string(),
            start_line: 1,
            end_line: 1,
            start_byte: 0,
            end_byte: 5,
            language: "rust".to_string(),
        }];
        let files = vec![FileRecord {
            path: "a.rs".to_string(),
            mtime_ns: 0,
            size: 5,
            content_hash: 0,
            chunk_start: 0,
            chunk_end: 1,
        }];
        let bm25 = Bm25Index::build(vec![vec!["hello".to_string()]]);
        let embeddings = vec![0.0; DIM];
        let meta = Meta {
            schema: SCHEMA.to_string(),
            ast_outline_version: env!("CARGO_PKG_VERSION").to_string(),
            model: ModelMeta { id: "m".into(), dim: DIM as u32 },
            created_unix: 0,
            chunk_count: 1,
            embedding_dtype: "f32_le".to_string(),
            tombstones: Vec::new(),
        };

        write_meta(&paths.meta_json, &meta).unwrap();
        write_bincode(&paths.chunks_bin, &chunks).unwrap();
        write_bincode(&paths.files_bin, &files).unwrap();
        write_bincode(&paths.bm25_bin, &bm25).unwrap();
        write_embeddings(&paths.embeddings_f32, &embeddings).unwrap();

        let meta2: Meta = read_meta(&paths.meta_json).unwrap();
        let chunks2: Vec<Chunk> = read_bincode(&paths.chunks_bin).unwrap();
        let files2: Vec<FileRecord> = read_bincode(&paths.files_bin).unwrap();
        let _bm25_2: Bm25Index = read_bincode(&paths.bm25_bin).unwrap();
        let emb2 = read_embeddings(&paths.embeddings_f32).unwrap();

        assert_eq!(meta2.chunk_count, 1);
        assert_eq!(chunks2, chunks);
        assert_eq!(files2, files);
        assert_eq!(emb2, embeddings);
    }

    /// Full end-to-end: build, search, find_related against a tiny tmp repo.
    /// Network-gated — requires the model be downloadable.
    #[test]
    #[ignore]
    fn network_end_to_end_build_and_search() {
        let dir = tmp_repo();
        // Plant some Rust files with semantically distinct content.
        write_file(
            dir.path(),
            "src/auth/login.rs",
            "pub fn login(username: &str, password: &str) -> bool { username == \"admin\" }",
        );
        write_file(
            dir.path(),
            "src/auth/logout.rs",
            "pub fn logout(session_id: &str) { drop_session(session_id) }",
        );
        write_file(
            dir.path(),
            "src/http/handler.rs",
            "pub struct HandlerStack { items: Vec<u32> }",
        );

        let index = Index::build(dir.path()).expect("build failed");
        assert!(index.chunk_count() >= 3);

        // Symbol query: should rank handler.rs first.
        let hits = index.search("HandlerStack", &SearchOptions::with_top_k(3));
        assert!(!hits.is_empty());
        assert!(hits[0].chunk.file_path.contains("handler.rs"));

        // NL query: should rank one of the auth files first.
        let hits = index.search("how does login work", &SearchOptions::with_top_k(3));
        assert!(!hits.is_empty());
        assert!(hits[0].chunk.file_path.contains("login.rs"));

        // find-related from login.rs:1 should pull logout.rs (same lang, related).
        let related = index
            .find_related("src/auth/login.rs", 1, 5)
            .expect("source chunk not found");
        assert!(!related.is_empty());
        // The source chunk itself must be excluded.
        assert!(related.iter().all(|h| !h.chunk.file_path.contains("login.rs")));

        // Re-open from cache: should detect no changes and skip rebuild.
        let reopened = Index::open(dir.path()).expect("re-open failed");
        assert_eq!(reopened.chunk_count(), index.chunk_count());
    }
}
