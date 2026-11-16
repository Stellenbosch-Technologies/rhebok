#!/bin/sh
# Build, package and smoke-test a release tarball on this machine, the same
# way .github/workflows/release.yml does, for when CI cannot run.
#
#   scripts/build-release.sh <notices>             # the host's release target
#   scripts/build-release.sh <notices> <target>    # e.g. aarch64-apple-darwin
#
# Writes dist/rhebok-<target>.tar.gz and dist/rhebok-<target>.sha256.
#
# RUSTFLAGS replaces .cargo/config.toml's per-target flags wholesale, so the
# export-dynamic flag is repeated here.
#
# Compiled-in paths (Rust panic locations, including the standard library's,
# C++ __FILE__ strings from lbug, usearch and cxx, and pdf-inspector's
# CARGO_MANIFEST_DIR) would otherwise carry this machine's home directory. The build therefore runs through
# neutral symlinks under /tmp/rhebok-build, and Rust paths are remapped too.
set -eu

notices="${1:?usage: scripts/build-release.sh <THIRD-PARTY-NOTICES.md> [<target>]}"
[ -s "$notices" ] || {
    echo "build-release: $notices is missing or empty; generate it with scripts/third-party-notices.sh" >&2
    exit 1
}
notices="$(cd "$(dirname "$notices")" && pwd -P)/$(basename "$notices")"

cd "$(dirname "$0")/.."
repo="$(pwd -P)"

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64)  host=aarch64-apple-darwin ;;
    Linux-x86_64)  host=x86_64-unknown-linux-gnu ;;
    Linux-aarch64) host=aarch64-unknown-linux-gnu ;;
    *) echo "build-release: unsupported host $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac
target="${2:-$host}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
real_target="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
real_cargo="${CARGO_HOME:-$HOME/.cargo}"

build=/tmp/rhebok-build
mkdir -p "$build" "$real_target"
ln -sfn "$real_cargo" "$build/cargo"
ln -sfn "$real_target" "$build/target"
export CARGO_HOME="$build/cargo"
export CARGO_TARGET_DIR="$build/target"
target_dir="$build/target"

# With rustup's rust-src component installed, panic locations inside the
# standard library point at the local toolchain instead of /rustc/<hash>.
sysroot="$(rustc --print sysroot)"

remap="--remap-path-prefix=$repo=/rhebok --remap-path-prefix=$build=/build --remap-path-prefix=$sysroot=/rustc-sysroot"

case "$target" in
    *-apple-darwin)
        # Link OpenSSL statically without touching Homebrew: the linker takes
        # libssl.a only from a directory that has no libssl.dylib, so point it
        # at one holding just the archives.
        keg="$(brew --prefix openssl@3)/lib"
        static="$(mktemp -d)"
        trap 'rm -rf "$static"' EXIT
        ln -s "$keg/libssl.a" "$keg/libcrypto.a" "$static/"
        export RUSTFLAGS="-L $static -C link-args=-Wl,-export_dynamic $remap"
        ;;
    *-linux-gnu)
        export RUSTFLAGS="-C link-args=-Wl,--export-dynamic $remap"
        ;;
    *) echo "build-release: unsupported target $target" >&2; exit 1 ;;
esac

cargo clean -p lbug --target "$target"
scripts/fetch-lbug.sh "$target"
cargo build --release --locked --target "$target"

mkdir -p dist
archive="dist/rhebok-$target.tar.gz"
scripts/package-release.sh "$target_dir/$target/release/rhebok" "$notices" "$archive"
(cd dist && shasum -a 256 "rhebok-$target.tar.gz" > "rhebok-$target.sha256")

user="$(id -un)"
if strings "$target_dir/$target/release/rhebok" | grep -q -e "$HOME" -e "/Users/$user/" -e "/home/$user/"; then
    echo "build-release: the binary still contains $HOME" >&2
    exit 1
fi
sh scripts/release-smoke.sh "dist/rhebok-$target.tar.gz" "$version"
echo "build-release: dist/rhebok-$target.tar.gz is ready"
