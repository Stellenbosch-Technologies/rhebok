#!/bin/sh
set -eu

version="${1:?usage: scripts/homebrew-formula.sh <version>, once v<version> is released}"
release="https://github.com/Stellenbosch-Technologies/rhebok/releases/download/v$version"

sha256_of() {
    sum="$(curl -fsSL "$release/rhebok-$1.sha256")"
    sum="${sum%% *}"
    case "$sum" in
        *[!0-9a-f]* | "") echo "no checksum for rhebok-$1 in v$version" >&2; exit 1 ;;
    esac
    if [ "${#sum}" -ne 64 ]; then
        echo "no checksum for rhebok-$1 in v$version" >&2
        exit 1
    fi
    echo "$sum"
}
mac="$(sha256_of aarch64-apple-darwin)"
linux_intel="$(sha256_of x86_64-unknown-linux-gnu)"
linux_arm="$(sha256_of aarch64-unknown-linux-gnu)"

cat <<FORMULA
class Rhebok < Formula
  desc "Local semantic search over your notes and past sessions for coding agents"
  homepage "https://github.com/Stellenbosch-Technologies/rhebok"
  version "$version"
  license "AGPL-3.0-only"

  on_macos do
    depends_on arch: :arm64
    url "$release/rhebok-aarch64-apple-darwin.tar.gz"
    sha256 "$mac"
  end

  on_linux do
    on_intel do
      url "$release/rhebok-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "$linux_intel"
    end
    on_arm do
      url "$release/rhebok-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "$linux_arm"
    end
  end

  def install
    bin.install "rhebok"
    prefix.install "LICENSE", "THIRD-PARTY-NOTICES.md"
  end

  def caveats
    <<~EOS
      Finish setting rhebok up. This registers its Claude Code plugin, offers to
      connect your other agents and downloads the embedding model:
        rhebok install

      rhebok needs Ollama running: https://ollama.com
      To update, run rhebok update: it runs brew upgrade rhebok, then rhebok install.
    EOS
  end

  test do
    assert_equal "rhebok #{version}", shell_output("#{bin}/rhebok --version").strip
    assert_path_exists prefix/"LICENSE"
    assert_path_exists prefix/"THIRD-PARTY-NOTICES.md"
  end
end
FORMULA
