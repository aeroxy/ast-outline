use super::base::{collapse_ws, count_parse_errors, field_text, LanguageAdapter};
use crate::core::{Declaration, DeclarationKind, ParseResult};
use ast_grep_core::{Doc, Node};
use std::path::Path;

pub struct GoAdapter;

impl LanguageAdapter for GoAdapter {
    fn language_name(&self) -> &'static str {
        "go"
    }

    fn parse<'a, D: Doc>(&self, path: &Path, source: &[u8], root: Node<'a, D>) -> ParseResult {
        let mut decls = Vec::new();
        _walk_top(&root, source, &mut decls);
        ParseResult {
            path: path.to_path_buf(),
            language: self.language_name(),
            source: source.to_vec(),
            line_count: source.iter().filter(|&&b| b == b'\n').count() + 1,
            declarations: decls,
            error_count: count_parse_errors(root.clone()),
            imports: Vec::new(),
        }
    }
}

fn _walk_top<'a, D: Doc>(node: &Node<'a, D>, src: &[u8], out: &mut Vec<Declaration>) {
    let mut package_ns: Option<Declaration> = None;
    let mut type_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut pending_methods: Vec<(String, Declaration)> = Vec::new();

    for child in node.children() {
        if !child.is_named() {
            continue;
        }
        let kind = child.kind();

        if kind == "package_clause" {
            let ns = _package_to_decl(&child, src);
            out.push(ns);
            package_ns = Some(out.last().unwrap().clone());
            continue;
        }
        if kind == "import_declaration" || kind == "comment" {
            continue;
        }
        if kind == "type_declaration" {
            for d in _type_declaration_to_decls(&child, src) {
                if matches!(d.kind, DeclarationKind::Struct | DeclarationKind::Interface) {
                    if let Some(ns) = &mut package_ns {
                        ns.children.push(d.clone());
                        type_index.insert(d.name.clone(), ns.children.len() - 1);
                    } else {
                        out.push(d.clone());
                        type_index.insert(d.name.clone(), out.len() - 1);
                    }
                } else if let Some(ns) = &mut package_ns {
                    ns.children.push(d);
                } else {
                    out.push(d);
                }
            }
            continue;
        }
        if kind == "function_declaration" {
            let func = _function_to_decl(&child, src);
            if let Some(ns) = &mut package_ns {
                ns.children.push(func);
            } else {
                out.push(func);
            }
            continue;
        }
        if kind == "method_declaration" {
            let recv = _receiver_type_name(&child, src);
            let decl = _method_to_decl(&child, src);
            if let Some(r) = recv {
                pending_methods.push((r, decl));
            } else if let Some(ns) = &mut package_ns {
                ns.children.push(decl);
            } else {
                out.push(decl);
            }
            continue;
        }
        if kind == "const_declaration" {
            let decls = _const_var_to_decls(&child, src, "const");
            if let Some(ns) = &mut package_ns {
                ns.children.extend(decls);
            } else {
                out.extend(decls);
            }
            continue;
        }
        if kind == "var_declaration" {
            let decls = _const_var_to_decls(&child, src, "var");
            if let Some(ns) = &mut package_ns {
                ns.children.extend(decls);
            } else {
                out.extend(decls);
            }
            continue;
        }
    }

    // Attach methods
    for (recv, method) in pending_methods {
        if let Some(&idx) = type_index.get(&recv) {
            let target = if let Some(ref mut inner) = package_ns {
                &mut inner.children[idx]
            } else {
                &mut out[idx]
            };
            target.end_line = std::cmp::max(target.end_line, method.end_line);
            target.end_byte = std::cmp::max(target.end_byte, method.end_byte);
            target.children.push(method);
        } else if let Some(ns) = &mut package_ns {
            ns.children.push(method);
        } else {
            out.push(method);
        }
    }

    if let Some(mut ns) = package_ns {
        if let Some(last) = ns.children.last() {
            ns.end_line = std::cmp::max(ns.end_line, last.end_line);
            ns.end_byte = std::cmp::max(ns.end_byte, last.end_byte);
        }
        // replace the first element which is the original empty package_ns
        out[0] = ns;
    }
}

