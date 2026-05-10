//! End-to-end tests for `callers` / `callees` against a small fixture
//! repo containing inter-file calls in Rust, Python, and TypeScript.
//!
//! These don't try to assert the full graph — just that the resolver
//! finds the *right* callers/callees and doesn't include obvious noise.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ast-outline"))
}

fn run_in(dir: &std::path::Path, args: &[&str]) -> (String, i32) {
    let out = Command::new(bin())
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("run");
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    (stdout, out.status.code().unwrap_or(-1))
}

fn write(p: &std::path::Path, body: &str) {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, body).unwrap();
}

#[test]
fn rust_callers_finds_cross_file_caller() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Minimal Cargo project so the dep resolver detects this as a project root.
    write(&root.join("Cargo.toml"), "[package]\nname = \"smoke\"\nversion = \"0.0.0\"\nedition = \"2021\"\n");
    write(
        &root.join("src/lib.rs"),
        r#"
pub mod helper;
pub fn run() {
    helper::greet();
}
"#,
    );
    write(
        &root.join("src/helper.rs"),
        r#"
pub fn greet() {
    println!("hi");
}
"#,
    );

    let (out, code) = run_in(root, &["callers", "greet", ".", "--rebuild"]);
    assert_eq!(code, 0, "callers exited non-zero: {}", out);
    assert!(
        out.contains("src/lib.rs") && out.contains("run"),
        "expected lib.rs::run in callers output, got:\n{}",
        out
    );
}

#[test]
fn rust_callees_lists_local_call() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("Cargo.toml"), "[package]\nname = \"smoke\"\nversion = \"0.0.0\"\nedition = \"2021\"\n");
    write(
        &root.join("src/lib.rs"),
        r#"
pub fn helper() {}
pub fn run() {
    helper();
}
"#,
    );
    let (out, code) = run_in(root, &["callees", "run", ".", "--rebuild"]);
    assert_eq!(code, 0, "callees exited non-zero: {}", out);
    assert!(
        out.contains("helper"),
        "expected `helper` in callees output, got:\n{}",
        out
    );
}

#[test]
fn python_callers_finds_cross_file_caller() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // pyproject.toml so the dep resolver picks this dir as the root.
    write(
        &root.join("pyproject.toml"),
        "[project]\nname = \"smoke\"\nversion = \"0.0.0\"\n",
    );
    write(
        &root.join("smoke/__init__.py"),
        "",
    );
    write(
        &root.join("smoke/helper.py"),
        "def greet():\n    print('hi')\n",
    );
    write(
        &root.join("smoke/main.py"),
        "from smoke.helper import greet\n\ndef run():\n    greet()\n",
    );
    let (out, code) = run_in(root, &["callers", "greet", ".", "--rebuild"]);
    assert_eq!(code, 0, "callers exited non-zero: {}", out);
    assert!(
        out.contains("smoke/main.py") && out.contains("run"),
        "expected smoke/main.py::run in callers, got:\n{}",
        out
    );
}

#[test]
fn typescript_callers_finds_cross_file_caller() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        &root.join("package.json"),
        r#"{"name":"smoke","version":"0.0.0"}"#,
    );
    write(
        &root.join("src/helper.ts"),
        "export function greet(): void { console.log('hi'); }\n",
    );
    write(
        &root.join("src/main.ts"),
        "import { greet } from './helper';\n\nexport function run(): void {\n  greet();\n}\n",
    );
    let (out, code) = run_in(root, &["callers", "greet", ".", "--rebuild"]);
    assert_eq!(code, 0, "callers exited non-zero: {}", out);
    assert!(
        out.contains("src/main.ts") && out.contains("run"),
        "expected src/main.ts::run in callers, got:\n{}",
        out
    );
}

#[test]
fn callers_with_file_filter_narrows_match() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.0.0\"\nedition=\"2021\"\n");
    // Two functions named `helper` in different files. `use` brings each
    // into scope so pass A resolves the calls precisely (no receiver).
    write(
        &root.join("src/lib.rs"),
        r#"
pub mod a;
pub mod b;
pub mod consumer_a;
pub mod consumer_b;
"#,
    );
    write(&root.join("src/a.rs"), "pub fn helper() {}\n");
    write(&root.join("src/b.rs"), "pub fn helper() {}\n");
    write(
        &root.join("src/consumer_a.rs"),
        "use crate::a::helper;\npub fn run_a() { helper(); }\n",
    );
    write(
        &root.join("src/consumer_b.rs"),
        "use crate::b::helper;\npub fn run_b() { helper(); }\n",
    );

    // With the file filter, only callers of `src/a.rs::helper` should appear.
    let (out, code) = run_in(root, &["callers", "src/a.rs:helper", ".", "--rebuild"]);
    assert_eq!(code, 0, "callers exited non-zero: {}", out);
    assert!(
        out.contains("run_a"),
        "expected run_a (caller of a::helper), got:\n{}",
        out
    );
    assert!(
        !out.contains("run_b"),
        "did not expect run_b (caller of b::helper), got:\n{}",
        out
    );
}

