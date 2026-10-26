#!/usr/bin/env bash
# Package a built kav binary the way releases ship it.
#
#   scripts/package.sh <kav-binary> <platform> <tag> [out-dir]
#
# <platform> is the download directory and file prefix: macos, linux or linux-arm64.
# Writes, under <out-dir> (default: dist):
#   <platform>/kav-<platform>-latest.tar.gz           (+ .sha256)
#   <platform>/archive/kav-<platform>-<tag>.tar.gz    (+ .sha256)
# Each tarball holds kav, kav.1, LICENSE and uninstall.sh. A .sha256 file holds just
# the hex digest, the format downloads.appachi.tech has always served.
set -euo pipefail

if [ $# -lt 3 ]; then
    echo "usage: $0 <kav-binary> <platform> <tag> [out-dir]" >&2
    exit 2
fi
bin=$1 platform=$2 tag=$3 out=${4:-dist}
root=$(cd "$(dirname "$0")/.." && pwd)

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

install -m 755 "$bin" "$stage/kav"
"$stage/kav" man > "$stage/kav.1"
chmod 644 "$stage/kav.1"
install -m 644 "$root/LICENSE" "$stage/LICENSE"
install -m 755 "$root/scripts/uninstall.sh" "$stage/uninstall.sh"

# Root-owned entries without extended attributes, from GNU tar and macOS bsdtar alike.
if tar --version 2>/dev/null | grep -q 'GNU tar'; then
    owner=(--owner=0 --group=0 --numeric-owner)
else
    owner=(--uid 0 --gid 0 --numeric-owner)
fi

mkdir -p "$out/$platform/archive"
latest="$out/$platform/kav-$platform-latest.tar.gz"
COPYFILE_DISABLE=1 tar --no-xattrs "${owner[@]}" -czf "$latest" -C "$stage" kav kav.1 LICENSE uninstall.sh

if command -v sha256sum > /dev/null; then
    sha256sum "$latest" | awk '{print $1}' > "$latest.sha256"
else
    shasum -a 256 "$latest" | awk '{print $1}' > "$latest.sha256"
fi

versioned="$out/$platform/archive/kav-$platform-$tag.tar.gz"
cp "$latest" "$versioned"
cp "$latest.sha256" "$versioned.sha256"
echo "$versioned $(cat "$versioned.sha256")"
