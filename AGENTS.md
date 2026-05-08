## Preparing Release

1. Bump the version in `Cargo.toml`.
2. Build the release binary: `cargo build --release`
3. Zip the binary inside the release folder: `zip -j target/release/ast-outline-macos-arm64.zip target/release/ast-outline`
4. Calculate the SHA256: `shasum -a 256 target/release/ast-outline-macos-arm64.zip`
5. Update `Formula/ast-outline.rb` with the new version, URL, and SHA256.

## WIKI

@wiki/architecture.md

The architecture page links to four deeper wiki files — read them on-demand when your work touches that subsystem:

- [wiki/deps.md](wiki/deps.md) — dependency-graph internals (deps / reverse-deps / cycles / graph)
- [wiki/search.md](wiki/search.md) — semantic search internals (BM25 + dense, chunking, on-disk format)
- [wiki/network-security.md](wiki/network-security.md) — model download, TLS policy, mirror fallback
- [wiki/file-filtering.md](wiki/file-filtering.md) — what gets walked, ignore layers, escape hatches
