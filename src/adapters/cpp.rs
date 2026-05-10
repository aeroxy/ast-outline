use super::base::{collapse_ws, count_parse_errors, field_text, LanguageAdapter};
use crate::core::{Declaration, DeclarationKind, ParseResult};
use ast_grep_core::{Doc, Node};
use std::path::Path;

pub struct CppAdapter;

impl LanguageAdapter for CppAdapter {
    fn language_name(&self) -> &'static str {
        "cpp"
    }

    fn parse<'a, D: Doc>(&self, path: &Path, source: &[u8], root: Node<'a, D>) -> ParseResult {
        let mut decls = Vec::new();
        _walk_namespace(&root, source, &mut decls);
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

fn _walk_namespace<'a, D: Doc>(node: &Node<'a, D>, src: &[u8], out: &mut Vec<Declaration>) {
    for child in node.children() {
        if !child.is_named() {
            continue;
        }
        let kind = child.kind();
        if kind == "namespace_definition" {
            if let Some(decl) = _namespace_to_decl(&child, src) {
                out.push(decl);
            }
        } else if kind == "class_specifier" || kind == "struct_specifier" {
            if let Some(decl) = _class_to_decl(&child, src) {
                out.push(decl);
            }
        } else if kind == "function_definition" {
            if let Some(decl) = _function_to_decl(&child, src, false) {
                out.push(decl);
            }
        } else if kind == "template_declaration" {
            if let Some(decl) = _template_to_decl(&child, src) {
                out.push(decl);
            }
        } else if kind == "enum_specifier" {
            if let Some(decl) = _enum_to_decl(&child, src) {
                out.push(decl);
            }
        }
    }
}

fn _namespace_to_decl<'a, D: Doc>(node: &Node<'a, D>, src: &[u8]) -> Option<Declaration> {
    let name = field_text(node, "name").unwrap_or_else(|| "?".to_string());
    let body = node.field("body")?;
    let mut children = Vec::new();
    for child in body.children() {
        if !child.is_named() {
            continue;
        }
        let kind = child.kind();
        if kind == "class_specifier" || kind == "struct_specifier" {
            if let Some(decl) = _class_to_decl(&child, src) {
                children.push(decl);
            }
        } else if kind == "function_definition" {
            if let Some(decl) = _function_to_decl(&child, src, false) {
                children.push(decl);
            }
        } else if kind == "template_declaration" {
            if let Some(decl) = _template_to_decl(&child, src) {
                children.push(decl);
            }
        } else if kind == "enum_specifier" {
            if let Some(decl) = _enum_to_decl(&child, src) {
                children.push(decl);
            }
        }
    }

    let sig = format!("namespace {}", name);
    let range = node.range();
    Some(Declaration {
        kind: DeclarationKind::Namespace,
        name: name.clone(),
        signature: sig,
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
        native_kind: Some("namespace".to_string()),
        modifiers: Vec::new(),
        deprecated: false,
        children,
        calls: Vec::new(),
    })
}