fn _package_to_decl<'a, D: Doc>(node: &Node<'a, D>, _src: &[u8]) -> Declaration {
    let name_node = node.children().find(|c| c.kind() == "package_identifier");
    let name = name_node.map(|n| n.text().into_owned()).unwrap_or_default();

    let signature = if name.is_empty() {
        "package".to_string()
    } else {
        format!("package {}", name)
    };
    let range = node.range();
    Declaration {
        kind: DeclarationKind::Namespace,
        name,
        signature,
        bases: Vec::new(),
        attrs: Vec::new(),
        docs: Vec::new(),
        docs_inside: false,
        visibility: String::new(),
        start_line: node.start_pos().line() + 1,
        end_line: node.end_pos().line() + 1,
        start_byte: range.start,
        end_byte: range.end,
        doc_start_byte: range.start,
        native_kind: None,
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    }
}

fn _type_declaration_to_decls<'a, D: Doc>(node: &Node<'a, D>, src: &[u8]) -> Vec<Declaration> {
    let mut out = Vec::new();
    let mut seen_first = false;

    for c in node.children() {
        if !c.is_named() {
            continue;
        }
        if c.kind() == "type_spec" {
            let anchor = if !seen_first {
                Some(node.clone())
            } else {
                None
            };
            if let Some(d) = _type_spec_to_decl(&c, src, anchor) {
                out.push(d);
                seen_first = true;
            }
        } else if c.kind() == "type_alias" {
            let anchor = if !seen_first {
                Some(node.clone())
            } else {
                None
            };
            if let Some(d) = _type_alias_to_decl(&c, src, anchor) {
                out.push(d);
                seen_first = true;
            }
        }
    }
    out
}

fn _type_spec_to_decl<'a, D: Doc>(
    node: &Node<'a, D>,
    src: &[u8],
    attach_outer_doc: Option<Node<'a, D>>,
) -> Option<Declaration> {
    let name = field_text(node, "name")?;
    let type_node = node.field("type")?;

    let docs_anchor = attach_outer_doc.unwrap_or_else(|| node.clone());
    let docs = _go_docs(&docs_anchor);
    let doc_start = _resolved_doc_start(&docs_anchor);
    let visibility = _go_visibility(&name);
    let range = node.range();

    if type_node.kind() == "struct_type" {
        let (children, bases) = _struct_members_and_bases(&type_node, src);
        let mut signature = _slice_until(
            node.range().start,
            &type_node,
            src,
            "field_declaration_list",
            node,
        );
        if !signature.starts_with("type ") {
            signature = format!("type {}", signature);
        }

        return Some(Declaration {
            kind: DeclarationKind::Struct,
            name,
            signature,
            bases,
            attrs: Vec::new(),
            docs,
            docs_inside: false,
            visibility,
            start_line: node.start_pos().line() + 1,
            end_line: node.end_pos().line() + 1,
            start_byte: range.start,
            end_byte: range.end,
            doc_start_byte: doc_start,
            native_kind: None,
            modifiers: Vec::new(),
            deprecated: false,
            children,
            calls: Vec::new(),
        });
    }

    if type_node.kind() == "interface_type" {
        let (children, bases) = _interface_members_and_bases(&type_node, src);
        let mut signature = _slice_until_brace(node.range().start, &type_node, src, node);
        if !signature.starts_with("type ") {
            signature = format!("type {}", signature);
        }

        return Some(Declaration {
            kind: DeclarationKind::Interface,
            name,
            signature,
            bases,
            attrs: Vec::new(),
            docs,
            docs_inside: false,
            visibility,
            start_line: node.start_pos().line() + 1,
            end_line: node.end_pos().line() + 1,
            start_byte: range.start,
            end_byte: range.end,
            doc_start_byte: doc_start,
            native_kind: None,
            modifiers: Vec::new(),
            deprecated: false,
            children,
            calls: Vec::new(),
        });
    }

    let mut bases = Vec::new();
    let base_text = collapse_ws(&type_node.text());
    if !base_text.is_empty() {
        bases.push(base_text);
    }

    let mut sig = collapse_ws(&node.text());
    if !sig.starts_with("type ") {
        sig = format!("type {}", sig);
    }

    Some(Declaration {
        kind: DeclarationKind::Delegate,
        name,
        signature: sig,
        bases,
        attrs: Vec::new(),
        docs,
        docs_inside: false,
        visibility,
        start_line: node.start_pos().line() + 1,
        end_line: node.end_pos().line() + 1,
        start_byte: range.start,
        end_byte: range.end,
        doc_start_byte: doc_start,
        native_kind: None,
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    })
}

