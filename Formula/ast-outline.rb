class AstOutline < Formula
  desc "Fast AST-based code-navigation toolkit: shape, surface, deps, search"
  homepage "https://github.com/aeroxy/ast-outline"
  url "https://github.com/aeroxy/ast-outline/releases/download/2.0.1/ast-outline-macos-arm64.zip"
  sha256 "a76c4e384a0dd155a42b6dc7b2fe4f125de7c5ede04ddb8e7ee8fbab51fc0f34"
  license "MIT"

  def install
    bin.install "ast-outline"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/ast-outline --version")
  end
end
