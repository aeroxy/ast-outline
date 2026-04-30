use clap::{Parser, Subcommand};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

mod adapters;
mod core;
mod prompt;
mod installers;
mod hook;
mod main_helpers;
mod mcp;

use crate::core::{DigestOptions, OutlineOptions, ParseResult};

#[derive(Parser)]
#[command(name = "ast-outline")]
#[command(version)]
#[command(about = "Fast, AST-based structural outline for source files", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Files or directories to outline (default command)
    #[arg(num_args = 1..)]
    paths: Vec<PathBuf>,

    #[arg(long)]
    no_private: bool,
    #[arg(long)]
    no_fields: bool,
    #[arg(long)]
    no_docs: bool,
    #[arg(long)]
    no_attrs: bool,
    #[arg(long)]
    no_lines: bool,
    #[arg(long)]
    glob: Option<String>,
    /// Emit output as JSON instead of text
    #[arg(long)]
    json: bool,
    /// With --json: emit compact (single-line) JSON instead of pretty-printed
    #[arg(long)]
    compact: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract source of a symbol
    Show {
        path: PathBuf,
        symbol: String,
        #[arg(num_args = 0..)]
        others: Vec<String>,
        /// Emit output as JSON instead of text
        #[arg(long)]
        json: bool,
        /// With --json: emit compact (single-line) JSON
        #[arg(long)]
        compact: bool,
    },
    /// One-page module map
    Digest {
        #[arg(num_args = 1..)]
        paths: Vec<PathBuf>,

        #[arg(long)]
        include_private: bool,
        #[arg(long)]
        include_fields: bool,
        #[arg(long, default_value_t = 50)]
        max_members: usize,
        /// Emit output as JSON instead of text
        #[arg(long)]
        json: bool,
        /// With --json: emit compact (single-line) JSON
        #[arg(long)]
        compact: bool,
    },
    /// Find subclasses / implementations
    Implements {
        target: String,
        #[arg(num_args = 1..)]
        paths: Vec<PathBuf>,

        #[arg(short, long)]
        direct: bool,
        /// Emit output as JSON instead of text
        #[arg(long)]
        json: bool,
        /// With --json: emit compact (single-line) JSON
        #[arg(long)]
        compact: bool,
    },
    /// Print the agent prompt snippet
    Prompt,
    /// Install ast-outline into a coding-agent CLI
    Install {
        #[arg(long, conflicts_with = "all")]
        target: Option<String>,
        #[arg(long, conflicts_with = "target")]
        all: bool,
        #[arg(long)]
        local: bool,
        #[arg(long, conflicts_with = "local")]
        global: bool,
        #[arg(long)]
        always: bool,
        #[arg(long, default_value_t = 200)]
        min_lines: usize,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        force: bool,
    },
    /// Remove ast-outline from a coding-agent CLI
    Uninstall {
        #[arg(long, conflicts_with = "all")]
        target: Option<String>,
        #[arg(long, conflicts_with = "target")]
        all: bool,
        #[arg(long)]
        local: bool,
        #[arg(long, conflicts_with = "local")]
        global: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Report what's installed where
    Status {
        #[arg(long)]
        local: bool,
        #[arg(long, conflicts_with = "local")]
        global: bool,
    },
    /// Internal: read a tool-call event from stdin and respond
    Hook {
        #[arg(long)]
        protocol: String,
        #[arg(long, default_value_t = 200)]
        min_lines: usize,
        #[arg(long)]
        always: bool,
    },
    /// Run as an MCP (Model Context Protocol) server over stdio
    Mcp,
}

pub(crate) fn parse_file(path: &Path) -> Option<ParseResult> {
    crate::main_helpers::parse_file_for_hook(path)
}

