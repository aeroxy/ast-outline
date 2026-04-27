use clap::{Parser, Subcommand};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

mod adapters;
mod core;
mod prompt;
mod installers;
mod hook;
mod main_helpers;

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
}

#[derive(Subcommand)]
enum Commands {
    /// Extract source of a symbol
    Show {
        path: PathBuf,
        symbol: String,
        #[arg(num_args = 0..)]
        others: Vec<String>,
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
    },
    /// Find subclasses / implementations
    Implements {
        target: String,
        #[arg(num_args = 1..)]
        paths: Vec<PathBuf>,

        #[arg(short, long)]
        direct: bool,
    },
    /// Print the agent prompt snippet
    Prompt,
    /// Internal: read a tool-call event from stdin and respond
    Hook {
        #[arg(long)]
        protocol: String,
        #[arg(long, default_value_t = 200)]
        min_lines: usize,
        #[arg(long)]
        always: bool,
    },
}

fn parse_file(path: &Path) -> Option<ParseResult> {
    crate::main_helpers::parse_file_for_hook(path)
}

fn walk_and_parse(paths: &[PathBuf], glob_str: Option<&str>) -> Vec<ParseResult> {
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
                if entry.file_type().map_or(false, |ft| ft.is_file()) {
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
            } => {
                if let Some(res) = parse_file(path) {
                    let mut symbols = vec![symbol.as_str()];
                    symbols.extend(others.iter().map(|s| s.as_str()));
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
            Commands::Digest {
                paths,
                include_private,
                include_fields,
                max_members,
            } => {
                let results = walk_and_parse(paths, None);
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
            Commands::Implements {
                target,
                paths,
                direct,
            } => {
                let results = walk_and_parse(paths, None);
                let matches = crate::core::find_implementations(&results, target, !direct);
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
            Commands::Prompt => {
                println!("{}", crate::prompt::AGENT_PROMPT);
            }
            Commands::Hook {
                protocol,
                min_lines,
                always,
            } => {
                let exit = hook::run(protocol, *min_lines, *always);
                std::process::exit(exit);
            }
        }
    } else if !cli.paths.is_empty() {
        let results = walk_and_parse(&cli.paths, cli.glob.as_deref());
        let opts = OutlineOptions {
            include_private: !cli.no_private,
            include_fields: !cli.no_fields,
            include_xml_doc: !cli.no_docs,
            include_attributes: !cli.no_attrs,
            include_line_numbers: !cli.no_lines,
            max_doc_lines: 6,
        };
        for res in results {
            println!("{}", crate::core::render_outline(&res, &opts));
            println!("");
        }
    } else {
        println!("Please provide a path or command.");
    }
}
