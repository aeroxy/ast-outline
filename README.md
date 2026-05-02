# ast-outline

Fast, AST-based **structural outline** for source files — classes, methods,
signatures with line numbers, but **no method bodies**. Built for LLM coding
agents and humans who want to read the *shape* of a file before diving into the whole thing.

`ast-outline` is written in Rust, leveraging the incredibly fast [ast-grep](https://github.com/ast-grep/ast-grep) bindings for [tree-sitter](https://tree-sitter.github.io/tree-sitter/), and it utilizes `rayon` to parse your entire workspace concurrently in milliseconds. This project's `show`/`digest`/`implements` commands were inspired by [dim-s/code-outline](https://github.com/dim-s/code-outline), while these three commands are similar, the Rust code itself was largely originated from a bigger code-agent project and uses `ast-grep` for parsing, not a direct port.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

---

## Purpose

**`ast-outline` exists to make LLM coding agents faster, cheaper, and smarter
when navigating unfamiliar code.**

Modern agentic coding tools explore codebases by reading files directly — not via embeddings or vector search. That approach is reliable but has a massive cost: on a 1000-line file, the agent pays for 1000 lines of tokens just to answer *"what methods exist here?"*.

`ast-outline` closes that gap. It's a **pre-reading layer**:

1. **95% token saving.** An outline replaces a full file read when you only need structural understanding.
2. **Faster exploration.** A whole module's public API fits on one screen.
3. **Precise navigation.** Every declaration has a line range (`L42-58`). You go straight to the method body you need.
4. **AST accuracy, not fuzzy match.** `implements` and `show` understand real syntax — no false positives from comments or strings like `grep`.
5. **Zero infrastructure.** No index, no cache, no embeddings, no network. Live, always fresh, invisible to your repo.

### The workflow

**Before `ast-outline`:**

```
Agent: Read Player.cs            # 1200 lines of tokens
Agent: Read Enemy.cs             # 800 lines of tokens
Agent: Read DamageSystem.cs      # 400 lines of tokens
Agent: grep "IDamageable" src/   # noisy, lots of false matches
...
```

**With `ast-outline`:**

```
Agent: ast-outline surface .                 # one-page true public API of the crate/package
Agent: ast-outline digest src/Combat         # ~100 lines, whole module's structure
Agent: ast-outline implements IDamageable    # precise list, no grep noise
Agent: ast-outline search "damage handling"  # hybrid BM25 + dense semantic, ranked
Agent: ast-outline show Player.cs TakeDamage # just the method body
```

Result: **same understanding, a fraction of the tokens, a fraction of the round-trips.**
For "what does this package actually expose?" — historically the most expensive question, since the answer was "read every file" — `surface` resolves the re-export graph and gives you the answer directly, often replacing dozens of file reads with a single call.

---

## Supported languages

| Language | Extensions |
| --- | --- |
| Rust       | `.rs` |
| C#         | `.cs` |
| Python     | `.py`, `.pyi` |
| TypeScript | `.ts`, `.tsx` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| Java       | `.java` |
| Kotlin     | `.kt`, `.kts` |
| Scala      | `.scala`, `.sc` |
| Go         | `.go` |
| Markdown   | `.md`, `.markdown`, `.mdx`, `.mdown` |

*More coming soon! Adding another language is a single new adapter file leveraging the massive `ast-grep` language ecosystem.*

---

## What gets walked

`ast-outline` skips a lot of files when walking a directory — by design. Filters apply uniformly across every subcommand.

1. **`.gitignore` and friends** — every level's `.gitignore`, your global gitignore, `.git/info/exclude`, and `.ignore` files (the [`ignore`](https://crates.io/crates/ignore) crate's convention used by `ripgrep`/`fd`).
2. **Hardcoded denylist** — directories almost no one wants walked, even if `.gitignore` doesn't list them: `.git`, `node_modules`, `target`, `dist`, `build`, `__pycache__`, `.venv`, `venv`, `.cache`, `.idea`, `.vscode`, `.next`, `.nuxt`, `.turbo`, `.parcel-cache`, `.gradle`, `.tox`, `.mypy_cache`, `.pytest_cache`, `.ruff_cache`, `.eggs`, `.ast-outline`, and a few others.
3. **`.ast-outline-ignore`** — per-repo escape hatch. Same syntax as `.gitignore`. Useful for excluding paths from `ast-outline` that you *don't* want excluded from git itself, e.g. test fixtures or vendored corpora:

   ```gitignore
   # .ast-outline-ignore
   tests/fixtures/large_corpus/
   benches/data/
   *.generated.rs
   ```