fn _type_alias_to_decl<'a, D: Doc>(
    node: &Node<'a, D>,
    _src: &[u8],
    attach_outer_doc: Option<Node<'a, D>>,
) -> Option<Declaration> {
    let name = field_text(node, "name")?;
    let docs_anchor = attach_outer_doc.unwrap_or_else(|| node.clone());
    let docs = _go_docs(&docs_anchor);
    let doc_start = _resolved_doc_start(&docs_anchor);

    let mut sig = collapse_ws(&node.text());
    if !sig.starts_with("type ") {
        sig = format!("type {}", sig);
    }

    let range = node.range();
    Some(Declaration {
        kind: DeclarationKind::Delegate,
        name: name.clone(),
        signature: sig,
        bases: Vec::new(),
        attrs: Vec::new(),
        docs,
        docs_inside: false,
        visibility: _go_visibility(&name),
        start_line: node.start_pos().line() + 1,
        end_line: node.end_pos().line() + 1,
        start_byte: range.start,
        end_byte: range.end,
        doc_start_byte: doc_start,
        native_kind: None,
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    })
}

fn _struct_members_and_bases<'a, D: Doc>(
    struct_node: &Node<'a, D>,
    _src: &[u8],
) -> (Vec<Declaration>, Vec<String>) {
    let mut members = Vec::new();
    let mut bases = Vec::new();

    let body = struct_node
        .children()
        .find(|c| c.kind() == "field_declaration_list");
    if let Some(b) = body {
        for fd in b.children() {
            if fd.kind() != "field_declaration" {
                continue;
            }
            let mut ids = Vec::new();
            for c in fd.children() {
                if c.kind() == "field_identifier" {
                    ids.push(c.text().into_owned());
                }
            }
            if !ids.is_empty() {
                let first_name = ids[0].clone();
                let sig = collapse_ws(&fd.text());
                members.push(Declaration {
                    kind: DeclarationKind::Field,
                    name: first_name.clone(),
                    signature: sig,
                    bases: Vec::new(),
                    attrs: Vec::new(),
                    docs: Vec::new(),
                    docs_inside: false,
                    visibility: _go_visibility(&first_name),
                    start_line: fd.start_pos().line() + 1,
                    end_line: fd.end_pos().line() + 1,
                    start_byte: fd.range().start,
                    end_byte: fd.range().end,
                    doc_start_byte: fd.range().start,
                    native_kind: None,
                    modifiers: Vec::new(),
                    deprecated: false,
                    children: Vec::new(),
                    calls: Vec::new(),
                });
            } else if let Some(base) = _embedded_base_name(&fd) {
                bases.push(base);
            }
        }
    }
    (members, bases)
}

fn _interface_members_and_bases<'a, D: Doc>(
    iface_node: &Node<'a, D>,
    _src: &[u8],
) -> (Vec<Declaration>, Vec<String>) {
    let mut members = Vec::new();
    let mut bases = Vec::new();

    for c in iface_node.children() {
        if c.kind() == "method_elem" {
            if let Some(name) = field_text(&c, "name") {
                let sig = collapse_ws(&c.text());
                members.push(Declaration {
                    kind: DeclarationKind::Method,
                    name: name.clone(),
                    signature: sig,
                    bases: Vec::new(),
                    attrs: Vec::new(),
                    docs: Vec::new(),
                    docs_inside: false,
                    visibility: _go_visibility(&name),
                    start_line: c.start_pos().line() + 1,
                    end_line: c.end_pos().line() + 1,
                    start_byte: c.range().start,
                    end_byte: c.range().end,
                    doc_start_byte: c.range().start,
                    native_kind: None,
                    modifiers: Vec::new(),
                    deprecated: false,
                    children: Vec::new(),
                    calls: Vec::new(),
                });
            }
        } else if c.kind() == "type_elem" {
            for cc in c.children() {
                if cc.kind() == "type_identifier" {
                    bases.push(cc.text().into_owned());
                    break;
                }
            }
        }
    }
    (members, bases)
}

fn _embedded_base_name<'a, D: Doc>(fd: &Node<'a, D>) -> Option<String> {
    for c in fd.children() {
        if !c.is_named() {
            continue;
        }
        if let Some(name) = _drill_to_type_identifier(&c) {
            return Some(name);
        }
        if matches!(c.kind().as_ref(), "qualified_type" | "generic_type") {
            return Some(collapse_ws(&c.text()));
        }
    }
    None
}

