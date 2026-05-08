//! CLI handler functions called from `src/main.rs`. Each one mirrors a
//! single subcommand and returns the process exit code (0 success, 2
//! user error, 1 internal error).

use std::path::{Path, PathBuf};

use crate::deps::render;
use crate::deps::scc;
use crate::deps::traverse;
use crate::deps::{load_or_build, DepGraph};
use crate::project_root::{relative_posix, resolve_home, Marker};

pub fn run_deps(
    file: &Path,
    depth: usize,
    json: bool,
    pretty: bool,
    rebuild: bool,
) -> i32 {
    if file.is_dir() {
        eprintln!(
            "# note: `deps` expects a file, not a directory.\n  \
             Use `ast-outline graph <dir>` to visualize the full dependency graph."
        );
        return 2;
    }
    let root = match find_root_for_with_cache(file) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("# note: {}", e);
            return 2;
        }
    };
    let graph = match load_or_build(&root, rebuild) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("# note: {}", e);
            return 1;
        }
    };
    let canonical = match canonicalise_in_root(file, &graph) {
        Some(p) => p,
        None => {
            eprintln!(
                "# note: {} is not part of the dep graph (excluded by .gitignore or unsupported language?)",
                file.display()
            );
            return 2;
        }
    };
    let hits = traverse::forward(&graph, &canonical, depth.max(1));
    if json {
        println!("{}", render::render_deps_json(&graph, &canonical, &hits, pretty));
    } else {
        print!("{}", render::render_deps_text(&graph, &canonical, &hits));
    }
    0
}

pub fn run_reverse_deps(
    file: &Path,
    depth: usize,
    limit: usize,
    json: bool,
    pretty: bool,
    rebuild: bool,
) -> i32 {
    if file.is_dir() {
        eprintln!(
            "# note: `reverse-deps` expects a file, not a directory.\n  \
             Use `ast-outline graph <dir>` to visualize the full dependency graph."
        );
        return 2;
    }
    let root = match find_root_for_with_cache(file) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("# note: {}", e);
            return 2;
        }
    };
    let graph = match load_or_build(&root, rebuild) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("# note: {}", e);
            return 1;
        }
    };
    let canonical = match canonicalise_in_root(file, &graph) {
        Some(p) => p,
        None => {
            eprintln!(
                "# note: {} is not part of the dep graph",
                file.display()
            );
            return 2;
        }
    };
    let hits = traverse::reverse(&graph, &canonical, depth.max(1), limit);
    if json {
        println!(
            "{}",
            render::render_reverse_deps_json(&graph, &canonical, &hits, pretty)
        );
    } else {
        print!(
            "{}",
            render::render_reverse_deps_text(&graph, &canonical, &hits)
        );
    }
    0
}

pub fn run_cycles(
    path: &Path,
    min_size: usize,
    json: bool,
    pretty: bool,
    rebuild: bool,
) -> i32 {
    let cwd = current_dir_or_dot();
    let (root, scope) = match resolve_dir_root_and_scope(path, &cwd) {
        Ok(rs) => rs,
        Err(e) => {
            eprintln!("# note: {}", e);
            return 2;
        }
    };
    let graph = match load_or_build(&root, rebuild) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("# note: {}", e);
            return 1;
        }
    };
    // Detect cycles on the full cached graph, then keep only those whose
    // members are entirely within the user's requested scope.
    let mut cycles = scc::detect(&graph, min_size);
    if !scope.is_empty() {
        cycles.retain(|c| c.members.iter().all(|m| graph.in_scope(m, &scope)));
    }
    if json {
        println!(
            "{}",
            render::render_cycles_json(&graph, &cycles, pretty)
        );
    } else {
        print!("{}", render::render_cycles_text(&graph, &cycles));
    }
    if cycles.is_empty() {
        0
    } else {
        // Non-zero exit so this can be wired into CI gates.
        3
    }
}