fn _class_to_decl<'a, D: Doc>(node: &Node<'a, D>, src: &[u8]) -> Option<Declaration> {
    let name = field_text(node, "name").unwrap_or_else(|| "?".to_string());
    let kind_str = if node.kind() == "struct_specifier" {
        "struct"
    } else {
        "class"
    };

    let bases = _class_bases(node);
    let body = node.field("body");
    let mut children = Vec::new();

    if let Some(b) = body {
        for child in b.children() {
            if !child.is_named() {
                continue;
            }
            let kind = child.kind();
            if kind == "function_definition" {
                if let Some(decl) = _function_to_decl(&child, src, true) {
                    children.push(decl);
                }
            } else if kind == "field_declaration" {
                // tree-sitter-cpp uses `field_declaration` for both data
                // members and method declarations. A function-typed declarator
                // means it's a method.
                let is_method = child
                    .field("declarator")
                    .is_some_and(|d| d.kind() == "function_declarator");
                if is_method {
                    if let Some(decl) = _method_decl_to_decl(&child, src) {
                        children.push(decl);
                    }
                } else if let Some(decl) = _field_to_decl(&child, src) {
                    children.push(decl);
                }
            } else if kind == "declaration" {
                // Constructors and destructors have no return type and parse
                // as `declaration` rather than `field_declaration`.
                if child
                    .field("declarator")
                    .is_some_and(|d| d.kind() == "function_declarator")
                {
                    if let Some(decl) = _ctor_decl_to_decl(&child, src) {
                        children.push(decl);
                    }
                }
            }
        }
    }

    let mut sig = format!("{} {}", kind_str, name);
    if !bases.is_empty() {
        sig.push_str(" : ");
        sig.push_str(&bases.join(", "));
    }

    let range = node.range();
    Some(Declaration {
        kind: DeclarationKind::Class,
        name: name.clone(),
        signature: sig,
        bases,
        attrs: Vec::new(),
        docs: Vec::new(),
        docs_inside: false,
        visibility: String::new(),
        start_line: node.start_pos().line() + 1,
        end_line: node.end_pos().line() + 1,
        start_byte: range.start,
        end_byte: range.end,
        doc_start_byte: range.start,
        native_kind: Some(kind_str.to_string()),
        modifiers: Vec::new(),
        deprecated: false,
        children,
        calls: Vec::new(),
    })
}

/// Build a constructor/destructor `Declaration` from a `declaration` whose
/// declarator is a `function_declarator` (no return type).
fn _ctor_decl_to_decl<'a, D: Doc>(node: &Node<'a, D>, _src: &[u8]) -> Option<Declaration> {
    let declarator = node.field("declarator")?;
    let inner = declarator.field("declarator")?;
    let raw_name = collapse_ws(&inner.text()).trim().to_string();

    let mut params = Vec::new();
    if let Some(pl) = declarator.field("parameters") {
        for child in pl.children() {
            if child.is_named() && child.kind() == "parameter_declaration" {
                let text = collapse_ws(&child.text());
                if !text.is_empty() {
                    params.push(text);
                }
            }
        }
    }

    let kind = if raw_name.starts_with('~') {
        DeclarationKind::Destructor
    } else {
        DeclarationKind::Constructor
    };

    let sig = format!("{}({})", raw_name, params.join(", "));
    let range = node.range();
    Some(Declaration {
        kind,
        name: raw_name,
        signature: sig,
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
        native_kind: Some("ctor".to_string()),
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    })
}

/// Build a method `Declaration` from a `field_declaration` whose declarator
/// is a `function_declarator` (i.e. method declaration without a body).
fn _method_decl_to_decl<'a, D: Doc>(node: &Node<'a, D>, _src: &[u8]) -> Option<Declaration> {
    let declarator = node.field("declarator")?;
    // The function_declarator's own `declarator` field carries the method name.
    let inner = declarator.field("declarator")?;
    let raw_name = collapse_ws(&inner.text()).trim().to_string();
    let return_type = node
        .field("type")
        .map(|t| collapse_ws(&t.text()).trim().to_string());

    let mut params = Vec::new();
    if let Some(pl) = declarator.field("parameters") {
        for child in pl.children() {
            if child.is_named() && child.kind() == "parameter_declaration" {
                let text = collapse_ws(&child.text());
                if !text.is_empty() {
                    params.push(text);
                }
            }
        }
    }

    let kind = if raw_name.starts_with('~') {
        DeclarationKind::Destructor
    } else if raw_name.contains("operator") {
        DeclarationKind::Operator
    } else {
        DeclarationKind::Method
    };

    let mut sig = String::new();
    if let Some(rt) = return_type {
        if !rt.is_empty() {
            sig.push_str(&rt);
            sig.push(' ');
        }
    }
    sig.push_str(&raw_name);
    sig.push('(');
    sig.push_str(&params.join(", "));
    sig.push(')');

    let range = node.range();
    Some(Declaration {
        kind,
        name: raw_name,
        signature: sig,
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
        native_kind: Some("method".to_string()),
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    })
}

