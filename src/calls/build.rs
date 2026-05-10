//! `build_call_graph(root, deps)` — populate the call-graph half of the
//! unified cache. Walks every file the dep graph already knows about,
//! re-parses each through `parse_file_for_hook` (so `Declaration::calls`
//! and `ParseResult::imports` are populated), then runs the three resolver
//! passes from `resolve.rs`.
//!
//! Why re-parse instead of cache `ParseResult`s alongside the dep graph:
//! `Declaration` carries the full source `Vec<u8>` — keeping it in the
//! bincode cache would balloon disk size 10× for no win. Re-parsing inside
//! `rayon::par_iter` is fast (the parse-for-hook path is what `map`/`show`
//! already use, and is sub-millisecond per file).

use crate::calls::graph::{CallEdge, CallGraph, CallKindCompat, CallTarget, CallableMeta, Qn, TypeMeta};
use crate::calls::resolve;
use crate::core::{CallSite, Declaration, DeclarationKind, ImportBinding};
use crate::deps::DepGraph;
use crate::main_helpers::parse_file_for_hook;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Per-file output of phase 1: the file's qns, its imports, and its raw
/// (unresolved) call-edges. Phase 2 in `resolve.rs` joins these globally.
pub struct FilePass {
    pub file: PathBuf,
    pub language: &'static str,
    /// All qns this file defines — top-level + nested. Order matters only
    /// for stable output; pass A consumes as a set.
    pub defined: Vec<Qn>,
    /// Per-callable file/line metadata for the qns in `defined`. Same
    /// length and order. Aggregated into `CallGraph::callable_meta`.
    pub callable_locations: Vec<CallableMeta>,
    /// `local_name → module spec` from `use`/`import`/`using` statements.
    pub imports: Vec<ImportBinding>,
    /// One entry per call site in the file. `target` is `Bare` here; the
    /// resolver promotes it.
    pub raw_edges: Vec<RawEdge>,
    /// Type declarations — class/struct/interface/trait/enum/record. Carries
    /// `bases` so the implementors-reverse index can be built without a
    /// second walk. Independent of the call-edge pipeline.
    pub types: Vec<(Qn, TypeMeta)>,
}

#[derive(Debug, Clone)]
pub struct RawEdge {
    pub source: Qn,
    pub bare_name: String,
    pub receiver: Option<String>,
    pub kind: CallKindCompat,
    pub line: u32,
}

pub fn build_call_graph(root: &Path, deps: &DepGraph) -> CallGraph {
    let start = Instant::now();
    let files: Vec<PathBuf> = deps.forward.keys().cloned().collect();

    let passes: Vec<FilePass> = files
        .par_iter()
        .filter_map(|file| extract_file(root, file))
        .collect();

    // Pull the type half + per-callable locations out before passing the
    // rest to the resolver — the resolver only cares about callable qns
    // and their edges, not their source positions.
    let mut callable_meta: HashMap<Qn, CallableMeta> = HashMap::new();
    let mut types: HashMap<Qn, TypeMeta> = HashMap::new();
    let mut type_by_name: HashMap<String, Vec<Qn>> = HashMap::new();
    let mut implementors: HashMap<String, Vec<Qn>> = HashMap::new();
    for fp in &passes {
        for (qn, meta) in fp.defined.iter().zip(fp.callable_locations.iter()) {
            callable_meta.insert(qn.clone(), meta.clone());
        }
        for (qn, meta) in &fp.types {
            types.insert(qn.clone(), meta.clone());
            type_by_name
                .entry(qn.name().to_string())
                .or_default()
                .push(qn.clone());
            for base in &meta.bases {
                let normalised = normalise_type_name(base);
                implementors.entry(normalised).or_default().push(qn.clone());
            }
        }
    }
    for v in type_by_name.values_mut() {
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v.dedup();
    }
    for v in implementors.values_mut() {
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v.dedup();
    }

    let resolved = resolve::run(root, deps, passes);
    let mut graph = CallGraph::empty(root.to_path_buf());
    graph.symbol_table = resolved.symbol_table;
    graph.forward = resolved.forward;
    graph.callable_meta = callable_meta;
    graph.types = types;
    graph.type_by_name = type_by_name;
    graph.implementors = implementors;
    graph.rebuild_reverse();

    // Stats roll-up.
    let mut stats = crate::calls::graph::GraphStats::default();
    stats.function_count = graph.forward.len();
    stats.type_count = graph.types.len();
    for edges in graph.forward.values() {
        for e in edges {
            stats.edge_count += 1;
            match (&e.target, e.confidence) {
                (CallTarget::Resolved(_), _) => stats.resolved_edge_count += 1,
                (CallTarget::External(_), _) => stats.external_edge_count += 1,
                (CallTarget::Bare(_), _) => stats.ambiguous_edge_count += 1,
            }
        }
    }
    stats.build_ms = start.elapsed().as_millis() as u64;
    graph.stats = stats;
    graph
}