fn _function_to_decl<'a, D: Doc>(node: &Node<'a, D>, src: &[u8]) -> Declaration {
    let name = field_text(node, "name").unwrap_or_else(|| "?".to_string());
    let docs = _go_docs(node);
    let body = node.field("body");
    let end = body.map(|b| b.range().start).unwrap_or(node.range().end);
    let sig = collapse_ws(&String::from_utf8_lossy(&src[node.range().start..end]))
        .trim_end_matches('{')
        .trim()
        .to_string();

    let range = node.range();
    Declaration {
        kind: DeclarationKind::Function,
        name: name.clone(),
        signature: sig,
        bases: Vec::new(),
        attrs: Vec::new(),
        docs,
        docs_inside: false,
        visibility: _go_visibility(&name),
        start_line: node.start_pos().line() + 1,
        end_line: node.end_pos().line() + 1,
        start_byte: range.start,
        end_byte: range.end,
        doc_start_byte: _resolved_doc_start(node),
        native_kind: None,
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    }
}

fn _method_to_decl<'a, D: Doc>(node: &Node<'a, D>, src: &[u8]) -> Declaration {
    let name = field_text(node, "name").unwrap_or_else(|| "?".to_string());
    let docs = _go_docs(node);
    let body = node.field("body");
    let end = body.map(|b| b.range().start).unwrap_or(node.range().end);
    let sig = collapse_ws(&String::from_utf8_lossy(&src[node.range().start..end]))
        .trim_end_matches('{')
        .trim()
        .to_string();

    let range = node.range();
    Declaration {
        kind: DeclarationKind::Method,
        name: name.clone(),
        signature: sig,
        bases: Vec::new(),
        attrs: Vec::new(),
        docs,
        docs_inside: false,
        visibility: _go_visibility(&name),
        start_line: node.start_pos().line() + 1,
        end_line: node.end_pos().line() + 1,
        start_byte: range.start,
        end_byte: range.end,
        doc_start_byte: _resolved_doc_start(node),
        native_kind: None,
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    }
}

fn _receiver_type_name<'a, D: Doc>(method: &Node<'a, D>, _src: &[u8]) -> Option<String> {
    let mut rcv = method.field("receiver");
    if rcv.is_none() {
        rcv = method.children().find(|c| c.kind() == "parameter_list");
    }
    let rcv = rcv?;

    for param in rcv.children() {
        if param.kind() != "parameter_declaration" {
            continue;
        }
        for c in param.children() {
            if let Some(name) = _drill_to_type_identifier(&c) {
                return Some(name);
            }
        }
    }
    None
}

fn _drill_to_type_identifier<'a, D: Doc>(node: &Node<'a, D>) -> Option<String> {
    let kind = node.kind();
    if kind == "type_identifier" {
        return Some(node.text().into_owned());
    }
    if kind == "pointer_type" {
        for c in node.children() {
            if !c.is_named() {
                continue;
            }
            if let Some(r) = _drill_to_type_identifier(&c) {
                return Some(r);
            }
        }
    }
    if kind == "generic_type" {
        for c in node.children() {
            if c.kind() == "type_identifier" {
                return Some(c.text().into_owned());
            }
        }
    }
    if kind == "qualified_type" {
        let mut last_id = None;
        for c in node.children() {
            if c.kind() == "type_identifier" {
                last_id = Some(c.text().into_owned());
            }
        }
        if last_id.is_some() {
            return last_id;
        }
    }
    None
}

fn _const_var_to_decls<'a, D: Doc>(
    node: &Node<'a, D>,
    src: &[u8],
    kind_name: &str,
) -> Vec<Declaration> {
    let mut out = Vec::new();
    let mut seen_first = false;

    for c in node.children() {
        if !c.is_named() {
            continue;
        }
        let k = c.kind();
        if k == "const_spec" || k == "var_spec" {
            let anchor = if !seen_first {
                Some(node.clone())
            } else {
                None
            };
            if let Some(d) = _spec_to_field(&c, src, kind_name, anchor) {
                out.push(d);
                seen_first = true;
            }
        } else if k == "var_spec_list" {
            for spec in c.children() {
                if spec.kind() == "var_spec" {
                    let anchor = if !seen_first {
                        Some(node.clone())
                    } else {
                        None
                    };
                    if let Some(d) = _spec_to_field(&spec, src, kind_name, anchor) {
                        out.push(d);
                        seen_first = true;
                    }
                }
            }
        }
    }
    out
}

