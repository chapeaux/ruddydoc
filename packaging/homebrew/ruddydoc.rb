class Ruddydoc < Formula
  desc "Fast document conversion with embedded knowledge graph"
  homepage "https://github.com/chapeaux/ruddydoc"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/chapeaux/ruddydoc/releases/download/v0.1.0/ruddydoc-v0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_ARM64_MACOS_SHA256"
    else
      url "https://github.com/chapeaux/ruddydoc/releases/download/v0.1.0/ruddydoc-v0.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_X86_64_MACOS_SHA256"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/chapeaux/ruddydoc/releases/download/v0.1.0/ruddydoc-v0.1.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_ARM64_LINUX_SHA256"
    else
      url "https://github.com/chapeaux/ruddydoc/releases/download/v0.1.0/ruddydoc-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_X86_64_LINUX_SHA256"
    end
  end

  def install
    bin.install "ruddydoc"
  end

  test do
    system "#{bin}/ruddydoc", "--version"
  end
end
