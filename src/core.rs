use colored::Colorize;
use serde::{Serialize, Serializer};
use std::path::PathBuf;

// Q. T. Felix - start
// Stable JSON schema identifires — bump on breaking changes
pub const JSON_SCHEMA_OUTLINE: &str = "ast-outline.outline.v1";
pub const JSON_SCHEMA_SHOW: &str = "ast-outline.show.v1";
pub const JSON_SCHEMA_IMPLEMENTS: &str = "ast-outline.implements.v1";
// Q. T. Felix - end

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub enum DeclarationKind {
    Namespace,
    Class,
    Struct,
    Interface,
    Record,
    Enum,
    EnumMember,
    Method,
    Function,
    Constructor,
    Destructor,
    Property,
    Indexer,
    Field,
    Event,
    Delegate,
    Operator,
    Heading,
    CodeBlock,
}

impl DeclarationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Interface => "interface",
            Self::Record => "record",
            Self::Enum => "enum",
            Self::EnumMember => "enum_member",
            Self::Method => "method",
            Self::Function => "function",
            Self::Constructor => "ctor",
            Self::Destructor => "dtor",
            Self::Property => "property",
            Self::Indexer => "indexer",
            Self::Field => "field",
            Self::Event => "event",
            Self::Delegate => "delegate",
            Self::Operator => "operator",
            Self::Heading => "heading",
            Self::CodeBlock => "code_block",
        }
    }
}

impl std::fmt::Display for DeclarationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// Q. T. Felix - start
impl Serialize for DeclarationKind {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}
// Q. T. Felix - end

#[derive(Debug, Clone, Serialize)]
pub struct Declaration {
    pub kind: DeclarationKind,
    pub name: String,
    pub signature: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub docs: Vec<String>,
    pub docs_inside: bool,
    pub visibility: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub doc_start_byte: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Declaration>,
}

