# ast-outline

Fast, AST-based **structural outline** for source files — classes, methods,
signatures with line numbers, but **no method bodies**. Built for LLM coding
agents and humans who want to read the *shape* of a file before diving into the whole thing.

`ast-outline` is written in Rust, leveraging the incredibly fast [ast-grep](https://github.com/ast-grep/ast-grep) bindings for [tree-sitter](https://tree-sitter.github.io/tree-sitter/), and it utilizes `rayon` to parse your entire workspace concurrently in milliseconds.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
![Status: beta](https://img.shields.io/badge/status-beta-orange.svg)

---

## Purpose

**`ast-outline` exists to make LLM coding agents faster, cheaper, and smarter
when navigating unfamiliar code.**

Modern agentic coding tools explore codebases by reading files directly — not via embeddings or vector search. That approach is reliable but has a massive cost: on a 1000-line file, the agent pays for 1000 lines of tokens just to answer *"what methods exist here?"*.

`ast-outline` closes that gap. It's a **pre-reading layer**:

1. **Token savings — typically 5–10×.** An outline replaces a full file read when you only need structural understanding.
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
Agent: ast-outline digest src/Combat         # ~100 lines, whole module
Agent: ast-outline implements IDamageable    # precise list, no grep noise
Agent: ast-outline show Player.cs TakeDamage # just the method body
```

Result: **same understanding, a fraction of the tokens, a fraction of the round-trips.**

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

# Every class that inherits/implements a given type
ast-outline implements IDamageable src/

# Output a prompt snippet to steer LLM agents
ast-outline prompt >> AGENTS.md
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

## Architecture & Development

See the [`wiki/`](./wiki/architecture.md) directory for details on how `ast-outline` leverages `ast-grep` internally and how you can add new language adapters.

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