#[test]
fn callers_with_flag_form_matches_positional_form() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.0.0\"\nedition=\"2021\"\n");
    write(&root.join("src/lib.rs"), "pub mod h;\nuse crate::h::greet;\npub fn run() { greet(); }\n");
    write(&root.join("src/h.rs"), "pub fn greet() {}\n");

    let (positional_out, code1) =
        run_in(root, &["callers", "src/h.rs:greet", ".", "--rebuild"]);
    assert_eq!(code1, 0);

    // `--file` / `--symbol` form. Note: omit the trailing positional path
    // (defaults to "."); clap can't disambiguate optional-positional vs
    // optional-target when both are present, same shape as `find-related`.
    let (flag_out, code2) = run_in(
        root,
        &["callers", "--file", "src/h.rs", "--symbol", "greet", "--rebuild"],
    );
    assert_eq!(code2, 0);

    // Strip the header line which differs ("for 'X:Y'" vs "for 'X:Y'") —
    // both spell the target the same way after compose_target, so they
    // should match exactly. We compare the body lines for safety.
    let body_pos: Vec<&str> = positional_out.lines().filter(|l| l.starts_with("src/")).collect();
    let body_flag: Vec<&str> = flag_out.lines().filter(|l| l.starts_with("src/")).collect();
    assert_eq!(body_pos, body_flag, "flag form should match positional form");
    assert!(
        body_pos.iter().any(|l| l.contains("run")),
        "expected `run` in callers output, got:\n{}",
        positional_out
    );
}

#[test]
fn callers_file_filter_unknown_path_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.0.0\"\nedition=\"2021\"\n");
    write(&root.join("src/lib.rs"), "pub fn foo() {}\n");
    let out = Command::new(bin())
        .args(["callers", "src/nope.rs:foo", "."])
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2), "expected exit 2 when file filter has no matches");
}

#[test]
fn passing_subdir_as_path_walks_up_to_project_root() {
    // Regression: `ast-outline callers <sym> ./src` used to treat ./src as
    // the project root, producing qns like `main.rs::run` instead of
    // `src/main.rs::run`. The `<file>:<symbol>` filter then silently missed.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.0.0\"\nedition=\"2021\"\n");
    write(&root.join("src/lib.rs"), "pub mod h;\nuse crate::h::greet;\npub fn run() { greet(); }\n");
    write(&root.join("src/h.rs"), "pub fn greet() {}\n");

    let (out, code) = run_in(
        root,
        &["callers", "src/h.rs:greet", "./src", "--rebuild"],
    );
    assert_eq!(code, 0, "callers exited non-zero: {}", out);
    assert!(
        out.contains("run"),
        "expected `run` (caller of greet) when project root is walked up to, got:\n{}",
        out
    );
}

#[test]
fn rust_callers_on_trait_returns_implementations() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.0.0\"\nedition=\"2021\"\n");
    write(
        &root.join("src/lib.rs"),
        r#"
pub trait Animal { fn speak(&self); }

pub struct Dog;
impl Animal for Dog { fn speak(&self) { println!("woof"); } }

pub struct Cat;
impl Animal for Cat { fn speak(&self) { println!("meow"); } }
"#,
    );
    let (out, code) = run_in(root, &["callers", "Animal", ".", "--rebuild"]);
    assert_eq!(code, 0, "callers exited non-zero: {}", out);
    assert!(
        out.contains("implementation(s)"),
        "expected implementations group, got:\n{}",
        out
    );
    assert!(
        out.contains("Dog") && out.contains("Cat"),
        "expected both impls listed, got:\n{}",
        out
    );
}