impl Declaration {
    pub fn lines_suffix(&self) -> String {
        if self.start_line == 0 {
            String::new()
        } else if self.start_line == self.end_line {
            format!("  L{}", self.start_line)
                .truecolor(150, 150, 150)
                .to_string()
        } else {
            format!("  L{}-{}", self.start_line, self.end_line)
                .truecolor(150, 150, 150)
                .to_string()
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ParseResult {
    #[serde(serialize_with = "_serialize_path")]
    pub path: PathBuf,
    pub language: &'static str,
    #[serde(skip)]
    pub source: Vec<u8>,
    pub line_count: usize,
    pub error_count: usize,
    pub declarations: Vec<Declaration>,
}

// Q. T. Felix - start
fn _serialize_path<S: Serializer>(p: &PathBuf, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(&p.to_string_lossy())
}
// Q. T. Felix - end

#[derive(Debug, Clone)]
pub struct OutlineOptions {
    pub include_private: bool,
    pub include_fields: bool,
    pub include_xml_doc: bool,
    pub include_attributes: bool,
    pub include_line_numbers: bool,
    pub max_doc_lines: usize,
}

impl Default for OutlineOptions {
    fn default() -> Self {
        Self {
            include_private: true,
            include_fields: true,
            include_xml_doc: true,
            include_attributes: true,
            include_line_numbers: true,
            max_doc_lines: 6,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DigestOptions {
    pub include_private: bool,
    pub include_fields: bool,
    pub max_members_per_type: usize,
    pub max_heading_depth: usize,
}

impl Default for DigestOptions {
    fn default() -> Self {
        Self {
            include_private: false,
            include_fields: false,
            max_members_per_type: 50,
            max_heading_depth: 3,
        }
    }
}

// --- Renderers ---

pub fn render_outline(result: &ParseResult, opts: &OutlineOptions) -> String {
    let mut lines = vec![_format_file_header(
        &format!("# {}", result.path.display()),
        result,
    )];
    if let Some(warn) = _format_error_warning(result) {
        lines.push(warn);
    }
    for decl in &result.declarations {
        _render_decl(decl, opts, 0, &mut lines);
    }
    lines.join("\n")
}
fn _format_file_header(prefix: &str, result: &ParseResult) -> String {
    let counts = _collect_counts(&result.declarations);
    let mut parts = vec![format!("{} lines", result.line_count)];

    if result.language == "markdown" {
        let order = [("headings", "headings"), ("code_blocks", "code blocks")];
        for (key, label) in order {
            let n = counts.get(key).copied().unwrap_or(0);
            if n > 0 {
                parts.push(format!("{} {}", n, label));
            }
        }
    } else {
        let order = [
            ("types", "types"),
            ("methods", "methods"),
            ("fields", "fields"),
        ];
        for (key, label) in order {
            let n = counts.get(key).copied().unwrap_or(0);
            if n > 0 {
                parts.push(format!("{} {}", n, label));
            }
        }
    }
    format!("{} ({})", prefix.blue().bold(), parts.join(", ").dimmed())
}

fn _format_error_warning(result: &ParseResult) -> Option<String> {
    if result.error_count == 0 {
        return None;
    }
    let plural = if result.error_count != 1 { "s" } else { "" };
    Some(
        format!(
            "# WARNING: {} parse error{} — output may be incomplete",
            result.error_count, plural
        )
        .red()
        .to_string(),
    )
}

fn _collect_counts(decls: &[Declaration]) -> std::collections::HashMap<&'static str, usize> {
    use DeclarationKind::*;
    let mut out = std::collections::HashMap::new();
    out.insert("types", 0);
    out.insert("methods", 0);
    out.insert("fields", 0);
    out.insert("headings", 0);
    out.insert("code_blocks", 0);

    let mut stack: Vec<&Declaration> = decls.iter().collect();
    while let Some(d) = stack.pop() {
        let k = &d.kind;
        match k {
            Class | Struct | Interface | Record | Enum => *out.get_mut("types").unwrap() += 1,
            Method | Function | Constructor | Destructor | Operator => {
                *out.get_mut("methods").unwrap() += 1
            }
            Field | Property | Event | Indexer => *out.get_mut("fields").unwrap() += 1,
            Heading => *out.get_mut("headings").unwrap() += 1,
            CodeBlock => *out.get_mut("code_blocks").unwrap() += 1,
            _ => {}
        }
        for child in &d.children {
            stack.push(child);
        }
    }
    out
}

fn _render_decl(decl: &Declaration, opts: &OutlineOptions, indent: usize, out: &mut Vec<String>) {
    use DeclarationKind::*;

    let is_field = matches!(decl.kind, Field | Property | Event | Indexer);
    if is_field && !opts.include_fields {
        return;
    }
    if decl.visibility == "private" && !opts.include_private {
        return;
    }

    let prefix = "    ".repeat(indent);

    if opts.include_xml_doc && !decl.docs.is_empty() && !decl.docs_inside {
        for d in _clip_docs(&decl.docs, opts.max_doc_lines) {
            out.push(format!("{}{}", prefix, d));
        }
    }

    let attrs_prefix = if opts.include_attributes && !decl.attrs.is_empty() {
        format!("{} ", decl.attrs.join(" "))
    } else {
        String::new()
    };

    let suffix = if opts.include_line_numbers {
        decl.lines_suffix()
    } else {
        String::new()
    };

    if decl.kind == Namespace {
        out.push(format!(
            "{}namespace {}",
            prefix,
            decl.name.magenta().bold()
        ));
    } else {
        out.push(format!(
            "{}{}{}{}",
            prefix, attrs_prefix, decl.signature, suffix
        ));
    }

    if opts.include_xml_doc && !decl.docs.is_empty() && decl.docs_inside {
        let inner_prefix = "    ".repeat(indent + 1);
        for d in _clip_docs(&decl.docs, opts.max_doc_lines) {
            out.push(format!("{}{}", inner_prefix, d));
        }
    }

    for child in &decl.children {
        _render_decl(child, opts, indent + 1, out);
    }

    if indent == 0
        || matches!(
            decl.kind,
            Class | Struct | Interface | Record | Enum | Namespace
        )
    {
        out.push(String::new());
    }
}

fn _clip_docs(docs: &[String], limit: usize) -> Vec<String> {
    if docs.len() <= limit {
        docs.to_vec()
    } else {
        let mut clipped = docs[..limit].to_vec();
        clipped.push("...".to_string());
        clipped
    }
}

pub fn render_digest(
    results: &[ParseResult],
    opts: &DigestOptions,
    root: Option<&std::path::Path>,
) -> String {
    if results.is_empty() {
        return "# no files\n".to_string();
    }

    let default_root = results[0].path.parent().unwrap_or(std::path::Path::new(""));
    let root = root.unwrap_or(default_root);

    let mut grouped: std::collections::BTreeMap<&std::path::Path, Vec<&ParseResult>> =
        std::collections::BTreeMap::new();
    for r in results {
        let parent = r.path.parent().unwrap_or(std::path::Path::new(""));
        grouped.entry(parent).or_default().push(r);
    }

    let mut lines = Vec::new();
    for (dir, res) in grouped {
        let rel = dir.strip_prefix(root).unwrap_or(dir);
        lines.push(format!("{}/", rel.display().to_string().cyan().bold()));
        for r in res {
            lines.extend(_digest_one(r, opts));
        }
        lines.push(String::new());
    }

    let mut out = lines.join("\n");
    out = out.trim_end().to_string();
    out.push('\n');
    out
}

fn _digest_one(result: &ParseResult, opts: &DigestOptions) -> Vec<String> {
    let name = result
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let mut lines = vec![_format_file_header(&format!("  {}", name), result)];
    if let Some(warn) = _format_error_warning(result) {
        lines.push(format!("  {}", warn));
    }

    if result.language == "markdown" {
        let toc = _digest_markdown(&result.declarations, opts, 4, 1);
        if toc.is_empty() {
            if let Some(last) = lines.last_mut() {
                last.push_str("  # empty");
            }
            return lines;
        }
        lines.extend(toc);
        return lines;
    }

    let types = _flatten_types(&result.declarations, "");
    let free_functions = _flatten_free_functions(&result.declarations, opts);

    if types.is_empty() && free_functions.is_empty() {
        if let Some(last) = lines.last_mut() {
            last.push_str("  # no declarations");
        }
        return lines;
    }

    for t in types {
        let mut header = format!(
            "    {} {}",
            t.kind.to_string().italic(),
            t.name.green().bold()
        );
        if !t.bases.is_empty() {
            header.push_str(" : ");
            header.push_str(&t.bases.join(", "));
        }
        header.push_str(&t.lines_suffix());
        lines.push(header);

        let members = _digest_members(&t, opts);
        if !members.is_empty() {
            let shown = &members[..std::cmp::min(members.len(), opts.max_members_per_type)];
            let mut tokens = Vec::new();
            for m in shown {
                if matches!(
                    m.kind,
                    DeclarationKind::Method
                        | DeclarationKind::Function
                        | DeclarationKind::Constructor
                        | DeclarationKind::Destructor
                ) {
                    tokens.push(format!("+{}", m.name.yellow()));
                } else {
                    tokens.push(format!(
                        "+{} [{}]",
                        m.name.yellow(),
                        m.kind.to_string().dimmed()
                    ));
                }
            }
            lines.extend(_wrap_tokens(&tokens, 100, "      "));
            if members.len() > shown.len() {
                lines.push(
                    format!("      ... +{} more", members.len() - shown.len())
                        .dimmed()
                        .to_string(),
                );
            }
        }
    }

    if !free_functions.is_empty() {
        let shown =
            &free_functions[..std::cmp::min(free_functions.len(), opts.max_members_per_type)];
        let mut tokens = Vec::new();
        for f in shown {
            if matches!(f.kind, DeclarationKind::Function | DeclarationKind::Method) {
                tokens.push(format!("+{}", f.name.yellow()));
            } else {
                tokens.push(format!(
                    "+{} [{}]",
                    f.name.yellow(),
                    f.kind.to_string().dimmed()
                ));
            }
        }
        lines.extend(_wrap_tokens(&tokens, 100, "    "));
    }

    lines
}

fn _flatten_types(decls: &[Declaration], prefix: &str) -> Vec<Declaration> {
    use DeclarationKind::*;
    let mut out = Vec::new();
    for d in decls {
        if d.kind == Namespace {
            let new_prefix = if prefix.is_empty() {
                format!("{}.", d.name)
            } else {
                format!("{}{}.", prefix, d.name)
            };
            out.extend(_flatten_types(&d.children, &new_prefix));
        } else if matches!(d.kind, Class | Struct | Interface | Record | Enum) {
            let mut qualified = d.clone();
            if !prefix.is_empty() {
                qualified.name = format!("{}{}", prefix, d.name);
            }
            let new_prefix = format!("{}.", qualified.name);
            out.push(qualified);
            out.extend(_flatten_types(&d.children, &new_prefix));
        }
    }
    out
}

fn _flatten_free_functions<'a>(
    decls: &'a [Declaration],
    opts: &DigestOptions,
) -> Vec<&'a Declaration> {
    use DeclarationKind::*;
    let mut out = Vec::new();
    for d in decls {
        if d.kind == Namespace {
            out.extend(_flatten_free_functions(&d.children, opts));
        } else if matches!(d.kind, Class | Struct | Interface | Record | Enum) {
            continue;
        } else {
            if d.kind == Field && !opts.include_fields {
                continue;
            }
            if d.visibility == "private" && !opts.include_private {
                continue;
            }
            out.push(d);
        }
    }
    out
}

fn _digest_members<'a>(type_decl: &'a Declaration, opts: &DigestOptions) -> Vec<&'a Declaration> {
    use DeclarationKind::*;
    let mut members = Vec::new();
    for c in &type_decl.children {
        if matches!(
            c.kind,
            Class | Struct | Interface | Record | Enum | Namespace | EnumMember
        ) {
            continue;
        }
        if c.kind == Field && !opts.include_fields {
            continue;
        }
        if c.visibility == "private" && !opts.include_private {
            continue;
        }
        members.push(c);
    }
    members
}

fn _wrap_tokens(tokens: &[String], width: usize, indent: &str) -> Vec<String> {
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = indent.to_string();
    for tok in tokens {
        let piece = if cur == indent {
            tok.clone()
        } else {
            format!("  {}", tok)
        };
        if cur.len() + piece.len() > width && cur != indent {
            out.push(cur);
            cur = format!("{}{}", indent, tok);
        } else {
            cur.push_str(&piece);
        }
    }
    if cur != indent {
        out.push(cur);
    }
    out
}

#[derive(Debug, Serialize)]
pub struct SymbolMatch {
    pub qualified_name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ancestor_signatures: Vec<String>,
}

pub fn find_symbols(result: &ParseResult, symbol: &str) -> Vec<SymbolMatch> {
    let parts: Vec<&str> = symbol.split('.').collect();
    let mut matches = Vec::new();
    _search_walk(
        &result.declarations,
        &result.source,
        Vec::new(),
        Vec::new(),
        &parts,
        &mut matches,
    );
    matches
}

fn _search_walk(
    decls: &[Declaration],
    src: &[u8],
    trail: Vec<String>,
    ancestors: Vec<&Declaration>,
    parts: &[&str],
    out: &mut Vec<SymbolMatch>,
) {
    for d in decls {
        let mut new_trail = trail.clone();
        if !d.name.is_empty() {
            new_trail.push(d.name.clone());
        }

        if !d.name.is_empty() && _trail_matches(&new_trail, parts) {
            let start = if d.doc_start_byte > 0 {
                std::cmp::max(d.doc_start_byte, d.start_byte)
            } else {
                d.start_byte
            };
            let end = d.end_byte;
            let source = String::from_utf8_lossy(&src[start..end]).to_string();

            out.push(SymbolMatch {
                qualified_name: new_trail.join("."),
                kind: d.kind.to_string(),
                start_line: d.start_line,
                end_line: d.end_line,
                source,
                ancestor_signatures: ancestors.iter().map(|a| a.signature.clone()).collect(),
            });
        }

        if !d.children.is_empty() {
            let mut new_ancestors = ancestors.clone();
            new_ancestors.push(d);
            _search_walk(&d.children, src, new_trail, new_ancestors, parts, out);
        }
    }
}

fn _trail_matches(trail: &[String], parts: &[&str]) -> bool {
    if parts.len() > trail.len() {
        return false;
    }
    let start = trail.len() - parts.len();
    for (i, p) in parts.iter().enumerate() {
        if trail[start + i] != *p {
            return false;
        }
    }
    true
}

#[derive(Clone, Serialize)]
pub struct ImplMatch {
    pub path: String,
    pub start_line: usize,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<String>,
}

pub fn find_implementations(
    results: &[ParseResult],
    type_name: &str,
    transitive: bool,
) -> Vec<ImplMatch> {
    let target = _normalize_type_name(type_name);
    let mut all_types: Vec<(&std::path::Path, &Declaration)> = Vec::new();
    for r in results {
        _collect_candidate_types(&r.declarations, &r.path, &mut all_types);
    }

    let mut direct = Vec::new();
    for (path, d) in &all_types {
        for b in &d.bases {
            if _normalize_type_name(b) == target {
                direct.push(_impl_match(path, d, Vec::new()));
                break;
            }
        }
    }

    if !transitive {
        return direct;
    }

    let mut out = direct.clone();
    let mut seen: std::collections::HashSet<(String, usize)> = std::collections::HashSet::new();
    for m in &direct {
        seen.insert((m.path.clone(), m.start_line));
    }

    let mut frontier = direct;
    while !frontier.is_empty() {
        let mut next_frontier = Vec::new();
        for parent in frontier {
            let parent_name = _normalize_type_name(&parent.name);
            for (path, d) in &all_types {
                let key = (path.to_string_lossy().to_string(), d.start_line);
                if seen.contains(&key) {
                    continue;
                }
                for b in &d.bases {
                    if _normalize_type_name(b) == parent_name {
                        let mut chain = parent.via.clone();
                        chain.push(parent.name.clone());
                        let m = _impl_match(path, d, chain);
                        seen.insert(key.clone());
                        out.push(_impl_match(path, d, m.via.clone()));
                        next_frontier.push(m);
                        break;
                    }
                }
            }
        }
        frontier = next_frontier;
    }
    out
}

fn _collect_candidate_types<'a>(
    decls: &'a [Declaration],
    path: &'a std::path::Path,
    out: &mut Vec<(&'a std::path::Path, &'a Declaration)>,
) {
    use DeclarationKind::*;
    for d in decls {
        if matches!(d.kind, Class | Struct | Interface | Record) {
            out.push((path, d));
        }
        if !d.children.is_empty() {
            _collect_candidate_types(&d.children, path, out);
        }
    }
}

