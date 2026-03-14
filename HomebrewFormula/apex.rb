class Apex < Formula
  desc "Autonomous path exploration — coverage-guided security analysis"
  homepage "https://github.com/allexdav2/apex"
  license "MIT"
  version "0.1.0"

  on_macos do
    on_intel do
      url "https://github.com/allexdav2/apex/releases/download/v#{version}/apex-x86_64-apple-darwin.tar.gz"
      # sha256 "UPDATE_AFTER_FIRST_RELEASE"
    end
    on_arm do
      url "https://github.com/allexdav2/apex/releases/download/v#{version}/apex-aarch64-apple-darwin.tar.gz"
      # sha256 "UPDATE_AFTER_FIRST_RELEASE"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/allexdav2/apex/releases/download/v#{version}/apex-x86_64-unknown-linux-gnu.tar.gz"
      # sha256 "UPDATE_AFTER_FIRST_RELEASE"
    end
    on_arm do
      url "https://github.com/allexdav2/apex/releases/download/v#{version}/apex-aarch64-unknown-linux-gnu.tar.gz"
      # sha256 "UPDATE_AFTER_FIRST_RELEASE"
    end
  end

  def install
    bin.install "apex"
  end

  test do
    assert_match "apex", shell_output("#{bin}/apex --version")
  end
end