#[test]
fn rust_callers_on_struct_returns_constructions() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.0.0\"\nedition=\"2021\"\n");
    write(
        &root.join("src/lib.rs"),
        r#"
pub struct Greeter;
impl Greeter {
    pub fn hello(&self) {}
}

pub fn run() {
    Greeter.hello();
}
"#,
    );
    let (out, code) = run_in(root, &["callers", "Greeter", ".", "--rebuild"]);
    assert_eq!(code, 0, "callers exited non-zero: {}", out);
    assert!(
        out.contains("construction(s)"),
        "expected constructions group, got:\n{}",
        out
    );
    assert!(
        out.contains("run"),
        "expected `run` (caller of Greeter.hello) in constructions, got:\n{}",
        out
    );
}

#[test]
fn callees_on_subtype_walks_to_ancestor_and_lists_its_methods() {
    // `callees <Type>` is the inverse of `callers <Type>` on the type
    // relationship graph: callers = downstream uses; callees = upstream
    // bases + the methods declared on those bases.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.0.0\"\nedition=\"2021\"\n");
    write(
        &root.join("src/lib.rs"),
        r#"
pub trait Animal {
    fn speak(&self);
    fn breathe(&self);
}

pub struct Dog;
impl Animal for Dog {
    fn speak(&self) {}
    fn breathe(&self) {}
}
"#,
    );
    let (out, code) = run_in(root, &["callees", "Dog", ".", "--rebuild"]);
    assert_eq!(code, 0, "callees exited non-zero: {}", out);
    assert!(
        out.contains("ancestor(s) of struct Dog"),
        "expected ancestor header, got:\n{}",
        out
    );
    assert!(
        out.contains("trait Animal"),
        "expected `Animal` ancestor listed, got:\n{}",
        out
    );
    assert!(
        out.contains("speak") && out.contains("breathe"),
        "expected ancestor's method signatures listed, got:\n{}",
        out
    );
}

#[test]
fn callees_on_root_type_reports_no_ancestors() {
    // A type with no `bases` (e.g. a top-level trait or a unit struct
    // without `impl X for` blocks) returns gracefully without errors.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.0.0\"\nedition=\"2021\"\n");
    write(
        &root.join("src/lib.rs"),
        "pub trait Animal { fn speak(&self); }\n",
    );
    let (out, code) = run_in(root, &["callees", "Animal", ".", "--rebuild"]);
    assert_eq!(code, 0, "callees on root type should not error, got exit {}", code);
    assert!(
        out.contains("no ancestors"),
        "expected `no ancestors` notice, got:\n{}",
        out
    );
}

#[test]
fn callees_on_type_walks_multiple_levels_with_depth() {
    // `--depth 2` should chase grandparents in a Java-style hierarchy
    // (Rust traits don't typically nest, but Scala / Java / Kotlin do).
    // Use Java for this test since multi-level hierarchies are idiomatic
    // there and tree-sitter-java is in our adapter set.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        &root.join("pom.xml"),
        "<project><modelVersion>4.0.0</modelVersion><groupId>x</groupId><artifactId>x</artifactId><version>0.0.0</version></project>\n",
    );
    write(
        &root.join("src/Animal.java"),
        "package smoke;\npublic interface Animal { void speak(); }\n",
    );
    write(
        &root.join("src/Mammal.java"),
        "package smoke;\npublic interface Mammal extends Animal { void nurse(); }\n",
    );
    write(
        &root.join("src/Dog.java"),
        "package smoke;\npublic class Dog implements Mammal { public void speak() {} public void nurse() {} }\n",
    );
    let (out, code) = run_in(root, &["callees", "Dog", ".", "--depth", "2", "--rebuild"]);
    assert_eq!(code, 0, "callees exited non-zero: {}", out);
    assert!(
        out.contains("Mammal"),
        "expected direct ancestor `Mammal`, got:\n{}",
        out
    );
    assert!(
        out.contains("Animal"),
        "expected grandparent `Animal` at depth=2, got:\n{}",
        out
    );
    assert!(
        out.contains("depth=2"),
        "expected `depth=2` annotation, got:\n{}",
        out
    );
}

#[test]
fn callers_unknown_symbol_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.0.0\"\nedition=\"2021\"\n");
    write(&root.join("src/lib.rs"), "pub fn a() {}\n");
    let out = Command::new(bin())
        .args(["callers", "nonexistent_sym_xyz", "."])
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2), "expected exit 2 for unknown symbol");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no symbol matches"),
        "expected hint, got stderr:\n{}",
        stderr
    );
}
