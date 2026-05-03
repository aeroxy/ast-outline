//! Per-language import extraction. Wraps existing `surface::imports`
//! extractors for Rust/Python/TS/JS/Scala and adds new tree-sitter
//! passes for Java/C#/Kotlin/Go.
//!
//! All extractors emit `RawImport` records — language-agnostic units
//! the resolver consumes. Each record carries enough info that the
//! renderer can show the original statement + line.

use ast_grep_core::{Doc, Node};
use ast_grep_language::{LanguageExt, SupportLang};
use std::path::Path;

use crate::deps::graph::ImportKind;
use crate::deps::resolver::build::Lang;
use crate::surface::imports as surface_imports;

/// One extracted import. Targets are normalised slash-joined module
/// paths (e.g. `com/foo/Bar`, `crate/net/client`); resolver does the
/// final mapping to a file.
#[derive(Debug, Clone)]
pub struct RawImport {
    pub spec: String,
    pub kind: ImportKind,
    pub line: u32,
    /// Display-only: the original statement source line.
    #[allow(dead_code)]
    pub statement: String,
    pub local_name: Option<String>,
    /// Source dotted path (preserves dots, no slashes).
    pub raw_path: Option<String>,
}

/// Top-level dispatch — extract every import from `path`. Returns
/// nothing for unrecognised extensions. Caller is responsible for
/// reading the file (we do it here).
pub fn extract(path: &Path, lang: Lang) -> Vec<RawImport> {
    let Ok(src) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match lang {
        Lang::Rust => extract_rust(&src),
        Lang::Python => extract_python(&src),
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => extract_typescript(&src, lang),
        Lang::Scala => extract_scala(&src),
        Lang::Java => extract_java(&src),
        Lang::Kotlin => extract_kotlin(&src),
        Lang::CSharp => extract_csharp(&src),
        Lang::Go => extract_go(&src),
        Lang::Other => Vec::new(),
    }
}

// ---- Rust ----

fn extract_rust(src: &str) -> Vec<RawImport> {
    let imports = surface_imports::extract_rust_imports(src);
    let mut out = Vec::new();
    for u in imports.uses {
        let kind = match u.kind {
            surface_imports::UseSegmentKind::Item => ImportKind::Use,
            surface_imports::UseSegmentKind::Glob => ImportKind::Glob,
        };
        out.push(RawImport {
            spec: u.path.clone(),
            kind,
            line: u.line as u32,
            statement: u.statement,
            local_name: u.alias,
            raw_path: Some(u.path),
        });
    }
    for m in imports.mods {
        if !m.is_external_file {
            continue;
        }
        out.push(RawImport {
            spec: format!("self::{}", m.name),
            kind: ImportKind::Mod,
            line: m.line as u32,
            statement: format!("mod {};", m.name),
            local_name: None,
            raw_path: Some(m.name),
        });
    }
    out
}

// ---- Python ----

fn extract_python(src: &str) -> Vec<RawImport> {
    let mut out = Vec::new();
    let imports = surface_imports::extract_python_imports(src);
    for fi in imports.from_imports {
        let prefix: String = ".".repeat(fi.relative_dots);
        let module = if fi.module.is_empty() {
            prefix.clone()
        } else {
            format!("{}{}", prefix, fi.module)
        };
        if fi.is_glob {
            // Treat `from x import *` as one edge to the source module.
            let spec = normalise_python(&module, fi.relative_dots);
            out.push(RawImport {
                spec,
                kind: ImportKind::StarFrom,
                line: fi.line as u32,
                statement: fi.statement.clone(),
                local_name: None,
                raw_path: Some(module.clone()),
            });
            continue;
        }
        // For each name imported, emit one edge — first try resolving
        // the full `module.name` path (e.g. `pkg.sub.fn`), the resolver
        // will fall back to dropping the trailing segment.
        if fi.names.is_empty() {
            let spec = normalise_python(&module, fi.relative_dots);
            out.push(RawImport {
                spec,
                kind: ImportKind::From,
                line: fi.line as u32,
                statement: fi.statement.clone(),
                local_name: None,
                raw_path: Some(module),
            });
            continue;
        }
        for n in fi.names {
            let dotted = if fi.module.is_empty() {
                format!("{}{}", prefix, n.name)
            } else {
                format!("{}.{}", module, n.name)
            };
            let spec = normalise_python(&dotted, fi.relative_dots);
            out.push(RawImport {
                spec,
                kind: ImportKind::From,
                line: fi.line as u32,
                statement: fi.statement.clone(),
                local_name: n.alias,
                raw_path: Some(dotted),
            });
        }
    }
    // Bare `import x.y` — surface::imports doesn't extract these, so
    // we run a small ast-grep pass directly.
    out.extend(extract_python_bare(src));
    out
}