pub(crate) fn walk_and_parse(paths: &[PathBuf], glob_str: Option<&str>) -> Vec<ParseResult> {
    let (tx, rx) = std::sync::mpsc::channel();

    if paths.is_empty() {
        return Vec::new();
    }

    let mut builder = WalkBuilder::new(&paths[0]);
    for p in paths.iter().skip(1) {
        builder.add(p);
    }

    builder.hidden(false); // don't ignore hidden files automatically if they match

    if let Some(g) = glob_str {
        if let Ok(override_builder) = ignore::overrides::OverrideBuilder::new("").add(g) {
            if let Ok(over) = override_builder.build() {
                builder.overrides(over);
            }
        }
    }

    let walker = builder.build_parallel();

    walker.run(|| {
        let tx = tx.clone();
        Box::new(move |result| {
            if let Ok(entry) = result {
                if entry.file_type().is_some_and(|ft| ft.is_file()) {
                    if let Some(parsed) = parse_file(entry.path()) {
                        let _ = tx.send(parsed);
                    }
                }
            }
            ignore::WalkState::Continue
        })
    });

    drop(tx);
    let mut results: Vec<_> = rx.into_iter().collect();
    results.sort_by(|a, b| a.path.cmp(&b.path));
    results
}

fn main() {
    let cli = Cli::parse();

    if let Some(cmd) = &cli.command {
        match cmd {
            Commands::Show {
                path,
                symbol,
                others,
                json,
                compact,
            } => {
                if let Some(res) = parse_file(path) {
                    let mut symbols = vec![symbol.as_str()];
                    symbols.extend(others.iter().map(|s| s.as_str()));
                    if *json || cli.json {
                        let mut seen = std::collections::HashSet::new();
                        let mut all_matches = Vec::new();
                        for sym in symbols {
                            for m in crate::core::find_symbols(&res, sym) {
                                let key = (m.start_line, m.end_line, m.qualified_name.clone());
                                if seen.insert(key) {
                                    all_matches.push(m);
                                }
                            }
                        }
                        println!(
                            "{}",
                            crate::core::render_json_show(&res, &all_matches, !(*compact || cli.compact))
                        );
                    } else {
                        for sym in symbols {
                            let matches = crate::core::find_symbols(&res, sym);
                            for m in matches {
                                println!(
                                    "# {}:{}-{} {} ({})",
                                    res.path.display(),
                                    m.start_line,
                                    m.end_line,
                                    m.qualified_name,
                                    m.kind
                                );
                                if !m.ancestor_signatures.is_empty() {
                                    println!("# in: {}", m.ancestor_signatures.join(" → "));
                                }
                                println!("{}", m.source);
                            }
                        }
                    }
                }
            }
            Commands::Digest {
                paths,
                include_private,
                include_fields,
                max_members,
                json,
                compact,
            } => {
                let results = walk_and_parse(paths, None);
                if *json || cli.json {
                    let opts = OutlineOptions {
                        include_private: *include_private,
                        include_fields: *include_fields,
                        include_docs: true,
                        include_attributes: true,
                        include_line_numbers: true,
                        max_doc_lines: 6,
                        max_members: Some(*max_members),
                    };
                    println!(
                        "{}",
                        crate::core::render_json_outline(&results, &opts, !(*compact || cli.compact))
                    );
                } else {
                    let opts = DigestOptions {
                        include_private: *include_private,
                        include_fields: *include_fields,
                        max_members_per_type: *max_members,
                        max_heading_depth: 3,
                    };
                    let root = if paths.len() == 1 && paths[0].is_dir() {
                        Some(paths[0].as_path())
                    } else {
                        None
                    };
                    println!("{}", crate::core::render_digest(&results, &opts, root));
                }
            }
            Commands::Implements {
                target,
                paths,
                direct,
                json,
                compact,
            } => {
                let results = walk_and_parse(paths, None);
                let transitive = !direct;
                let matches = crate::core::find_implementations(&results, target, transitive);
                if *json || cli.json {
                    println!(
                        "{}",
                        crate::core::render_json_implements(
                            target,
                            &matches,
                            transitive,
                            !(*compact || cli.compact),
                        )
                    );
                } else {
                    println!(
                        "# {} match(es) for '{}' (incl. transitive):",
                        matches.len(),
                        target
                    );
                    for m in matches {
                        let via = if m.via.is_empty() {
                            String::new()
                        } else {
                            format!(" [via {}]", m.via.last().unwrap())
                        };
                        println!("{}:{}  {} {}{}", m.path, m.start_line, m.kind, m.name, via);
                    }
                }
            }
            Commands::Prompt => {
                println!("{}", crate::prompt::AGENT_PROMPT);
            }
            Commands::Install {
                target,
                all,
                local,
                global,
                always,
                min_lines,
                dry_run,
                force,
            } => {
                let scope = resolve_scope(*local, *global);
                let opts = installers::InstallOpts {
                    min_lines: *min_lines,
                    always: *always,
                    dry_run: *dry_run,
                    force: *force,
                };
                let exit = run_install(target.as_deref(), *all, &scope, &opts);
                std::process::exit(exit);
            }
            Commands::Uninstall {
                target,
                all,
                local,
                global,
                dry_run,
            } => {
                let scope = resolve_scope(*local, *global);
                let opts = installers::InstallOpts {
                    dry_run: *dry_run,
                    ..installers::InstallOpts::default()
                };
                let exit = run_uninstall(target.as_deref(), *all, &scope, &opts);
                std::process::exit(exit);
            }
            Commands::Status { local, global } => {
                let scope = resolve_scope(*local, *global);
                run_status(&scope);
            }
            Commands::Hook {
                protocol,
                min_lines,
                always,
            } => {
                let exit = hook::run(protocol, *min_lines, *always);
                std::process::exit(exit);
            }
            Commands::Mcp => {
                let exit = mcp::run();
                std::process::exit(exit);
            }
        }
    } else if !cli.paths.is_empty() {
        let results = walk_and_parse(&cli.paths, cli.glob.as_deref());
        let opts = OutlineOptions {
            include_private: !cli.no_private,
            include_fields: !cli.no_fields,
            include_docs: !cli.no_docs,
            include_attributes: !cli.no_attrs,
            include_line_numbers: !cli.no_lines,
            max_doc_lines: 6,
            max_members: None,
        };
        if cli.json {
            println!("{}", crate::core::render_json_outline(&results, &opts, !cli.compact));
        } else {
            for res in results {
                println!("{}", crate::core::render_outline(&res, &opts));
                println!();
            }
        }
    } else {
        println!("Please provide a path or command.");
    }
}