fn _function_to_decl<'a, D: Doc>(
    node: &Node<'a, D>,
    src: &[u8],
    inside_class: bool,
) -> Option<Declaration> {
    let name = field_text(node, "declarator")
        .or_else(|| field_text(node, "name"))
        .unwrap_or_else(|| "?".to_string());

    let return_type = node
        .field("type")
        .map(|t| collapse_ws(&t.text()).trim().to_string());

    let params = _function_params(node, src);

    let kind = if inside_class {
        if name.contains("operator") {
            DeclarationKind::Operator
        } else if name.starts_with('~') {
            DeclarationKind::Constructor
        } else {
            DeclarationKind::Method
        }
    } else {
        DeclarationKind::Function
    };

    let mut sig = String::new();
    if let Some(rt) = return_type {
        sig.push_str(&rt);
        sig.push(' ');
    }
    sig.push_str(&name);
    sig.push('(');
    sig.push_str(&params.join(", "));
    sig.push(')');

    let range = node.range();
    Some(Declaration {
        kind,
        name: name.clone(),
        signature: sig,
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
        native_kind: Some(if inside_class {
            "method"
        } else {
            "function"
        }
        .to_string()),
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    })
}

fn _template_to_decl<'a, D: Doc>(node: &Node<'a, D>, src: &[u8]) -> Option<Declaration> {
    for child in node.children() {
        if !child.is_named() {
            continue;
        }
        let kind = child.kind();
        if kind == "class_specifier" || kind == "struct_specifier" {
            return _class_to_decl(&child, src);
        } else if kind == "function_definition" {
            return _function_to_decl(&child, src, false);
        }
    }
    None
}

fn _enum_to_decl<'a, D: Doc>(node: &Node<'a, D>, _src: &[u8]) -> Option<Declaration> {
    let name = field_text(node, "name").unwrap_or_else(|| "?".to_string());
    let sig = format!("enum {}", name);
    let range = node.range();
    Some(Declaration {
        kind: DeclarationKind::Enum,
        name: name.clone(),
        signature: sig,
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
        native_kind: Some("enum".to_string()),
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    })
}

fn _field_to_decl<'a, D: Doc>(node: &Node<'a, D>, _src: &[u8]) -> Option<Declaration> {
    let decl = node.field("declarator")?;
    let name = collapse_ws(&decl.text()).trim().to_string();
    let type_str = node
        .field("type")
        .map(|t| collapse_ws(&t.text()).trim().to_string());

    let sig = if let Some(t) = type_str {
        format!("{} {}", t, name)
    } else {
        name.clone()
    };

    let range = node.range();
    Some(Declaration {
        kind: DeclarationKind::Field,
        name,
        signature: sig,
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
        native_kind: Some("field".to_string()),
        modifiers: Vec::new(),
        deprecated: false,
        children: Vec::new(),
        calls: Vec::new(),
    })
}

fn _class_bases<'a, D: Doc>(node: &Node<'a, D>) -> Vec<String> {
    let mut bases = Vec::new();
    let base_clause = node.field("base_class_clause");
    if let Some(bc) = base_clause {
        for child in bc.children() {
            if child.is_named() && child.kind() != "::" {
                let text = collapse_ws(&child.text());
                if !text.is_empty() {
                    bases.push(text);
                }
            }
        }
    }
    bases
}

fn _function_params<'a, D: Doc>(node: &Node<'a, D>, _src: &[u8]) -> Vec<String> {
    let mut params = Vec::new();
    let param_list = node.field("parameters");
    if let Some(pl) = param_list {
        for child in pl.children() {
            if child.is_named() && child.kind() == "parameter_declaration" {
                let text = collapse_ws(&child.text());
                if !text.is_empty() {
                    params.push(text);
                }
            }
        }
    }
    params
}
