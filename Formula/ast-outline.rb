class AstOutline < Formula
  desc "Fast AST-based code-navigation toolkit: shape, surface, deps, search"
  homepage "https://github.com/aeroxy/ast-outline"
  url "https://github.com/aeroxy/ast-outline/releases/download/1.0.1/ast-outline-macos-arm64.zip"
  sha256 "4bbe228ca5b029935c9c0548f148e2b30a07734fe18f00dccc2f8440d06164a7"
  license "MIT"

  def install
    bin.install "ast-outline"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/ast-outline --version")
  end
end
