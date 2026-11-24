#!/usr/bin/env bash
# Download the static LLVM built by llvm-build.yml for one target, check it
# against the SHA-256 pinned in SHA256SUMS beside this script, and unpack it.
#
#   fetch.sh <rust-target-triple> <dest-dir>
#
# Prints the unpacked prefix, the value for LLVM_SYS_211_PREFIX, as its only
# line on standard output; everything else goes to standard error. A prefix
# already unpacked (a cache hit) is used as it is. The release tag and version are pinned here; a new LLVM build gets a
# new tag and new checksums, never a replaced asset.
set -euo pipefail

version=21.1.8
tag=llvm-$version-libjay-1
triple=$1
here() { pwd -W 2>/dev/null || pwd; }
dir=$(cd "$(dirname "$0")" && pwd)
dest=$(mkdir -p "$2" && cd "$2" && here)

name=llvm-$version-$triple
asset=$name.tar.xz
want=$(awk -v a="$asset" '$2 == a || $2 == "*" a { print $1 }' "$dir/SHA256SUMS")
if [ -z "$want" ]; then
  echo "no pinned checksum for $asset in .github/llvm/SHA256SUMS:" >&2
  echo "run the LLVM workflow (llvm-build.yml) and commit the SHA256SUMS it publishes" >&2
  exit 1
fi

repo=${GITHUB_REPOSITORY:-amyodov/libjay}
if [ ! -d "$dest/$name" ]; then
  curl -fsSL --retry 5 -o "$dest/$asset" \
    "https://github.com/$repo/releases/download/$tag/$asset"
  got=$(cd "$dest" && cmake -E sha256sum "$asset" | cut -d' ' -f1)
  if [ "$got" != "$want" ]; then
    echo "checksum mismatch for $asset: got $got, pinned $want" >&2
    exit 1
  fi
  (cd "$dest" && cmake -E tar xf "$asset" && rm "$asset")
fi
"$dest/$name/bin/llvm-config" --version >&2
echo "$dest/$name"
