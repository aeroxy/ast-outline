class AstOutline < Formula
  desc "Fast AST-based code-navigation toolkit: shape, surface, deps, search"
  homepage "https://github.com/aeroxy/ast-outline"
  url "https://github.com/aeroxy/ast-outline/releases/download/1.0.0/ast-outline-macos-arm64.zip"
  sha256 "26f24bca60d1db78cca913245edd6a081bb01642c38d63fbbd9ba1182743b2d4"
  license "MIT"

  def install
    bin.install "ast-outline"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/ast-outline --version")
  end
end
