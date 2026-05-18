class Argyph < Formula
  desc "Local-first MCP server giving AI coding agents fast, structured, and semantic context over any codebase"
  homepage "https://github.com/Ezzy1630/argyph"
  version "1.0.4"
  license "MIT OR Apache-2.0"

  # Prebuilt binaries from cargo-dist. SHA256 values are filled in
  # automatically by scripts/update-homebrew.sh after each tagged release.
  on_macos do
    on_arm do
      url "https://github.com/Ezzy1630/argyph/releases/download/v1.0.4/argyph-aarch64-apple-darwin.tar.xz"
      sha256 "a93d1555eb9b9c0d0740288118589e257a1a0584e1c8b0a5687665cb3e4ace53"
    end
    # Intel Mac: no prebuilt available (ort/ONNX Runtime does not ship
    # an x86_64-apple-darwin binary). Fall back to building from source
    # via cargo. Homebrew will install rust as a build-time dependency.
  end

  on_linux do
    on_arm do
      url "https://github.com/Ezzy1630/argyph/releases/download/v1.0.4/argyph-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "b1a0190c10362afbffe8257e57dd62d7b94d0be61ea31f4bfb284a16b2b137b1"
    end
    on_intel do
      url "https://github.com/Ezzy1630/argyph/releases/download/v1.0.4/argyph-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "745ab83b8a8af8b783b956c4e36e7e3c0f4c35979f94dec1de93159dd7b30370"
    end
  end

  def install
    if OS.mac? && Hardware::CPU.intel?
      odie <<~EOS
        Argyph does not ship a prebuilt binary for Intel macOS because the
        bundled ONNX Runtime backend has no x86_64-apple-darwin binary.
        Install via cargo instead:

            cargo install argyph --locked
      EOS
    end
    bin.install "argyph"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/argyph --version")
  end
end