fn _spec_to_field<'a, D: Doc>(
    node: &Node<'a, D>,
    _src: &[u8],
    kind_name: &str,
    outer_doc_anchor: Option<Node<'a, D>>,
) -> Option<Declaration> {
    let name_node = node
        .field("name")
        .or_else(|| node.children().find(|c| c.kind() == "identifier"))?;
    let name = name_node.text().into_owned();

    let mut docs = _go_docs(node);
    if docs.is_empty() {
        if let Some(ref inner) = outer_doc_anchor {
            docs = _go_docs(inner);
        }
    }

    let mut doc_start = _leading_doc_start_byte(node);
    if doc_start.is_none() {
        if let Some(ref inner) = outer_doc_anchor {
            doc_start = _leading_doc_start_byte(inner);
        }
    }
    let doc_start = doc_start.unwrap_or(node.range().start);

    let mut sig_text = collapse_ws(&node.text());
    if !sig_text.starts_with(&format!("{} ", kind_name)) && !sig_text.starts_with(kind_name) {
        sig_text = format!("{} {}", kind_name, sig_text);
    }

    let range = node.range();
    Some(Declaration {
        kind: DeclarationKind::Field,
        name: name.clone(),
        signature: sig_text.trim_end().to_string(),
        bases: Vec::new(),
        attrs: Vec::new(),
        docs,
        docs_inside: false,
        visibility: _go_visibility(&name),
        start_line: node.start_pos().line() + 1,
        end_line: node.end_pos().line() + 1,
        start_byte: range.start,
        end_byte: range.end,
        doc_start_byte: doc_start,
        native_kind: None,
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    })
}

fn _go_visibility(name: &str) -> String {
    if name.is_empty() {
        return "public".to_string();
    }
    let first = name.chars().next().unwrap();
    if first.is_uppercase() {
        "public".to_string()
    } else {
        "private".to_string()
    }
}

fn _go_docs<'a, D: Doc>(node: &Node<'a, D>) -> Vec<String> {
    let mut docs = Vec::new();
    let mut sib = node.prev();
    let mut last_start_line = Some(node.start_pos().line());

    while let Some(s) = sib {
        if s.kind() == "comment" {
            if let Some(lsl) = last_start_line {
                if s.end_pos().line() + 1 < lsl {
                    break;
                }
            }
            docs.push(s.text().into_owned());
            last_start_line = Some(s.start_pos().line());
            sib = s.prev();
        } else {
            break;
        }
    }
    docs.reverse();
    docs
}

fn _leading_doc_start_byte<'a, D: Doc>(node: &Node<'a, D>) -> Option<usize> {
    let mut first = None;
    let mut sib = node.prev();
    let mut last_start_line = Some(node.start_pos().line());

    while let Some(s) = sib {
        if s.kind() == "comment" {
            if let Some(lsl) = last_start_line {
                if s.end_pos().line() + 1 < lsl {
                    break;
                }
            }
            first = Some(s.clone());
            last_start_line = Some(s.start_pos().line());
            sib = s.prev();
        } else {
            break;
        }
    }
    first.map(|f| f.range().start)
}

fn _resolved_doc_start<'a, D: Doc>(node: &Node<'a, D>) -> usize {
    _leading_doc_start_byte(node).unwrap_or(node.range().start)
}

fn _slice_until<'a, D: Doc>(
    start_byte: usize,
    type_node: &Node<'a, D>,
    src: &[u8],
    body_node_type: &str,
    default_to_node: &Node<'a, D>,
) -> String {
    let mut cut = None;
    for c in type_node.children() {
        if c.kind() == body_node_type {
            cut = Some(c.range().start);
            break;
        }
    }
    let end = cut.unwrap_or(default_to_node.range().end);
    collapse_ws(&String::from_utf8_lossy(&src[start_byte..end]))
        .trim_end_matches('{')
        .trim()
        .to_string()
}

fn _slice_until_brace<'a, D: Doc>(
    start_byte: usize,
    type_node: &Node<'a, D>,
    src: &[u8],
    default_to_node: &Node<'a, D>,
) -> String {
    let mut cut = None;
    for c in type_node.children() {
        if c.kind() == "{" {
            cut = Some(c.range().start);
            break;
        }
    }
    let end = cut.unwrap_or(default_to_node.range().end);
    collapse_ws(&String::from_utf8_lossy(&src[start_byte..end]))
        .trim_end_matches('{')
        .trim()
        .to_string()
}
