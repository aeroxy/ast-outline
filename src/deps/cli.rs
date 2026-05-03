//! CLI handler functions called from `src/main.rs`. Each one mirrors a
//! single subcommand and returns the process exit code (0 success, 2
//! user error, 1 internal error).

use std::path::{Path, PathBuf};

use crate::deps::dsm;
use crate::deps::render;
use crate::deps::scc;
use crate::deps::traverse;
use crate::deps::{load_or_build, DepGraph};

pub fn run_deps(
    file: &Path,
    depth: usize,
    json: bool,
    pretty: bool,
    rebuild: bool,
) -> i32 {
    let root = match find_root_for(file) {
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
    let root = match find_root_for(file) {
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
    let root = match path.canonicalize() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("# note: cannot resolve {}: {}", path.display(), e);
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
    let cycles = scc::detect(&graph, min_size);
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
    format: &str,
    include_external: bool,
    pretty: bool,
    rebuild: bool,
) -> i32 {
    let root = match path.canonicalize() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("# note: cannot resolve {}: {}", path.display(), e);
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
    match format {
        "text" => print!("{}", render::render_graph_text(&graph)),
        "dot" => print!("{}", render::render_graph_dot(&graph)),
        "dsm" => {
            let dsm = dsm::build(&graph);
            print!("{}", render::render_graph_dsm(&graph, &dsm));
        }
        "json" => println!("{}", render::render_graph_json(&graph, include_external, pretty)),
        other => {
            eprintln!(
                "# note: unknown --format '{}'. Expected text|json|dot|dsm.",
                other
            );
            return 2;
        }
    }
    0
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