fn _impl_match(path: &std::path::Path, d: &Declaration, via: Vec<String>) -> ImplMatch {
    ImplMatch {
        path: path.to_string_lossy().to_string(),
        start_line: d.start_line,
        kind: d.kind.to_string(),
        name: d.name.clone(),
        via,
    }
}

fn _normalize_type_name(name: &str) -> String {
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

fn _digest_markdown(
    decls: &[Declaration],
    opts: &DigestOptions,
    indent: usize,
    depth: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    if depth > opts.max_heading_depth {
        return out;
    }
    let pad = " ".repeat(indent);
    for d in decls {
        if matches!(d.kind, DeclarationKind::Heading) {
            out.push(format!("{}{}{}", pad, d.signature, d.lines_suffix()));
            out.extend(_digest_markdown(&d.children, opts, indent + 2, depth + 1));
        } else if matches!(d.kind, DeclarationKind::CodeBlock) && opts.include_fields {
            out.push(format!("{}{}{}", pad, d.signature, d.lines_suffix()));
        }
    }
    out
}

// Q. T. Felix - start
// --- JSON view ---
//
// JSON is just another view over the same Declaration symbol graph that
// powers the terminal/digest formatters. The schema is versioned via the
// JSON_SCHEMA_* constants above; bump those on breaking changes.

#[derive(Serialize)]
struct JsonOutlineDoc<'a> {
    schema: &'static str,
    files: Vec<JsonFile<'a>>,
}

