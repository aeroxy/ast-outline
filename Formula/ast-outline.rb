class AstOutline < Formula
  desc "Fast, AST-based structural outline for source files"
  homepage "https://github.com/aeroxy/ast-outline"
  url "https://github.com/aeroxy/ast-outline/releases/download/0.5.0/ast-outline-macos-arm64.zip"
  sha256 "f86112229eb79ef820d8c12f14c395ace2d2bc37502f7eb15bcc6dafa3ce1e53"
  license "MIT"

  def install
    bin.install "ast-outline"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/ast-outline --version")
  end
end
