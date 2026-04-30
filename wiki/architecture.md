# Architecture

`ast-outline` is a fast, structurally-aware CLI tool built to extract the shape of source code files without bringing the heavy baggage of method bodies.

It is written natively in Rust, relying heavily on the [tree-sitter](https://tree-sitter.github.io/tree-sitter/) parsing framework via the excellent [`ast-grep`](https://ast-grep.github.io/) ecosystem bindings, achieving incredibly fast speeds while still taking advantage of `rayon` for massive multithreading across directories.

## Core Flow

1. **Routing (`src/main.rs`)**: `ast-outline` iterates through files using the `ignore` crate (which handles `.gitignore` automatically in parallel). Each file extension is identified by `ast-grep`'s `SupportLang::from_path(path)`.
2. **Parsing (`src/adapters/*`)**: The raw source string is handed to `ast-grep` which returns a tree of `ast_grep_core::Node`. A language-specific adapter (e.g. `rust.rs`, `python.rs`) performs a highly tailored AST traversal over these nodes.
3. **IR Generation (`src/core.rs`)**: The traversal emits a canonical `Declaration` tree. This is the Intermediate Representation (IR) shared across every language. It encapsulates `kind`, `name`, `signature`, `docs`, `visibility`, etc.
4. **Rendering (`src/core.rs`)**: 
   - `outline` iterates the declarations to print a hierarchical file breakdown.
   - `digest` squashes the tree into a concise module-level API map.
   - `show` walks the tree for a specific suffix match and extracts the raw string boundaries.
   - `implements` performs a generic Breadth-First-Search across the IR trees of the entire repository to find inheritance hierarchies.
   - `--json` is a fifth rendering mode: any of the above commands accepts `--json` to serialise the same `Declaration` IR directly via `serde_json` into a versioned JSON schema, instead of formatting it as text. Add `--compact` for single-line output.

## MCP Server (`src/mcp/`)

`ast-outline mcp` runs the binary as a [Model Context Protocol](https://modelcontextprotocol.io) server so coding agents can invoke the same operations as native tools. The implementation is intentionally tiny:

- **Transport**: line-delimited JSON-RPC 2.0 on stdin/stdout, fully synchronous — no tokio, no extra dependencies. The cost is ~600 KB of binary (~1%) and zero overhead on the regular CLI commands, since none of the MCP code runs unless you invoke the `mcp` subcommand.
- **`src/mcp/protocol.rs`**: serde types for `Request`/`Response`/`RpcError` and the standard JSON-RPC error codes.
- **`src/mcp/tools.rs`**: declares the four tool schemas and dispatches `tools/call` into the existing `core::render_*` functions. Each tool maps 1:1 to a CLI subcommand and reuses its render logic byte-for-byte, so the JSON schemas (`ast-outline.outline.v1`, `ast-outline.show.v1`, `ast-outline.implements.v1`) are shared with the CLI's `--json` output.
- **`src/mcp/mod.rs`**: read loop, method routing (`initialize`, `ping`, `tools/list`, `tools/call`, `resources/list`, `prompts/list`), and panic-safe tool dispatch (panics are surfaced as `-32603 internal error` instead of taking the server down).

Tools are exposed in their text form by default — that's what the agent prompt is built around — with `json: true` available for any client that wants the structured payload.

## Adding a New Language

Adding a new language is incredibly straightforward due to the foundation provided by `ast-grep-language`.

1. Identify the target language from the `SupportLang` enum in `ast-grep` (e.g. `SupportLang::Cpp`). If not present, you may need to implement a native fallback like we do for `MarkdownLang` in `src/adapters/markdown.rs`.
2. Create a new `src/adapters/mylang.rs` file.
3. Implement the `LanguageAdapter` trait.
4. Write a `_walk_top` function to perform depth-first traversal of the `ast_grep_core::Node` children.
5. Identify AST kinds by matching `node.kind()` and retrieve source values using `node.field("name")` or slicing `src[node.range().start .. node.range().end]`.
6. Convert them to generic `Declaration` objects representing Classes, Functions, Fields, Interfaces, etc.
7. Wire your new adapter into the `parse_file` routing match block in `src/main.rs`!