#[derive(Serialize)]
struct JsonFile<'a> {
    path: String,
    language: &'static str,
    line_count: usize,
    error_count: usize,
    declarations: Vec<Declaration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<&'a str>,
}

#[derive(Serialize)]
struct JsonImplementsDoc<'a> {
    schema: &'static str,
    target: &'a str,
    transitive: bool,
    matches: Vec<JsonImplMatch<'a>>,
}

#[derive(Serialize)]
struct JsonImplMatch<'a> {
    path: &'a str,
    start_line: usize,
    kind: &'a str,
    name: &'a str,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    via: &'a [String],
}

#[derive(Serialize)]
struct JsonShowDoc<'a> {
    schema: &'static str,
    path: String,
    language: &'static str,
    matches: Vec<&'a SymbolMatch>,
}

fn _filter_decls(decls: &[Declaration], opts: &OutlineOptions) -> Vec<Declaration> {
    use DeclarationKind::*;
    let mut out = Vec::new();
    for d in decls {
        let is_field = matches!(d.kind, Field | Property | Event | Indexer);
        if is_field && !opts.include_fields {
            continue;
        }
        if d.visibility == "private" && !opts.include_private {
            continue;
        }
        let mut clone = d.clone();
        clone.children = _filter_decls(&d.children, opts);
        out.push(clone);
    }
    out
}