4. **Extension allowlist** — files are only opened if their extension is one ast-outline knows how to parse (the table above for outline/digest/show/implements; a broader set for the search commands).

Want to see exactly what ast-outline walks? Compare `ast-outline digest some/dir` with `rg --files some/dir` — anything in `rg` but not the digest is being filtered by one of the layers above.

---

## Install

### Homebrew (macOS)

```bash
brew install aeroxy/ast-outline/ast-outline
```

### Cargo

```bash
cargo install ast-outline
```

This installs the `ast-outline` CLI globally into `~/.cargo/bin` — make sure that's on your `PATH`.

### Nix

You can run `ast-outline` directly with Nix without installing:

```bash
nix run github:aeroxy/ast-outline
```

Or add it as a dependency in your Nix flake:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    ast-outline.url = "github:aeroxy/ast-outline";
  };

  outputs = { self, nixpkgs, ast-outline }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      devShells.${system}.default = pkgs.mkShell {
        buildInputs = [ ast-outline.packages.${system}.default ];
      };
    };
}
```

---

## Quick start

```bash
# Structural outline of one file
ast-outline path/to/Player.rs
ast-outline path/to/user_service.py

# Outline a whole directory (recurses supported extensions in parallel)
ast-outline src/

# Print the exact source of one specific method
ast-outline show Player.cs TakeDamage

# Compact public-API map of a whole module
ast-outline digest src/Services

# True public surface (resolves `pub use` / `__all__`, not every `pub` item)
ast-outline surface .                  # auto-detect Cargo.toml / pyproject.toml / __init__.py
ast-outline surface --tree --include-chain mycrate/

# Every class that inherits/implements a given type
ast-outline implements IDamageable src/

# Hybrid BM25 + dense semantic search (builds an index on first call)
ast-outline search "how does login work"
ast-outline search "HandlerStack" -k 5

# Find code semantically similar to a given file:line
ast-outline find-related src/auth/login.rs:42

# Build / refresh / inspect the per-repo search index
ast-outline index            # build or refresh
ast-outline index --stats    # show chunk count, model, etc.
ast-outline index --rebuild  # drop cache and rebuild

# Output a prompt snippet to steer LLM agents
ast-outline prompt >> AGENTS.md

# Machine-readable JSON (stable schema, great for tooling)
ast-outline src/player.rs --json
ast-outline digest src/ --json
ast-outline show Player.cs TakeDamage --json
ast-outline implements IDamageable src/ --json
ast-outline search "json rendering" --json
```

---

## Using with LLM coding agents

This is the main use case. The fastest path is `ast-outline install`,
which writes the agent prompt snippet (and, where supported, a real
`Read`-interceptor hook) into your coding agent's config.

```bash
# Install into every supported CLI it can detect on your system.
ast-outline install --all

# Or pick a single target.
ast-outline install --target claude-code
ast-outline install --target gemini --min-lines 150

# See exactly what would change before writing.
ast-outline install --all --dry-run

# Per-repo install (default is global).
ast-outline install --target claude-code --local

# Remove everything we wrote.
ast-outline uninstall --all

