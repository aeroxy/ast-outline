---
name: ast-outline
description: Fast AST-based structural outline for source code. Use to explore unfamiliar directories, list a file's symbols without reading bodies, jump to a specific function/class, find subclasses or implementations, search a repo by symbol or behaviour, or extract a package's true public API. Prefer this over reading whole files when you only need shape.
user-invocable: true
---

Usage: ast-outline [OPTIONS] [PATHS]... [COMMAND]

Commands:
  show          Extract source of a symbol
  digest        One-page module map
  implements    Find subclasses / implementations
  prompt        Print the agent prompt snippet
  install       Install ast-outline into a coding-agent CLI
  uninstall     Remove ast-outline from a coding-agent CLI
  status        Report what's installed where
  hook          Internal: read a tool-call event from stdin and respond
  mcp           Run as an MCP (Model Context Protocol) server over stdio
  search        Hybrid BM25 + dense semantic search over the repo
  find-related  Find chunks semantically similar to a given file:line
  index         Build, refresh, or inspect the per-repo search index
  surface       True public API surface (resolves `pub use` / `__all__`)

Options:
      --no-private
      --no-fields
      --no-docs
      --no-attrs
      --no-lines
      --glob <GLOB>
      --json         Emit output as JSON instead of text
      --compact      With --json: emit compact (single-line) JSON instead of pretty-printed

Read structure with `ast-outline` before opening full contents. Pull method bodies only once you know which ones you need.

Stop at the step that answers the question:

1. **Unfamiliar directory** — `ast-outline digest <dir>`: one-page map of every file's types and public methods.

2. **One file's shape** — `ast-outline <file>`: signatures with line ranges, no bodies (5–10× smaller than a full read).

3. **One method, class, or markdown section** — `ast-outline show <file> <Symbol>`. Suffix matching: `TakeDamage`, or `Player.TakeDamage` when ambiguous. Multiple at once: `ast-outline show Player TakeDamage Heal Die`. For markdown, the symbol is the heading text.

4. **Who implements/extends a type** — `ast-outline implements <Type> <dir>`: AST-accurate (skip `grep`), transitive by default with `[via Parent]` tags on indirect matches. Add `--direct` for level-1 only.

5. **You don't know the file or symbol name** — `ast-outline search "<query>"`: hybrid BM25 + dense semantic search over the repo. Use bare identifiers for symbol lookup (`HandlerStack`, `Sinatra::Base` — auto-leans BM25), full sentences for behaviour search ("how does login work" — auto-balances semantic + BM25). First call builds an index at `.ast-outline/index/` (~seconds for typical repos); subsequent calls reuse it and refresh incrementally.

6. **Find code similar to a chunk you already have** — `ast-outline find-related <file>:<line>`: returns chunks semantically similar to the one containing that line. Useful for "what else looks like this?" or finding alternative implementations. Pastes directly from `search` output (which prints results as `path:start-end`).

7. **The actual published API of a package** — `ast-outline surface <dir>`: resolves `pub use` re-exports (Rust) and `__all__` (Python) so you see exactly what a downstream user can reach, not the union of every `pub`/non-underscore item. Falls back to visibility-filtered output for Java/C#/Go/Kotlin (no real re-export concept). Use `--tree` for hierarchy, `--include-chain` to see the re-export path each entry took.

Fall back to a full read only when you need context beyond the body `show` returned. If the outline header contains `# WARNING: N parse errors`, the outline for that file is partial — read the source directly for the affected region.