fn resolve_scope(local: bool, _global: bool) -> installers::Scope {
    if local {
        installers::Scope::Local(std::env::current_dir().expect("cwd"))
    } else {
        installers::Scope::Global
    }
}

fn run_install(
    target: Option<&str>,
    all: bool,
    scope: &installers::Scope,
    opts: &installers::InstallOpts,
) -> i32 {
    let registry = installers::registry();
    let chosen: Vec<&Box<dyn installers::Installer>> = if all {
        select_all(&registry, scope)
    } else if let Some(name) = target {
        match registry.iter().find(|i| i.name() == name) {
            Some(i) => vec![i],
            None => {
                eprintln!(
                    "unknown --target '{}'. Known: {}",
                    name,
                    names(&registry)
                );
                return 2;
            }
        }
    } else {
        eprintln!(
            "must pass --target <name> or --all. Known: {}",
            names(&registry)
        );
        return 2;
    };

    let mut any_installed = false;
    let mut any_failed = false;
    for inst in chosen {
        let label = inst.name();
        match inst.install_prompt(scope, opts) {
            Ok(c) => {
                print_change(label, "prompt", &c);
                if !matches!(
                    c,
                    installers::Change::Skipped { .. } | installers::Change::NotApplicable
                ) {
                    any_installed = true;
                }
            }
            Err(e) => {
                eprintln!("{}: prompt: {}", label, e);
                any_failed = true;
            }
        }
        match inst.install_hook(scope, opts) {
            Ok(c) => {
                print_change(label, "hook", &c);
                if !matches!(
                    c,
                    installers::Change::Skipped { .. } | installers::Change::NotApplicable
                ) {
                    any_installed = true;
                }
            }
            Err(e) => {
                eprintln!("{}: hook: {}", label, e);
                any_failed = true;
            }
        }
    }

    if any_failed && any_installed {
        1
    } else if any_failed {
        2
    } else {
        0
    }
}

