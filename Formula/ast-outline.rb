class AstOutline < Formula
  desc "Fast AST-based code-navigation toolkit: shape, surface, deps, search"
  homepage "https://github.com/aeroxy/ast-outline"
  url "https://github.com/aeroxy/ast-outline/releases/download/1.1.0/ast-outline-macos-arm64.zip"
  sha256 "22b58ee7538a4bb814422eb2191a5d4742872ea4138a91c9e13d326822872bad"
  license "MIT"

  def install
    bin.install "ast-outline"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/ast-outline --version")
  end
end