pub fn run_graph(
    path: &Path,
    json: bool,
    include_external: bool,
    pretty: bool,
    rebuild: bool,
) -> i32 {
    // `graph` expects a directory (repo root), not a file.
    // Give a helpful hint if the user passes a file path.
    if path.is_file() {
        eprintln!(
            "# note: `graph` expects a directory, not a file.\n\
             For per-file dependency analysis, use:\n  \
             ast-outline deps <file>       # what this file imports\n  \
             ast-outline reverse-deps <file>  # who imports this file"
        );
        return 2;
    }
    let cwd = current_dir_or_dot();
    let (root, scope) = match resolve_dir_root_and_scope(path, &cwd) {
        Ok(rs) => rs,
        Err(e) => {
            eprintln!("# note: {}", e);
            return 2;
        }
    };
    let full = match load_or_build(&root, rebuild) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("# note: {}", e);
            return 1;
        }
    };
    let graph = if scope.is_empty() {
        full
    } else {
        full.subgraph_for_scope(&scope)
    };
    if json {
        println!("{}", render::render_graph_json(&graph, include_external, pretty));
    } else {
        print!("{}", render::render_graph_text(&graph));
    }
    0
}

/// Resolve `(graph_root, scope)` for directory-arg subcommands (cycles,
/// graph). Walks up from `path` looking for an existing `.ast-outline/deps/`
/// (capped at `cwd`); falls back to building at `cwd` when `path` is under
/// `cwd`, else `path` itself. `scope` is `path` expressed relative to the
/// resolved root (POSIX, "" = whole root).
fn resolve_dir_root_and_scope(path: &Path, cwd: &Path) -> Result<(PathBuf, String), String> {
    if !path.exists() {
        return Err(format!("path not found: {}", path.display()));
    }
    let (home, _) = resolve_home(path, cwd, Marker::DepsCache);
    let scope = relative_posix(path, &home).unwrap_or_default();
    Ok((home, scope))
}

fn current_dir_or_dot() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Prefer an existing `.ast-outline/deps/` cache between `file` and CWD.
/// Falls back to the manifest walk (`find_root_for`) when no cache exists,
/// preserving the historical behaviour for users running `deps` for the
/// first time.
fn find_root_for_with_cache(file: &Path) -> Result<PathBuf, String> {
    if !file.exists() {
        return Err(format!("file not found: {}", file.display()));
    }
    let cwd = current_dir_or_dot();
    let (home, found) = resolve_home(file, &cwd, Marker::DepsCache);
    if found {
        return Ok(home);
    }
    find_root_for(file)
}

/// Look for the project root (containing a known manifest) starting
/// at the file's directory and walking up. Falls back to current dir.
pub fn find_root_for(file: &Path) -> Result<PathBuf, String> {
    if !file.exists() {
        return Err(format!("file not found: {}", file.display()));
    }
    let abs = file
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {}", file.display(), e))?;
    let mut cur: &Path = if abs.is_dir() {
        &abs
    } else {
        abs.parent().ok_or("no parent directory")?
    };
    let manifest_names = [
        "Cargo.toml",
        "go.mod",
        "package.json",
        "pyproject.toml",
        "build.gradle",
        "build.gradle.kts",
        "build.sbt",
        "pom.xml",
    ];
    loop {
        for n in &manifest_names {
            if cur.join(n).is_file() {
                return Ok(cur.to_path_buf());
            }
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => break,
        }
    }
    // Fall back to file's parent directory.
    Ok(if abs.is_dir() {
        abs
    } else {
        abs.parent()
            .ok_or("no parent directory")?
            .to_path_buf()
    })
}

fn canonicalise_in_root(file: &Path, graph: &DepGraph) -> Option<PathBuf> {
    let abs = file.canonicalize().ok()?;
    if graph.forward.contains_key(&abs) {
        return Some(abs);
    }
    // Try matching by suffix — user may have passed a relative path.
    let target_str = abs.to_string_lossy();
    for known in graph.forward.keys() {
        if known.to_string_lossy() == target_str {
            return Some(known.clone());
        }
    }
    None
}
