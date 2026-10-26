#!/usr/bin/env bash
# Print the Homebrew formula for a release.
#
#   scripts/homebrew-formula.sh <tag> <macos-sha256> <linux-sha256> <linux-arm64-sha256>
#
# The release workflow writes the output to AppachiTech/homebrew-kaval as Formula/kaval.rb.
set -euo pipefail

if [ $# -ne 4 ]; then
    echo "usage: $0 <tag> <macos-sha256> <linux-sha256> <linux-arm64-sha256>" >&2
    exit 2
fi
tag=$1 macos_sha=$2 linux_sha=$3 linux_arm64_sha=$4

if ! [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "not a release tag: $tag" >&2
    exit 1
fi
for sha in "$macos_sha" "$linux_sha" "$linux_arm64_sha"; do
    if ! [[ $sha =~ ^[0-9a-f]{64}$ ]]; then
        echo "not a sha256 digest: $sha" >&2
        exit 1
    fi
done

version=${tag#v}
base=https://downloads.appachi.tech

cat <<EOF
class Kaval < Formula
  desc "Guard your ports: a developer-focused port and process manager TUI"
  homepage "https://www.appachi.tech/kaval/"
  version "${version}"
  license "MIT"

  on_macos do
    # One universal binary for Apple Silicon and Intel.
    url "${base}/macos/archive/kav-macos-${tag}.tar.gz"
    sha256 "${macos_sha}"
  end

  on_linux do
    on_intel do
      url "${base}/linux/archive/kav-linux-${tag}.tar.gz"
      sha256 "${linux_sha}"
    end
    on_arm do
      url "${base}/linux-arm64/archive/kav-linux-arm64-${tag}.tar.gz"
      sha256 "${linux_arm64_sha}"
    end
  end

  def install
    bin.install "kav"
    man1.install "kav.1"
    generate_completions_from_executable(bin/"kav", "completions")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/kav --version")
  end
end
EOF
