use std::path::Path;

use ast_grep_core::Language;
use ast_grep_language::{LanguageExt, SupportLang};

use crate::adapters::base::LanguageAdapter;
use crate::adapters::csharp::CSharpAdapter;
use crate::adapters::go::GoAdapter;
use crate::adapters::java::JavaAdapter;
use crate::adapters::kotlin::KotlinAdapter;
use crate::adapters::python::PythonAdapter;
use crate::adapters::rust::RustAdapter;
use crate::adapters::scala::ScalaAdapter;
use crate::adapters::typescript::TypeScriptAdapter;
use crate::core::ParseResult;

pub fn parse_file_for_hook(path: &Path) -> Option<ParseResult> {
    let lang = SupportLang::from_path(path);
    let source = std::fs::read_to_string(path).ok()?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if matches!(ext, "md" | "markdown" | "mdx" | "mdown") {
        return Some(crate::adapters::markdown::parse_markdown(
            path,
            source.as_bytes(),
        ));
    }
    let lang = lang?;
    match lang {
        SupportLang::Rust => Some(RustAdapter.parse(
            path,
            source.as_bytes(),
            lang.ast_grep(source.clone()).root(),
        )),
        SupportLang::Python => Some(PythonAdapter.parse(
            path,
            source.as_bytes(),
            lang.ast_grep(source.clone()).root(),
        )),
        SupportLang::TypeScript | SupportLang::Tsx | SupportLang::JavaScript => Some(
            TypeScriptAdapter.parse(path, source.as_bytes(), lang.ast_grep(source.clone()).root()),
        ),
        SupportLang::CSharp => Some(CSharpAdapter.parse(
            path,
            source.as_bytes(),
            lang.ast_grep(source.clone()).root(),
        )),
        SupportLang::Go => Some(GoAdapter.parse(
            path,
            source.as_bytes(),
            lang.ast_grep(source.clone()).root(),
        )),
        SupportLang::Java => Some(JavaAdapter.parse(
            path,
            source.as_bytes(),
            lang.ast_grep(source.clone()).root(),
        )),
        SupportLang::Kotlin => Some(KotlinAdapter.parse(
            path,
            source.as_bytes(),
            lang.ast_grep(source.clone()).root(),
        )),
        SupportLang::Scala => Some(ScalaAdapter.parse(
            path,
            source.as_bytes(),
            lang.ast_grep(source.clone()).root(),
        )),
        _ => None,
    }
}