fn normalise_python(module: &str, dots: usize) -> String {
    if dots == 0 {
        // Absolute: `a.b.c` → `a/b/c`.
        return module.replace('.', "/");
    }
    // Relative: leading `.` or `..` becomes `./` or `../`.
    let body = module.trim_start_matches('.').replace('.', "/");
    let mut s = String::new();
    if dots == 1 {
        s.push_str("./");
    } else {
        for _ in 0..dots - 1 {
            s.push_str("../");
        }
    }
    s.push_str(&body);
    if s.ends_with('/') {
        s.pop();
    }
    s
}

fn extract_python_bare(src: &str) -> Vec<RawImport> {
    let mut out = Vec::new();
    let lang = SupportLang::Python;
    let ast = lang.ast_grep(src.to_string());
    let root = ast.root();
    _walk_python_bare(&root, &mut out);
    out
}

fn _walk_python_bare<'a, D: Doc>(node: &Node<'a, D>, out: &mut Vec<RawImport>) {
    for c in node.children() {
        if !c.is_named() {
            continue;
        }
        if c.kind() == "import_statement" {
            // Children include `dotted_name` or `aliased_import` for each name.
            let line = (c.start_pos().line() + 1) as u32;
            let stmt = c.text().into_owned();
            for n in c.children() {
                let k = n.kind();
                if k == "dotted_name" {
                    let mod_name = n.text().into_owned();
                    out.push(RawImport {
                        spec: mod_name.replace('.', "/"),
                        kind: ImportKind::Bare,
                        line,
                        statement: stmt.clone(),
                        local_name: None,
                        raw_path: Some(mod_name),
                    });
                } else if k == "aliased_import" {
                    let name = n.field("name").map(|f| f.text().into_owned());
                    let alias = n.field("alias").map(|f| f.text().into_owned());
                    if let Some(name) = name {
                        out.push(RawImport {
                            spec: name.replace('.', "/"),
                            kind: ImportKind::Bare,
                            line,
                            statement: stmt.clone(),
                            local_name: alias,
                            raw_path: Some(name),
                        });
                    }
                }
            }
        }
    }
}

// ---- TS / JS ----

fn extract_typescript(src: &str, lang: Lang) -> Vec<RawImport> {
    let support = match lang {
        Lang::TypeScript => SupportLang::TypeScript,
        Lang::Tsx => SupportLang::Tsx,
        Lang::JavaScript => SupportLang::JavaScript,
        _ => return Vec::new(),
    };
    let ast = support.ast_grep(src.to_string());
    let root = ast.root();
    let mut out = Vec::new();
    _walk_ts(&root, &mut out);
    out
}

fn _walk_ts<'a, D: Doc>(node: &Node<'a, D>, out: &mut Vec<RawImport>) {
    for c in node.children() {
        if !c.is_named() {
            continue;
        }
        let kind = c.kind();
        let kind = kind.as_ref();
        if kind == "import_statement" {
            consume_ts_import(&c, out);
        } else if kind == "export_statement" {
            // Re-exports are also dependency edges.
            consume_ts_export(&c, out);
        } else if kind == "expression_statement" {
            // Top-level CommonJS `require('x')` calls.
            consume_ts_require(&c, out);
        }
    }
}