# Quick visibility.
ast-outline status
```

Supported targets: `claude-code`, `gemini`, `tabnine`, `cursor`,
`aider`, `codex`, `copilot`. Claude Code, Gemini, and Tabnine also get
a tool-call hook that intercepts `Read` on supported source files when
they exceed `--min-lines` (default 200) and substitutes the outline.
The other targets receive the prompt only.

Manual install via `ast-outline prompt` (e.g. project-level):

```bash
ast-outline prompt >> AGENTS.md
ast-outline prompt | pbcopy   # macOS clipboard
```

### Works with

- Claude Code (+ custom subagents like `Explore`, `codebase-scout`)
- Cursor agent mode
- Aider
- Copilot Chat / Workspace
- Any custom agent on the Claude / OpenAI / Gemini APIs
- Humans (the colored terminal format is highly readable; `show` is a nice alternative to `grep -A 20`)

---

## Output format

The format is designed to be **LLM-friendly**: Python-style indentation,
line-number suffixes in `L<start>-<end>` form, doc-comments preserved.
The header summarises scale and flags partial parses.

When you run it yourself, you'll see a gorgeous ANSI-colored output. Don't worry, the terminal colors are automatically stripped when piped to a file or consumed by an agent's shell hook!

### Rust

```
# src/core.rs (490 lines, 3 types, 12 methods, 5 fields)
pub struct Declaration  L10-120
    pub kind: DeclarationKind  L12
    pub name: String  L15
    pub fn lines_suffix(&self) -> String  L30-48
```

### `show` with ancestor context

`ast-outline show <file> <Symbol>` prints a `# in: ...` breadcrumb
between the header and the body so you know what the extracted code is
nested inside, without a second `outline` call:

```
# Player.cs:30-48  Game.Player.PlayerController.TakeDamage  (method)
# in: namespace Game.Player → public class PlayerController : MonoBehaviour, IDamageable
/// <summary>Apply damage.</summary>
public void TakeDamage(int amount) { ... }
```

---

## JSON output

Add `--json` to any command to get the full symbol graph as stable,
structured JSON instead of formatted text — ideal for editors, language
servers, CI tooling, or any script that needs to consume the data
programmatically.

```bash
ast-outline src/player.rs --json            # per-file outline
ast-outline digest src/ --json              # digest view
ast-outline show Player.cs TakeDamage --json
ast-outline implements IDamageable src/ --json
ast-outline src/ --json --compact           # single-line (no pretty-print)
```

Every JSON document includes a `schema` field that is bumped on breaking
changes, so downstream tooling can guard on it:

```json
{
  "schema": "ast-outline.outline.v1",
  "files": [
    {
      "path": "src/player.rs",
      "language": "rust",
      "line_count": 312,
      "error_count": 0,
      "declarations": [
        {
          "kind": "struct",
          "name": "Player",
          "signature": "pub struct Player",
          "visibility": "pub",
          "start_line": 10,
          "end_line": 40,
          "children": [ ... ]
        }
      ]
    }
  ]
}
```

| Schema | Command |
|--------|----------|
| `ast-outline.outline.v1` | default outline, `digest --json` |
| `ast-outline.show.v1` | `show --json` |
| `ast-outline.implements.v1` | `implements --json` |
| `ast-outline.surface.v1` | `surface --json` |
| `ast-outline.search.v1` | `search --json` |
| `ast-outline.related.v1` | `find-related --json` |
| `ast-outline.index-stats.v1` | `index --stats --json` |

---

## MCP server

