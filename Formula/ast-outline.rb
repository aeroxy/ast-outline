class AstOutline < Formula
  desc "Fast, AST-based structural outline for source files"
  homepage "https://github.com/aeroxy/ast-outline"
  url "https://github.com/aeroxy/ast-outline/releases/download/0.4.2/ast-outline-macos-arm64.zip"
  sha256 "4e5c02528a4f7f3e32e44745c1c5078b396931e972c70b8242710e310cd64651"
  license "MIT"

  def install
    bin.install "ast-outline"
  end

  test do
    # Run the help command to ensure the binary is functional
    assert_match "Usage: ast-outline", shell_output("#{bin}/ast-outline --help")
  end
end
