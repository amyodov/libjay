#!/usr/bin/env bash
# Build the static LLVM that libjay's wheels and C bundles link, for the
# machine this runs on, and pack it as one archive.
#
#   build.sh <llvm-version> <rust-target-triple> <out-dir>
#
# Only what llvm-sys needs is built and installed: the static libraries, the
# headers and llvm-config. One target (the host's), no assertions, and none
# of zlib, zstd, libxml2, libedit or libpfm, so `llvm-config --system-libs`
# names only the platform's own libraries. On Windows the runtime is /MD, as
# Rust's MSVC targets use. MACOSX_DEPLOYMENT_TARGET, where set, is passed on.
# The source tarball is checked against the SHA-256 below before use.
#
# Runs on Linux (inside the manylinux image the wheels are built in), macOS
# and Windows (Git Bash, with the MSVC environment loaded). Needs cmake,
# ninja and a C++ compiler on PATH.
set -euo pipefail

version=$1
triple=$2
# A path native Windows tools understand (D:/a/...), elsewhere the plain one.
here() { pwd -W 2>/dev/null || pwd; }
out=$(mkdir -p "$3" && cd "$3" && here)

# llvm-project-<version>.src.tar.xz from the llvm/llvm-project GitHub release.
case $version in
  21.1.8) src_sha256=4633a23617fa31a3ea51242586ea7fb1da7140e426bd62fc164261fe036aa142 ;;
  *) echo "no pinned source checksum for LLVM $version" >&2; exit 1 ;;
esac

case $triple in
  x86_64-*) llvm_target=X86 ;;
  aarch64-*) llvm_target=AArch64 ;;
  *) echo "unknown target $triple" >&2; exit 1 ;;
esac

mkdir -p llvm-work
cd llvm-work
work=$(here)

tarball=llvm-project-$version.src.tar.xz
if [ ! -f "$tarball" ]; then
  curl -fsSL --retry 5 -o "$tarball" \
    "https://github.com/llvm/llvm-project/releases/download/llvmorg-$version/$tarball"
fi
got=$(cmake -E sha256sum "$tarball" | cut -d' ' -f1)
if [ "$got" != "$src_sha256" ]; then
  echo "checksum mismatch for $tarball: $got" >&2
  exit 1
fi
rm -rf "llvm-project-$version.src"
cmake -E tar xf "$tarball"

name=llvm-$version-$triple
prefix=$work/$name
rm -rf "$prefix" build

extra=()
case $triple in
  *-windows-msvc)
    # Git Bash has a `link` of its own; name MSVC's, which sits beside cl.
    msvc_bin=$(cygpath -m "$(dirname "$(command -v cl)")")
    extra+=(-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL -DLLVM_ENABLE_DIA_SDK=OFF
            -DCMAKE_C_COMPILER=cl -DCMAKE_CXX_COMPILER=cl
            "-DCMAKE_LINKER=$msvc_bin/link.exe")
    ;;
  *-apple-darwin)
    extra+=("-DCMAKE_OSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:?set MACOSX_DEPLOYMENT_TARGET}")
    ;;
esac

cmake -S "llvm-project-$version.src/llvm" -B build -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$prefix" \
  -DLLVM_TARGETS_TO_BUILD="$llvm_target" \
  -DLLVM_ENABLE_PROJECTS= \
  -DLLVM_ENABLE_RUNTIMES= \
  -DLLVM_ENABLE_ASSERTIONS=OFF \
  -DLLVM_BUILD_LLVM_DYLIB=OFF \
  -DLLVM_LINK_LLVM_DYLIB=OFF \
  -DLLVM_ENABLE_PIC=ON \
  -DLLVM_ENABLE_ZLIB=OFF \
  -DLLVM_ENABLE_ZSTD=OFF \
  -DLLVM_ENABLE_LIBXML2=OFF \
  -DLLVM_ENABLE_LIBEDIT=OFF \
  -DLLVM_ENABLE_LIBPFM=OFF \
  -DLLVM_ENABLE_CURL=OFF \
  -DLLVM_ENABLE_HTTPLIB=OFF \
  -DLLVM_ENABLE_BINDINGS=OFF \
  -DLLVM_INCLUDE_TESTS=OFF \
  -DLLVM_INCLUDE_BENCHMARKS=OFF \
  -DLLVM_INCLUDE_EXAMPLES=OFF \
  -DLLVM_INCLUDE_DOCS=OFF \
  -DLLVM_TOOL_LTO_BUILD=OFF \
  -DLLVM_TOOL_REMARKS_SHLIB_BUILD=OFF \
  ${extra[@]+"${extra[@]}"}

# The install targets build what they install and nothing more: every
# library, the headers, and llvm-config. No other tool is compiled.
cmake --build build --target install-llvm-libraries install-llvm-headers install-llvm-config

"$prefix/bin/llvm-config" --version
"$prefix/bin/llvm-config" --targets-built
"$prefix/bin/llvm-config" --system-libs --link-static
# What llvm-sys asks for; it fails if any library it names is missing.
"$prefix/bin/llvm-config" --libnames --link-static > /dev/null
cp "llvm-project-$version.src/llvm/LICENSE.TXT" "$prefix/"

cd "$work"
cmake -E tar cJf "$out/$name.tar.xz" "$name"
cd "$out"
cmake -E sha256sum "$name.tar.xz" > "$name.tar.xz.sha256"
cat "$name.tar.xz.sha256"
ls -l "$out"