fn consume_ts_import<'a, D: Doc>(node: &Node<'a, D>, out: &mut Vec<RawImport>) {
    let Some(source) = node.field("source") else {
        return;
    };
    let from = strip_quotes(&source.text());
    if from.is_empty() {
        return;
    }
    let line = (node.start_pos().line() + 1) as u32;
    let stmt = node.text().into_owned();

    // Detect what kind of import this is.
    let mut named_seen = false;
    let mut star_seen = false;
    for c in node.children() {
        let k = c.kind();
        if k == "import_clause" {
            for sub in c.children() {
                let sk = sub.kind();
                if sk == "named_imports" {
                    named_seen = true;
                } else if sk == "namespace_import" {
                    star_seen = true;
                }
            }
        }
    }
    let kind = if star_seen {
        ImportKind::StarFrom
    } else if named_seen {
        ImportKind::NamedFrom
    } else {
        ImportKind::Bare
    };
    out.push(RawImport {
        spec: from.clone(),
        kind,
        line,
        statement: stmt,
        local_name: None,
        raw_path: Some(from),
    });
}

fn consume_ts_export<'a, D: Doc>(node: &Node<'a, D>, out: &mut Vec<RawImport>) {
    let Some(source) = node.field("source") else {
        return;
    };
    let from = strip_quotes(&source.text());
    if from.is_empty() {
        return;
    }
    let line = (node.start_pos().line() + 1) as u32;
    let stmt = node.text().into_owned();
    let mut star = false;
    for c in node.children() {
        let k = c.kind();
        if k == "export_clause" {
            // `export { Foo } from 'x'`
            out.push(RawImport {
                spec: from.clone(),
                kind: ImportKind::NamedFrom,
                line,
                statement: stmt.clone(),
                local_name: None,
                raw_path: Some(from.clone()),
            });
            return;
        } else if k == "*" || k.as_ref() == "namespace_export" {
            star = true;
        }
    }
    out.push(RawImport {
        spec: from.clone(),
        kind: if star {
            ImportKind::StarFrom
        } else {
            ImportKind::NamedFrom
        },
        line,
        statement: stmt,
        local_name: None,
        raw_path: Some(from),
    });
}

fn consume_ts_require<'a, D: Doc>(node: &Node<'a, D>, out: &mut Vec<RawImport>) {
    // Look for `require('x')` or `require("x")` calls, top-level only.
    let text = node.text();
    let s = text.as_ref();
    if !s.contains("require(") {
        return;
    }
    let line = (node.start_pos().line() + 1) as u32;
    if let Some(start) = s.find("require(") {
        let after = &s[start + "require(".len()..];
        let arg = after.split(')').next().unwrap_or("").trim();
        let from = arg.trim_matches('\'').trim_matches('"');
        if !from.is_empty() && (from.starts_with('.') || !from.contains(' ')) {
            out.push(RawImport {
                spec: from.to_string(),
                kind: ImportKind::Bare,
                line,
                statement: s.to_string(),
                local_name: None,
                raw_path: Some(from.to_string()),
            });
        }
    }
}

