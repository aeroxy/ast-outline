## Preparing Release

1. Bump the version in `Cargo.toml`.
2. Build the release binary: `cargo build --release`
3. Zip the binary inside the release folder: `zip -j target/release/ast-outline-macos-arm64.zip target/release/ast-outline`
4. Calculate the SHA256: `shasum -a 256 target/release/ast-outline-macos-arm64.zip`
5. Update `Formula/ast-outline.rb` with the new version, URL, and SHA256.

## WIKI

@wiki/architecture.md
@wiki/network-security.md
@wiki/file-filtering.md
@wiki/search.md
@wiki/deps.md