/// Mirror of `core::_normalize_type_name` — strip generics, brackets,
/// dotted/`::`-prefixed scope so `crate::base::LanguageAdapter`,
/// `LanguageAdapter<T>`, and `LanguageAdapter` all hash to the same key.
fn normalise_type_name(name: &str) -> String {
    let mut name = name.trim();
    if let Some(i) = name.find('<') {
        name = &name[..i];
    }
    if let Some(i) = name.find('[') {
        name = &name[..i];
    }
    if let Some(i) = name.rfind('.') {
        name = &name[i + 1..];
    }
    if let Some(i) = name.rfind("::") {
        name = &name[i + 2..];
    }
    name.to_string()
}

/// Phase 1 per-file: parse, walk Declarations, emit qns + raw edges + types.
fn extract_file(root: &Path, file: &Path) -> Option<FilePass> {
    let parse = parse_file_for_hook(file)?;
    let language = parse.language;
    let rel = file_rel(root, file);

    let mut defined = Vec::new();
    let mut callable_locations = Vec::new();
    let mut raw_edges = Vec::new();
    let mut types = Vec::new();
    walk(
        &parse.declarations,
        &rel,
        Vec::new(),
        &mut defined,
        &mut callable_locations,
        &mut raw_edges,
        &mut types,
    );

    Some(FilePass {
        file: file.to_path_buf(),
        language,
        defined,
        callable_locations,
        imports: parse.imports,
        raw_edges,
        types,
    })
}

/// Recursive walker collecting:
/// - one `Qn` per callable declaration (functions / methods / ctors).
/// - one `RawEdge` per `CallSite` attached to a callable declaration.
/// - one `(Qn, TypeMeta)` per type declaration (class / struct / interface
///   / enum / record — Rust traits land here as `Interface` via the adapter).
fn walk(
    decls: &[Declaration],
    rel_file: &str,
    parents: Vec<String>,
    defined: &mut Vec<Qn>,
    callable_locations: &mut Vec<CallableMeta>,
    raw_edges: &mut Vec<RawEdge>,
    types: &mut Vec<(Qn, TypeMeta)>,
) {
    for d in decls {
        let mut next_parents = parents.clone();
        if !d.name.is_empty() {
            next_parents.push(d.name.clone());
        }

        if is_callable(d) && !d.name.is_empty() {
            let qn = qn_from(rel_file, &next_parents);
            defined.push(qn.clone());
            callable_locations.push(CallableMeta {
                file: PathBuf::from(rel_file),
                line: d.start_line as u32,
                kind: d.kind.to_string(),
            });
            for cs in &d.calls {
                raw_edges.push(call_to_raw(qn.clone(), cs));
            }
        } else if is_type(d) && !d.name.is_empty() {
            let qn = qn_from(rel_file, &next_parents);
            types.push((
                qn,
                TypeMeta {
                    kind: d
                        .native_kind
                        .clone()
                        .unwrap_or_else(|| d.kind.to_string()),
                    file: PathBuf::from(rel_file),
                    line: d.start_line as u32,
                    bases: d.bases.clone(),
                },
            ));
        }

        if !d.children.is_empty() {
            walk(
                &d.children,
                rel_file,
                next_parents,
                defined,
                callable_locations,
                raw_edges,
                types,
            );
        }
    }
}

fn is_callable(d: &Declaration) -> bool {
    use DeclarationKind::*;
    matches!(
        d.kind,
        Function | Method | Constructor | Destructor | Operator
    )
}

fn is_type(d: &Declaration) -> bool {
    use DeclarationKind::*;
    matches!(d.kind, Class | Struct | Interface | Enum | Record)
}

fn call_to_raw(source: Qn, cs: &CallSite) -> RawEdge {
    RawEdge {
        source,
        bare_name: cs.name.clone(),
        receiver: cs.receiver.clone(),
        kind: CallKindCompat::from(cs.kind),
        line: cs.line,
    }
}

pub fn qn_from(rel_file: &str, parents: &[String]) -> Qn {
    let scope = parents.join("::");
    if scope.is_empty() {
        Qn::new(rel_file.to_string())
    } else {
        Qn::new(format!("{}::{}", rel_file, scope))
    }
}

pub fn file_rel(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[allow(dead_code)]
pub(crate) fn raw_to_edge(raw: RawEdge, target: CallTarget, confidence: crate::calls::graph::Confidence, file: PathBuf, candidates: Vec<Qn>) -> CallEdge {
    CallEdge {
        source: raw.source,
        target,
        kind: raw.kind,
        line: raw.line,
        file,
        confidence,
        receiver: raw.receiver,
        candidates,
    }
}