fn strip_quotes(s: &std::borrow::Cow<'_, str>) -> String {
    let t = s.trim();
    if (t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')) {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

// ---- Scala ----

fn extract_scala(src: &str) -> Vec<RawImport> {
    let lang = SupportLang::Scala;
    let ast = lang.ast_grep(src.to_string());
    let root = ast.root();
    let mut out = Vec::new();
    _walk_scala(&root, &mut out);
    out
}

fn _walk_scala<'a, D: Doc>(node: &Node<'a, D>, out: &mut Vec<RawImport>) {
    for c in node.children() {
        if !c.is_named() {
            continue;
        }
        let kind = c.kind();
        let kind = kind.as_ref();
        if kind == "import_declaration" {
            // The import's text is `import a.b.c` or `import a.b.{c, d}`.
            let line = (c.start_pos().line() + 1) as u32;
            let stmt = c.text().into_owned();
            // Heuristic: take the part after `import `, split on `{` for selectors.
            let after_kw = stmt.trim_start_matches("import").trim_start();
            let (base, selectors) = match after_kw.split_once('{') {
                Some((a, sel)) => (a.trim().trim_end_matches('.').to_string(), Some(sel)),
                None => (after_kw.trim().to_string(), None),
            };
            if let Some(sel) = selectors {
                let inner = sel.trim_end_matches('}');
                for raw in inner.split(',') {
                    let part = raw.trim();
                    if part.is_empty() {
                        continue;
                    }
                    let (name, alias) = match part.split_once("=>") {
                        Some((n, a)) => (n.trim().to_string(), Some(a.trim().to_string())),
                        None => (part.to_string(), None),
                    };
                    let spec = if name == "_" {
                        base.clone()
                    } else {
                        format!("{}.{}", base, name)
                    };
                    out.push(RawImport {
                        spec: spec.replace('.', "/"),
                        kind: if name == "_" {
                            ImportKind::StarFrom
                        } else {
                            ImportKind::Bare
                        },
                        line,
                        statement: stmt.clone(),
                        local_name: alias,
                        raw_path: Some(spec),
                    });
                }
            } else {
                let kind = if base.ends_with("._") {
                    ImportKind::StarFrom
                } else {
                    ImportKind::Bare
                };
                let dotted = base.trim_end_matches("._").to_string();
                out.push(RawImport {
                    spec: dotted.replace('.', "/"),
                    kind,
                    line,
                    statement: stmt,
                    local_name: None,
                    raw_path: Some(dotted),
                });
            }
        } else if matches!(
            kind,
            "package_clause" | "object_definition" | "class_definition" | "trait_definition"
        ) {
            // Descend into bodies — Scala 3 allows nested imports.
            _walk_scala(&c, out);
        }
    }
}

// ---- Java ----

fn extract_java(src: &str) -> Vec<RawImport> {
    let lang = SupportLang::Java;
    let ast = lang.ast_grep(src.to_string());
    let root = ast.root();
    let mut out = Vec::new();
    for c in root.children() {
        if !c.is_named() {
            continue;
        }
        if c.kind() == "import_declaration" {
            let line = (c.start_pos().line() + 1) as u32;
            let stmt = c.text().into_owned();
            let body = stmt
                .trim_start_matches("import")
                .trim_end_matches(';')
                .trim();
            let is_static = body.starts_with("static ");
            let body = body.trim_start_matches("static ").trim();
            let is_glob = body.ends_with(".*");
            let dotted = body.trim_end_matches(".*").to_string();
            out.push(RawImport {
                spec: dotted.replace('.', "/"),
                kind: if is_static {
                    ImportKind::Static
                } else if is_glob {
                    ImportKind::Glob
                } else {
                    ImportKind::Bare
                },
                line,
                statement: stmt,
                local_name: None,
                raw_path: Some(dotted),
            });
        }
    }
    out
}

// ---- Kotlin ----

fn extract_kotlin(src: &str) -> Vec<RawImport> {
    let lang = SupportLang::Kotlin;
    let ast = lang.ast_grep(src.to_string());
    let root = ast.root();
    let mut out = Vec::new();
    _walk_kotlin(&root, &mut out);
    out
}

fn _walk_kotlin<'a, D: Doc>(node: &Node<'a, D>, out: &mut Vec<RawImport>) {
    for c in node.children() {
        if !c.is_named() {
            continue;
        }
        let kind = c.kind();
        let kind = kind.as_ref();
        if kind == "import_header" || kind == "import_directive" || kind == "import_list" {
            // Some Kotlin tree-sitter grammars name it differently;
            // handle either. The text form is `import x.y.Z [as W]`.
            if kind == "import_list" {
                _walk_kotlin(&c, out);
                continue;
            }
            let line = (c.start_pos().line() + 1) as u32;
            let stmt = c.text().into_owned();
            let body = stmt.trim_start_matches("import").trim();
            // Optional `as Quux` rename.
            let (path, alias) = match body.split_once(" as ") {
                Some((p, a)) => (p.trim().to_string(), Some(a.trim().to_string())),
                None => (body.to_string(), None),
            };
            let is_glob = path.ends_with(".*");
            let dotted = path.trim_end_matches(".*").to_string();
            out.push(RawImport {
                spec: dotted.replace('.', "/"),
                kind: if alias.is_some() {
                    ImportKind::Alias
                } else if is_glob {
                    ImportKind::Glob
                } else {
                    ImportKind::Bare
                },
                line,
                statement: stmt,
                local_name: alias,
                raw_path: Some(dotted),
            });
        }
    }
}

