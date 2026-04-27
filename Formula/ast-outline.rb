class AstOutline < Formula
  desc "Fast, AST-based structural outline for source files"
  homepage "https://github.com/aeroxy/ast-outline"
  url "https://github.com/aeroxy/ast-outline/releases/download/0.1.2/ast-outline-macos-arm64.zip"
  sha256 "3ee17d16acaa8363e24d2bd5d4aa3298ea5b84d6e3d8147645b2bd769447f429"
  license "MIT"

  def install
    bin.install "ast-outline"
  end

  test do
    # Run the help command to ensure the binary is functional
    assert_match "Usage: ast-outline", shell_output("#{bin}/ast-outline --help")
  end
end