fn run_uninstall(
    target: Option<&str>,
    all: bool,
    scope: &installers::Scope,
    opts: &installers::InstallOpts,
) -> i32 {
    let registry = installers::registry();
    let chosen: Vec<&Box<dyn installers::Installer>> = if all {
        select_all(&registry, scope)
    } else if let Some(name) = target {
        match registry.iter().find(|i| i.name() == name) {
            Some(i) => vec![i],
            None => {
                eprintln!(
                    "unknown --target '{}'. Known: {}",
                    name,
                    names(&registry)
                );
                return 2;
            }
        }
    } else {
        eprintln!(
            "must pass --target <name> or --all. Known: {}",
            names(&registry)
        );
        return 2;
    };

    let mut any_failed = false;
    for inst in chosen {
        match inst.uninstall(scope, opts) {
            Ok(changes) => {
                for c in changes {
                    print_change(inst.name(), "uninstall", &c);
                }
            }
            Err(e) => {
                eprintln!("{}: {}", inst.name(), e);
                any_failed = true;
            }
        }
    }
    if any_failed {
        1
    } else {
        0
    }
}

fn run_status(scope: &installers::Scope) {
    for inst in installers::registry() {
        let s = inst.status(scope);
        let prompt = if s.prompt_installed {
            format!("prompt {}", s.prompt_version.unwrap_or_else(|| "?".into()))
        } else {
            "prompt -".to_string()
        };
        let hook = if s.hook_installed { "hook ✓" } else { "hook -" };
        println!("{:<14} {:<14} {}", inst.name(), prompt, hook);
    }
}

fn names(registry: &[Box<dyn installers::Installer>]) -> String {
    registry
        .iter()
        .map(|i| i.name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Picks the adapters to act on for `--all`. For `Scope::Global`, we
/// skip targets whose `detect()` reports the CLI is absent (and print a
/// note). For `Scope::Local`, the user explicitly opted into this repo
/// so detection is bypassed.
#[allow(clippy::borrowed_box)]
fn select_all<'a>(
    registry: &'a [Box<dyn installers::Installer>],
    scope: &installers::Scope,
) -> Vec<&'a Box<dyn installers::Installer>> {
    let bypass_detection = matches!(scope, installers::Scope::Local(_));
    registry
        .iter()
        .filter(|inst| {
            if bypass_detection {
                return true;
            }
            let d = inst.detect(scope);
            if !d.present {
                println!("{:<14} {:<10} skipped  (not detected on this system)", inst.name(), "detect");
            }
            d.present
        })
        .collect()
}

fn print_change(target: &str, phase: &str, change: &installers::Change) {
    use installers::Change::*;
    match change {
        Created(p) => println!("{:<14} {:<10} created  {}", target, phase, p.display()),
        Updated(p) => println!("{:<14} {:<10} updated  {}", target, phase, p.display()),
        Removed(p) => println!("{:<14} {:<10} removed  {}", target, phase, p.display()),
        Skipped { path, reason } => {
            println!(
                "{:<14} {:<10} skipped  {} ({})",
                target,
                phase,
                path.display(),
                reason
            )
        }
        NotApplicable => println!("{:<14} {:<10} n/a", target, phase),
    }
}