// ---- C# ----

fn extract_csharp(src: &str) -> Vec<RawImport> {
    let lang = SupportLang::CSharp;
    let ast = lang.ast_grep(src.to_string());
    let root = ast.root();
    let mut out = Vec::new();
    _walk_csharp(&root, &mut out);
    out
}

fn _walk_csharp<'a, D: Doc>(node: &Node<'a, D>, out: &mut Vec<RawImport>) {
    for c in node.children() {
        if !c.is_named() {
            continue;
        }
        let kind = c.kind();
        let kind = kind.as_ref();
        if kind == "using_directive" {
            let line = (c.start_pos().line() + 1) as u32;
            let stmt = c.text().into_owned();
            let body = stmt.trim_start_matches("using").trim_end_matches(';').trim().to_string();
            let is_static = body.starts_with("static ");
            let rest = body.trim_start_matches("static ").trim().to_string();

            // Alias form: `A = X.Y` (no `static`).
            if !is_static {
                if let Some((alias, target)) = rest.split_once('=') {
                    let dotted = target.trim().to_string();
                    out.push(RawImport {
                        spec: dotted.replace('.', "/"),
                        kind: ImportKind::Alias,
                        line,
                        statement: stmt,
                        local_name: Some(alias.trim().to_string()),
                        raw_path: Some(dotted),
                    });
                    continue;
                }
            }

            let dotted = rest.to_string();
            out.push(RawImport {
                spec: dotted.replace('.', "/"),
                kind: if is_static {
                    ImportKind::Static
                } else {
                    ImportKind::Bare
                },
                line,
                statement: stmt,
                local_name: None,
                raw_path: Some(dotted),
            });
        } else if matches!(kind, "namespace_declaration" | "file_scoped_namespace_declaration") {
            // Recurse into namespace bodies; usings can live inside.
            _walk_csharp(&c, out);
        }
    }
}

// ---- Go ----

fn extract_go(src: &str) -> Vec<RawImport> {
    let lang = SupportLang::Go;
    let ast = lang.ast_grep(src.to_string());
    let root = ast.root();
    let mut out = Vec::new();
    for c in root.children() {
        if !c.is_named() {
            continue;
        }
        if c.kind() == "import_declaration" {
            // Either single `import "foo"` or grouped `import (...)`.
            for spec in c.children() {
                if !spec.is_named() {
                    continue;
                }
                if spec.kind() == "import_spec" {
                    consume_go_spec(&spec, &mut out);
                } else if spec.kind() == "import_spec_list" {
                    for inner in spec.children() {
                        if inner.is_named() && inner.kind() == "import_spec" {
                            consume_go_spec(&inner, &mut out);
                        }
                    }
                }
            }
        }
    }
    out
}

fn consume_go_spec<'a, D: Doc>(node: &Node<'a, D>, out: &mut Vec<RawImport>) {
    let line = (node.start_pos().line() + 1) as u32;
    let stmt = node.text().into_owned();
    let path = node.field("path").map(|f| f.text().into_owned());
    let name = node.field("name").map(|f| f.text().into_owned());
    let Some(path) = path else { return };
    let stripped = path.trim_matches('"').to_string();
    out.push(RawImport {
        spec: stripped.clone(),
        kind: ImportKind::Bare,
        line,
        statement: stmt,
        local_name: name,
        raw_path: Some(stripped),
    });
}
