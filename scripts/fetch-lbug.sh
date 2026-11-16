#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

fail() {
    echo "fetch-lbug: $*" >&2
    exit 1
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

pinned="$(sed -n 's/^LBUG_VERSION = { value = "\([^"]*\)", force = true }$/\1/p' .cargo/config.toml)"
[ -n "$pinned" ] || fail ".cargo/config.toml does not pin LBUG_VERSION with force = true"
locked="$(awk '$0 == "name = \"lbug\"" { getline; print; exit }' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')"
[ "$pinned" = "$locked" ] ||
    fail "Cargo.lock locks lbug ${locked:-nothing} but .cargo/config.toml pins LBUG_VERSION $pinned; change both, and the checksums here, together"

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) host=aarch64-apple-darwin ;;
    Linux-x86_64) host=x86_64-unknown-linux-gnu ;;
    Linux-aarch64) host=aarch64-unknown-linux-gnu ;;
    *) fail "unsupported host $(uname -s)-$(uname -m)" ;;
esac
target="${1:-$host}"
[ "$target" = "$host" ] ||
    fail "lbug's build script downloads for the host, so $target can only be fetched on a $target machine (this is $host)"

case "$pinned $target" in
    "0.19.1 aarch64-apple-darwin")
        archive=liblbug-static-osx-arm64.tar.gz
        expected=9d8bf7fd2a2b715e419db1f087f57777fd9413e214abdf32fa60ca3a9e51d883
        ;;
    "0.19.1 x86_64-unknown-linux-gnu")
        archive=liblbug-static-linux-x86_64-compat.tar.gz
        expected=6d706ddb7317f3d816685bd616a252df002ddeb6f270a3a7eb875a301f3aa266
        ;;
    "0.19.1 aarch64-unknown-linux-gnu")
        archive=liblbug-static-linux-aarch64-compat.tar.gz
        expected=a357add94b764336da41aa0d3f2406c13bf7648849e91e8f172608041a6ceaac
        ;;
    *) fail "no recorded checksum for LadybugDB $pinned on $target; add one to scripts/fetch-lbug.sh" ;;
esac

paths="$(cargo metadata --format-version 1 --locked | python3 -c '
import json, os, sys
meta = json.load(sys.stdin)
crates = [p for p in meta["packages"] if p["name"] == "lbug" and p["source"]]
if len(crates) != 1:
    sys.exit("expected one registry lbug package, found %d" % len(crates))
print(os.path.dirname(crates[0]["manifest_path"]))
print(meta["target_directory"])
')" || fail "cargo metadata could not locate the lbug crate"
crate_dir="$(printf '%s\n' "$paths" | sed -n 1p)"
target_dir="$(printf '%s\n' "$paths" | sed -n 2p)"

downloads="$target_dir/lbug-prebuilt/$pinned"
mkdir -p "$downloads"
download="$downloads/$archive"
if [ ! -f "$download" ] || [ "$(sha256_of "$download")" != "$expected" ]; then
    rm -f "$download" "$download.part"
    curl -fsSL --retry 3 -o "$download.part" \
        "https://github.com/LadybugDB/ladybug/releases/download/v$pinned/$archive"
    got="$(sha256_of "$download.part")"
    if [ "$got" != "$expected" ]; then
        rm -f "$download.part"
        fail "$archive for v$pinned has sha256 $got, expected $expected; refusing to build against it"
    fi
    mv "$download.part" "$download"
fi

lib_dir="$crate_dir/.cache/lbug-prebuilt/version-$pinned/lib"
staging="$lib_dir.staging.$$"
rm -rf "$staging"
mkdir -p "$staging"
tar -xzf "$download" -C "$staging"
unchanged=yes
for f in liblbug.a lbug.h lbug.hpp; do
    [ -f "$staging/$f" ] || { rm -rf "$staging"; fail "$archive has no $f"; }
    cmp -s "$staging/$f" "$lib_dir/$f" || unchanged=no
done
if [ "$unchanged" = yes ] && [ "$(ls "$lib_dir" | wc -l | tr -d ' ')" -eq 3 ]; then
    rm -rf "$staging"
    echo "fetch-lbug: LadybugDB $pinned for $target, sha256 $expected, already in $lib_dir"
    exit 0
fi
rm -rf "$lib_dir"
mv "$staging" "$lib_dir"
echo "fetch-lbug: LadybugDB $pinned for $target, sha256 $expected, unpacked into $lib_dir"