pub fn render_json_outline(results: &[ParseResult], opts: &OutlineOptions, pretty: bool) -> String {
    let files: Vec<JsonFile> = results
        .iter()
        .map(|r| JsonFile {
            path: r.path.to_string_lossy().to_string(),
            language: r.language,
            line_count: r.line_count,
            error_count: r.error_count,
            declarations: _filter_decls(&r.declarations, opts),
            warning: if r.error_count > 0 {
                Some("output may be incomplete")
            } else {
                None
            },
        })
        .collect();

    let doc = JsonOutlineDoc {
        schema: JSON_SCHEMA_OUTLINE,
        files,
    };
    if pretty {
        serde_json::to_string_pretty(&doc).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
    } else {
        serde_json::to_string(&doc).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
    }
}

pub fn render_json_implements(
    target: &str,
    matches: &[ImplMatch],
    transitive: bool,
    pretty: bool,
) -> String {
    let mapped: Vec<JsonImplMatch> = matches
        .iter()
        .map(|m| JsonImplMatch {
            path: &m.path,
            start_line: m.start_line,
            kind: &m.kind,
            name: &m.name,
            via: &m.via,
        })
        .collect();

    let doc = JsonImplementsDoc {
        schema: JSON_SCHEMA_IMPLEMENTS,
        target,
        transitive,
        matches: mapped,
    };
    if pretty {
        serde_json::to_string_pretty(&doc).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
    } else {
        serde_json::to_string(&doc).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
    }
}

pub fn render_json_show(result: &ParseResult, matches: &[SymbolMatch], pretty: bool) -> String {
    let doc = JsonShowDoc {
        schema: JSON_SCHEMA_SHOW,
        path: result.path.to_string_lossy().to_string(),
        language: result.language,
        matches: matches.iter().collect(),
    };
    if pretty { // it perfectly same lal
        serde_json::to_string_pretty(&doc).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
    } else {
        serde_json::to_string(&doc).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
    }
}
// Q. T. Felix - end