Run `ast-outline` as a [Model Context Protocol](https://modelcontextprotocol.io)
server over stdio so any MCP-aware coding agent can call the same operations
as native tools — no shell parsing required:

```bash
ast-outline mcp
```

The server speaks line-delimited JSON-RPC 2.0 on stdin/stdout and exposes eight
tools that map 1:1 to the CLI commands:

| Tool | Equivalent CLI | Returns |
|------|----------------|---------|
| `outline`      | `ast-outline <paths>`                    | text, or `ast-outline.outline.v1` with `json: true` |
| `digest`       | `ast-outline digest <paths>`             | text, or `ast-outline.outline.v1` with `json: true` |
| `show`         | `ast-outline show <path> <syms>`         | text, or `ast-outline.show.v1` with `json: true` |
| `implements`   | `ast-outline implements <type> <paths>`  | text, or `ast-outline.implements.v1` with `json: true` |
| `surface`      | `ast-outline surface [path]`             | text, or `ast-outline.surface.v1` with `json: true` |
| `search`       | `ast-outline search "<query>"`           | text, or `ast-outline.search.v1` with `json: true` |
| `find_related` | `ast-outline find-related <file>:<line>` | text, or `ast-outline.related.v1` with `json: true` |
| `index`        | `ast-outline index`                      | text, or `ast-outline.index-stats.v1` with `json: true` |

Wire it into a client by pointing at the binary:

```jsonc
{
  "mcpServers": {
    "ast-outline": { "command": "ast-outline", "args": ["mcp"] }
  }
}
```

The server is fully synchronous, has no extra runtime dependencies, and adds
roughly 1% to the binary size. The CLI itself is unaffected — none of the MCP
code runs unless you invoke the `mcp` subcommand.

---

## Semantic search

`ast-outline search` runs hybrid retrieval over a per-repo index:

- **BM25** for exact identifier matches and keyword density.
- **Dense embeddings** via [`minishlab/potion-code-16M`](https://huggingface.co/minishlab/potion-code-16M) — a static (no inference) `vocab × 256` model that runs on CPU in microseconds.
- **Reciprocal Rank Fusion** (k = 60) blends the two; alpha auto-resolves to 0.3 for symbol queries (`HandlerStack`, `Sinatra::Base` — lean BM25) and 0.5 for natural language ("how does login work" — balanced).
- A ranking pass adds definition boosts (3× for chunks that *define* a queried symbol), file-coherence boosts (multi-chunk hits in the same file lift the top chunk), file-stem matches for NL queries, and path-based penalties (test files 0.3×, `.d.ts` stubs 0.7×, `__init__.py` 0.5×).

`ast-outline find-related <file>:<line>` is the same engine in semantic-only mode, language-filtered, with the source chunk excluded — useful for "what else is structured like this?"

```bash
ast-outline search "request validation" -k 5
ast-outline search "HandlerStack" --json
ast-outline find-related src/auth/login.rs:42 -k 3
```

### How indexing works

First call to `search` / `find-related` builds an index at `.ast-outline/index/`:

```
.ast-outline/
  .gitignore           # auto-written, contents: "*"
  index/
    meta.json          # schema + model + chunk_count
    chunks.bin         # per-chunk content + line range + language
    embeddings.f32     # chunk_count × 256 little-endian f32, mmap-friendly
    bm25.bin           # vocab + idf + postings
    files.bin          # per-file mtime + xxhash + chunk range
    lock               # advisory lock for concurrent writers
```

Subsequent calls walk the tree, compare `(mtime, size)` against `files.bin`, and only hash files where the cheap check fails. If anything changed, the index rebuilds automatically (a v2 will support partial updates against the same on-disk format). Steady-state cost on an unchanged 10k-file repo: ~30 ms of stat syscalls.

The model is downloaded once (~64 MB) on first use to `~/.cache/ast-outline/models/`. It tries HuggingFace first, falls back to `hf-mirror.com` if blocked. **TLS verification is disabled by default** so corporate MITM proxies don't break setup; integrity is enforced via SHA-256 on every cached file. Set `AST_OUTLINE_TLS_STRICT=1` to enforce strict TLS.

For more on what gets indexed (the five filter layers, `.ast-outline-ignore` syntax) see the "What gets walked" section above. For the security trade-offs around the TLS default, see the [network-security wiki page](https://github.com/aeroxy/ast-outline/blob/main/wiki/network-security.md) on GitHub.

---

## Architecture & Development

See the [wiki](https://github.com/aeroxy/ast-outline/blob/main/wiki/architecture.md) on GitHub for details on how `ast-outline` leverages `ast-grep` internally and how you can add new language adapters.

### Getting started

```bash
git clone https://github.com/aeroxy/ast-outline.git
cd ast-outline

# With Cargo
cargo run -- digest src/

# With Nix flake
nix develop        # Enter development shell
nix build          # Build the project
nix flake check    # Run all checks (tests, clippy, formatting)
```

Contributions welcome.

---

## License

[MIT](./LICENSE)
